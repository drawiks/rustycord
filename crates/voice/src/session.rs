use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::Once,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use opusic_c::{Application, Bitrate, Channels, Decoder, Encoder, InbandFec, SampleRate};
use serde_json::{Value, json};
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    time::{self, Instant},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use tracing::{debug, info, trace, warn};

use crate::{
    AudioEngine, CHANNELS, CryptoMode, Error, FRAME_SAMPLES, OPUS_PAYLOAD_TYPE, Result, RtpHeader,
    TransportCrypto,
    dave::{Dave, DaveAction},
    discovery::{self, discovery_request, parse_discovery, parse_ping, ping_request},
};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Write = SplitSink<Ws, Message>;
type Read = SplitStream<Ws>;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const FRAME_VALUES: usize = FRAME_SAMPLES * CHANNELS;

#[derive(Clone, Debug)]
pub struct VoiceSession {
    pub server_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub session_id: String,
    pub token: String,
    pub endpoint: String,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
}

#[derive(Debug)]
pub enum VoiceCommand {
    Leave,
    SetMute(bool),
    SetDeafen(bool),
    SetDevices {
        input: Option<String>,
        output: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum VoiceEvent {
    Connecting,
    Ready {
        mode: String,
        dave: bool,
    },
    Speaking {
        user_id: String,
        ssrc: u32,
        speaking: bool,
    },
    ClientConnect {
        user_id: String,
    },
    ClientDisconnect {
        user_id: String,
    },
    Ping(u32),
    Disconnected {
        reason: String,
        reconnect: bool,
    },
    Error(String),
}

pub struct VoiceHandle {
    commands: UnboundedSender<VoiceCommand>,
}

impl VoiceHandle {
    pub fn leave(&self) {
        let _ = self.commands.send(VoiceCommand::Leave);
    }

    pub fn set_mute(&self, mute: bool) {
        let _ = self.commands.send(VoiceCommand::SetMute(mute));
    }

    pub fn set_deafen(&self, deafen: bool) {
        let _ = self.commands.send(VoiceCommand::SetDeafen(deafen));
    }

    pub fn set_devices(&self, input: Option<String>, output: Option<String>) {
        let _ = self
            .commands
            .send(VoiceCommand::SetDevices { input, output });
    }
}

pub fn spawn(session: VoiceSession, events: UnboundedSender<VoiceEvent>) -> VoiceHandle {
    install_crypto_provider();
    let (commands, command_rx) = unbounded_channel();
    tokio::spawn(async move {
        let _ = events.send(VoiceEvent::Connecting);
        run(session, command_rx, events).await;
    });
    VoiceHandle { commands }
}

fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

enum Outcome {
    Left,
    Closed { reason: String, reconnect: bool },
}

async fn run(
    session: VoiceSession,
    mut commands: UnboundedReceiver<VoiceCommand>,
    events: UnboundedSender<VoiceEvent>,
) {
    let mut attempt = 0u32;
    loop {
        let outcome = connect_once(&session, &mut commands, &events).await;
        let (reason, reconnect) = match outcome {
            Ok(Outcome::Left) => break,
            Ok(Outcome::Closed { reason, reconnect }) => (reason, reconnect),
            Err(error) => (error.to_string(), true),
        };
        let _ = events.send(VoiceEvent::Error(reason.clone()));
        let _ = events.send(VoiceEvent::Disconnected { reason, reconnect });
        if !reconnect {
            break;
        }
        attempt += 1;
        if attempt > 4 {
            break;
        }
        let delay = Duration::from_secs(1 << attempt.min(4));
        tokio::select! {
            _ = time::sleep(delay) => {}
            command = commands.recv() => match command {
                Some(VoiceCommand::Leave) | None => break,
                Some(_) => {}
            }
        }
    }
}

async fn connect_once(
    session: &VoiceSession,
    commands: &mut UnboundedReceiver<VoiceCommand>,
    events: &UnboundedSender<VoiceEvent>,
) -> Result<Outcome> {
    let url = format!("wss://{}/?v=9", host(&session.endpoint));
    let (ws, _) = connect_async(url.as_str()).await?;
    let (mut write, mut read) = ws.split();

    send_json(
        &mut write,
        json!({
            "op": 0,
            "d": {
                "server_id": session.server_id,
                "channel_id": session.channel_id,
                "user_id": session.user_id,
                "session_id": session.session_id,
                "token": session.token,
                "max_dave_protocol_version": 1,
            }
        }),
    )
    .await?;

    let hello = wait_for(&mut read, HANDSHAKE_TIMEOUT, 8, &mut None).await?;
    let heartbeat_ms = hello["d"]["heartbeat_interval"]
        .as_f64()
        .unwrap_or(41_250.0)
        .max(1_000.0);
    info!(
        "voice hello v={} interval={heartbeat_ms}ms",
        hello["d"]["v"].as_u64().unwrap_or_default()
    );

    let ready = wait_for(&mut read, HANDSHAKE_TIMEOUT, 2, &mut None).await?;
    let ssrc = ready["d"]["ssrc"]
        .as_u64()
        .ok_or_else(|| Error::Protocol("voice ready missing ssrc".to_string()))?
        as u32;
    let ip: IpAddr = ready["d"]["ip"]
        .as_str()
        .and_then(|ip| ip.parse().ok())
        .ok_or_else(|| Error::Protocol("voice ready missing ip".to_string()))?;
    let port = ready["d"]["port"]
        .as_u64()
        .ok_or_else(|| Error::Protocol("voice ready missing port".to_string()))?
        as u16;
    let modes: Vec<String> = serde_json::from_value(ready["d"]["modes"].clone())
        .map_err(|_| Error::Protocol("voice ready missing modes".to_string()))?;
    let mode = CryptoMode::negotiate(&modes)?;

    let bind = if ip.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let udp = UdpSocket::bind(bind).await?;
    udp.connect((ip, port)).await?;
    let public = discover(&udp, ssrc).await?;
    info!("voice udp discovered {}:{}", public.address, public.port);

    send_json(
        &mut write,
        json!({
            "op": 1,
            "d": {
                "protocol": "udp",
                "data": {
                    "address": public.address,
                    "port": public.port,
                    "mode": mode.name(),
                }
            }
        }),
    )
    .await?;

    let mut external_sender = None;
    let description = wait_for(&mut read, HANDSHAKE_TIMEOUT, 4, &mut external_sender).await?;
    let description = &description["d"];
    let secret: Vec<u8> = serde_json::from_value(description["secret_key"].clone())
        .map_err(|_| Error::Protocol("voice session missing secret key".to_string()))?;
    let mut crypto = TransportCrypto::new(mode, &secret)?;
    let dave_version = description["dave_protocol_version"].as_u64().unwrap_or(0) as u16;
    let mut dave = if dave_version > 0 {
        let channel_id = session
            .channel_id
            .parse::<u64>()
            .map_err(|_| Error::Protocol("invalid channel id".to_string()))?;
        let user_id = session
            .user_id
            .parse::<u64>()
            .map_err(|_| Error::Protocol("invalid user id".to_string()))?;
        Some(Dave::new(user_id, channel_id))
    } else {
        None
    };
    if let (Some(dave), Some(external)) = (&mut dave, &external_sender) {
        dave.set_external_sender(external);
    }
    let audio = AudioEngine::start(session.input_device.clone(), session.output_device.clone());
    let mut encoder = Encoder::new(Channels::Stereo, SampleRate::Hz48000, Application::Voip).ok();
    if let Some(encoder) = &mut encoder {
        let _ = encoder.set_bitrate(Bitrate::Value(64_000));
        let _ = encoder.set_inband_fec(InbandFec::Mode1);
        let _ = encoder.set_packet_loss(10);
    }
    let mut muted = session.self_mute;
    let mut deafened = session.self_deaf;
    let mut speaking_flag = false;
    let mut silence_frames = 0u8;
    let mut sequence = rand::random::<u16>();
    let mut timestamp = rand::random::<u32>();
    let mut decoders: HashMap<u64, Decoder> = HashMap::new();
    let mut jitter: HashMap<u64, VecDeque<Vec<f32>>> = HashMap::new();
    let mut audio_timer = time::interval(Duration::from_millis(20));
    audio_timer.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    let _ = events.send(VoiceEvent::Ready {
        mode: mode.name().to_string(),
        dave: dave_version > 0,
    });
    info!(
        "voice session ready: mode={} dave={} ssrc={ssrc}",
        mode.name(),
        dave_version
    );

    let heartbeat_interval = Duration::from_millis(heartbeat_ms as u64);
    let mut heartbeat = time::interval_at(Instant::now() + heartbeat_interval, heartbeat_interval);
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    let mut ping_timer = time::interval_at(
        Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    let mut awaiting_ack = false;
    let mut last_seq = 0u64;
    let mut ping_seq = 0u32;
    let mut pings: HashMap<u32, Instant> = HashMap::new();
    let mut ssrc_users: HashMap<u32, String> = HashMap::new();
    let mut buffer = [0u8; 2048];

    send_speaking(&mut write, ssrc, false).await?;

    loop {
        tokio::select! {
            message = read.next() => {
                let message = match message {
                    Some(Ok(message)) => message,
                    Some(Err(error)) => return Err(Error::Ws(error)),
                    None => return Ok(Outcome::Closed { reason: "voice socket closed".to_string(), reconnect: true }),
                };
                match message {
                    Message::Text(text) => {
                        let Ok(value) = serde_json::from_str::<Value>(text.as_str()) else {
                            continue;
                        };
                        let op = value["op"].as_u64().unwrap_or(u64::MAX);
                        let data = &value["d"];
                        match op {
                            5 => {
                                let ssrc = data["ssrc"].as_u64().unwrap_or_default() as u32;
                                let user_id = data["user_id"].as_str().unwrap_or_default().to_string();
                                if !user_id.is_empty() {
                                    ssrc_users.insert(ssrc, user_id.clone());
                                }
                                let speaking = data["speaking"].as_u64().unwrap_or(0) != 0;
                                let _ = events.send(VoiceEvent::Speaking { user_id, ssrc, speaking });
                            }
                            6 => awaiting_ack = false,
                            8 => {
                                if let Some(interval) = data["heartbeat_interval"].as_f64() {
                                    let interval = Duration::from_millis(interval.max(1_000.0) as u64);
                                    heartbeat = time::interval_at(Instant::now() + interval, interval);
                                    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
                                }
                            }
                            11 => {
                                for user_id in data["user_ids"].as_array().into_iter().flatten() {
                                    if let Some(user_id) = user_id.as_str() {
                                        if let Some(dave) = &mut dave
                                            && let Ok(id) = user_id.parse::<u64>()
                                        {
                                            dave.client_connect(id);
                                        }
                                        let _ = events.send(VoiceEvent::ClientConnect { user_id: user_id.to_string() });
                                    }
                                }
                            }
                            13 => {
                                let user_id = data["user_id"].as_str().unwrap_or_default().to_string();
                                if let Some(dave) = &mut dave
                                    && let Ok(id) = user_id.parse::<u64>()
                                {
                                    dave.client_disconnect(id);
                                }
                                let _ = events.send(VoiceEvent::ClientDisconnect { user_id });
                            }
                            21 => {
                                let transition_id = data["transition_id"].as_u64().unwrap_or_default() as u16;
                                let version = data["protocol_version"].as_u64().unwrap_or_default() as u16;
                                if let Some(dave) = &mut dave {
                                    let actions = dave.prepare_transition(transition_id, version);
                                    send_actions(&mut write, actions).await?;
                                }
                            }
                            22 => {
                                let transition_id = data["transition_id"].as_u64().unwrap_or_default() as u16;
                                if let Some(dave) = &mut dave {
                                    dave.execute_transition(transition_id);
                                }
                            }
                            24 => {
                                let epoch = data["epoch"].as_u64().unwrap_or_default() as u32;
                                let version = data["protocol_version"].as_u64().unwrap_or_default() as u16;
                                if let Some(dave) = &mut dave {
                                    let actions = dave.prepare_epoch(epoch, version);
                                    send_actions(&mut write, actions).await?;
                                }
                            }
                            other => debug!("voice op {other}"),
                        }
                    }
                    Message::Binary(bytes) => {
                        if bytes.len() < 3 {
                            continue;
                        }
                        let seq = u16::from_be_bytes([bytes[0], bytes[1]]) as u64;
                        last_seq = seq;
                        let op = bytes[2];
                        let payload = &bytes[3..];
                        match op {
                            25 => {
                                if let Some(dave) = &mut dave {
                                    dave.set_external_sender(payload);
                                }
                            }
                            27 => {
                                if let Some((&operation, proposals)) = payload.split_first()
                                    && let Some(dave) = &mut dave
                                {
                                    let actions = dave.proposals(operation, proposals);
                                    send_actions(&mut write, actions).await?;
                                }
                            }
                            29 => {
                                if let Some((transition, commit)) = split_transition(payload)
                                    && let Some(dave) = &mut dave
                                {
                                    let actions = dave.commit(transition, commit);
                                    send_actions(&mut write, actions).await?;
                                }
                            }
                            30 => {
                                if let Some((transition, welcome)) = split_transition(payload)
                                    && let Some(dave) = &mut dave
                                {
                                    let actions = dave.welcome(transition, welcome);
                                    send_actions(&mut write, actions).await?;
                                }
                            }
                            26 | 28 | 31 => {}
                            other => debug!("voice binary op {other}"),
                        }
                    }
                    Message::Close(frame) => {
                        let code = frame.map(|frame| u16::from(frame.code));
                        let (reason, reconnect) = close_outcome(code);
                        return Ok(Outcome::Closed { reason, reconnect });
                    }
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
                }
            }
            _ = heartbeat.tick() => {
                if awaiting_ack {
                    return Ok(Outcome::Closed { reason: "voice heartbeat timeout".to_string(), reconnect: true });
                }
                send_json(&mut write, json!({
                    "op": 3,
                    "d": { "t": unix_ms(), "seq_ack": last_seq }
                })).await?;
                awaiting_ack = true;
            }
            _ = ping_timer.tick() => {
                ping_seq = ping_seq.wrapping_add(1);
                if pings.len() > 32 {
                    pings.clear();
                }
                pings.insert(ping_seq, Instant::now());
                udp.send(&ping_request(ping_seq)).await?;
            }
            received = udp.recv(&mut buffer) => {
                let received = received?;
                let packet = &buffer[..received];
                if let Some(sequence) = parse_ping(packet) {
                    if let Some(started) = pings.remove(&sequence) {
                        let _ = events.send(VoiceEvent::Ping(started.elapsed().as_millis() as u32));
                    }
                    continue;
                }
                let Some((header, offset)) = RtpHeader::parse(packet) else {
                    continue;
                };
                if header.payload_type != OPUS_PAYLOAD_TYPE {
                    continue;
                }
                let decrypted = match crypto.decrypt(packet, offset) {
                    Ok(decrypted) => decrypted,
                    Err(error) => {
                        debug!("voice transport decrypt failed: {error}");
                        continue;
                    }
                };
                let Some((decrypted_header, offset)) = RtpHeader::parse(&decrypted) else {
                    continue;
                };
                let payload = &decrypted[offset..];
                let Some(user_id) = ssrc_users.get(&decrypted_header.ssrc) else {
                    continue;
                };
                let Ok(user_id) = user_id.parse::<u64>() else {
                    continue;
                };
                let opus = match dave.as_mut() {
                    Some(dave) => dave
                        .decrypt_opus(user_id, payload)
                        .unwrap_or_else(|| payload.to_vec()),
                    None => payload.to_vec(),
                };
                if !decoders.contains_key(&user_id) {
                    match Decoder::new(Channels::Stereo, SampleRate::Hz48000) {
                        Ok(decoder) => {
                            decoders.insert(user_id, decoder);
                        }
                        Err(error) => {
                            debug!("voice decoder failed: {error:?}");
                            continue;
                        }
                    }
                }
                let Some(decoder) = decoders.get_mut(&user_id) else {
                    continue;
                };
                let mut pcm = vec![0f32; FRAME_VALUES * 6];
                let Ok(decoded) = decoder.decode_float_to_slice(&opus, &mut pcm, false) else {
                    continue;
                };
                let values = if decoded <= FRAME_SAMPLES {
                    decoded * 2
                } else {
                    decoded
                };
                pcm.truncate(values.min(FRAME_VALUES));
                if pcm.len() >= 2 {
                    let queue = jitter.entry(user_id).or_default();
                    queue.push_back(pcm);
                    while queue.len() > 8 {
                        queue.pop_front();
                    }
                }
                trace!(
                    "voice audio ssrc={} user={} bytes={}",
                    decrypted_header.ssrc,
                    user_id,
                    opus.len()
                );
            }
            _ = audio_timer.tick() => {
                if muted || deafened {
                    let _ = audio.take_frame();
                    if speaking_flag {
                        let _ = send_speaking(&mut write, ssrc, false).await?;
                        speaking_flag = false;
                    }
                } else if let Some(frame) = audio.take_frame() {
                    let energy = (frame.iter().map(|sample| sample * sample).sum::<f32>()
                        / frame.len() as f32)
                        .sqrt();
                    if energy > 0.012 {
                        if !speaking_flag {
                            send_speaking(&mut write, ssrc, true).await?;
                            speaking_flag = true;
                        }
                        silence_frames = 0;
                        if let Some(encoder) = &mut encoder {
                            send_frame(
                                encoder,
                                &mut dave,
                                &mut crypto,
                                &udp,
                                &frame,
                                ssrc,
                                sequence,
                                timestamp,
                            )
                            .await?;
                            sequence = sequence.wrapping_add(1);
                            timestamp = timestamp.wrapping_add(FRAME_SAMPLES as u32);
                        }
                    } else if speaking_flag {
                        silence_frames += 1;
                        let silence = vec![0f32; FRAME_VALUES];
                        if let Some(encoder) = &mut encoder {
                            send_frame(
                                encoder,
                                &mut dave,
                                &mut crypto,
                                &udp,
                                &silence,
                                ssrc,
                                sequence,
                                timestamp,
                            )
                            .await?;
                            sequence = sequence.wrapping_add(1);
                            timestamp = timestamp.wrapping_add(FRAME_SAMPLES as u32);
                        }
                        if silence_frames >= 5 {
                            send_speaking(&mut write, ssrc, false).await?;
                            speaking_flag = false;
                            silence_frames = 0;
                        }
                    }
                }

                if deafened {
                    jitter.clear();
                } else {
                    let mut mixed = vec![0f32; FRAME_VALUES];
                    let mut any = false;
                    for queue in jitter.values_mut() {
                        if let Some(frame) = queue.pop_front() {
                            for (target, source) in mixed.iter_mut().zip(frame.iter()) {
                                *target += *source;
                            }
                            any = true;
                        }
                    }
                    if any {
                        for sample in &mut mixed {
                            *sample = (*sample * 0.8).clamp(-1.0, 1.0);
                        }
                        audio.push_frame(&mixed);
                    }
                    jitter.retain(|_, queue| !queue.is_empty());
                }
            }
            command = commands.recv() => {
                match command {
                    Some(VoiceCommand::Leave) | None => {
                        let _ = send_speaking(&mut write, ssrc, false).await;
                        return Ok(Outcome::Left);
                    }
                    Some(VoiceCommand::SetMute(value)) => {
                        muted = value;
                        if muted && speaking_flag {
                            let _ = send_speaking(&mut write, ssrc, false).await;
                            speaking_flag = false;
                        }
                    }
                    Some(VoiceCommand::SetDeafen(value)) => {
                        deafened = value;
                        if deafened {
                            jitter.clear();
                            if speaking_flag {
                                let _ = send_speaking(&mut write, ssrc, false).await;
                                speaking_flag = false;
                            }
                        }
                    }
                    Some(VoiceCommand::SetDevices { input, output }) => {
                        audio.set_devices(input, output);
                    }
                }
            }
        }
    }
}

fn host(endpoint: &str) -> &str {
    endpoint
        .trim_start_matches("wss://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
}

async fn discover(udp: &UdpSocket, ssrc: u32) -> Result<discovery::DiscoveryResponse> {
    let request = discovery_request(ssrc);
    let mut buffer = [0u8; 128];
    for _ in 0..5 {
        udp.send(&request).await?;
        if let Ok(Ok(received)) = time::timeout(Duration::from_secs(2), udp.recv(&mut buffer)).await
            && let Some(response) = parse_discovery(&buffer[..received])
        {
            return Ok(response);
        }
    }
    Err(Error::Timeout)
}

async fn send_speaking(write: &mut Write, ssrc: u32, speaking: bool) -> Result<()> {
    send_json(
        write,
        json!({
            "op": 5,
            "d": { "speaking": if speaking { 1 } else { 0 }, "delay": 0, "ssrc": ssrc }
        }),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn send_frame(
    encoder: &mut Encoder,
    dave: &mut Option<Dave>,
    crypto: &mut TransportCrypto,
    udp: &UdpSocket,
    pcm: &[f32],
    ssrc: u32,
    sequence: u16,
    timestamp: u32,
) -> Result<()> {
    let mut packet = [0u8; 4000];
    let encoded = match encoder.encode_float_to_slice(pcm, &mut packet) {
        Ok(encoded) => encoded,
        Err(error) => {
            debug!("voice encode failed: {error:?}");
            return Ok(());
        }
    };
    let opus = &packet[..encoded];
    let payload = match dave {
        Some(dave) => dave.encrypt_opus(opus).unwrap_or_else(|| opus.to_vec()),
        None => opus.to_vec(),
    };
    let header = RtpHeader {
        payload_type: OPUS_PAYLOAD_TYPE,
        sequence,
        timestamp,
        ssrc,
    };
    let packet = crate::rtp::build(&header, &payload);
    let encrypted = crypto.encrypt(&packet[..12], &packet[12..])?;
    udp.send(&encrypted).await?;
    Ok(())
}

async fn send_actions(write: &mut Write, actions: Vec<DaveAction>) -> Result<()> {
    for action in actions {
        match action {
            DaveAction::KeyPackage(package) => {
                let mut frame = Vec::with_capacity(package.len() + 1);
                frame.push(26);
                frame.extend(package);
                write.send(Message::Binary(frame.into())).await?;
            }
            DaveAction::CommitWelcome { commit, welcome } => {
                let mut frame =
                    Vec::with_capacity(commit.len() + welcome.as_ref().map_or(0, Vec::len) + 1);
                frame.push(28);
                frame.extend(commit);
                if let Some(welcome) = welcome {
                    frame.extend(welcome);
                }
                write.send(Message::Binary(frame.into())).await?;
            }
            DaveAction::Ready(transition_id) => {
                send_json(
                    write,
                    json!({ "op": 23, "d": { "transition_id": transition_id } }),
                )
                .await?;
            }
            DaveAction::Invalid(transition_id) => {
                send_json(
                    write,
                    json!({ "op": 31, "d": { "transition_id": transition_id } }),
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn send_json(write: &mut Write, value: Value) -> Result<()> {
    write
        .send(Message::Text(value.to_string().into()))
        .await
        .map_err(Error::from)
}

fn split_transition(payload: &[u8]) -> Option<(u16, &[u8])> {
    if payload.len() < 2 {
        return None;
    }
    Some((u16::from_be_bytes([payload[0], payload[1]]), &payload[2..]))
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn close_outcome(code: Option<u16>) -> (String, bool) {
    match code {
        Some(4014) => ("голосовой канал покинут".to_string(), false),
        Some(4015) => ("голосовой сервер перезапускается".to_string(), true),
        Some(4016) => (
            "голосовой сервер получил неизвестный запрос".to_string(),
            false,
        ),
        Some(4017) => ("требуется поддержка DAVE".to_string(), false),
        Some(4021) => ("превышен лимит подключений к голосу".to_string(), false),
        Some(4022) => ("звонок завершён".to_string(), false),
        Some(4004) => ("не удалось авторизоваться в голосе".to_string(), false),
        Some(4006) => ("недействительная голосовая сессия".to_string(), false),
        Some(4009) => ("таймаут подключения к голосу".to_string(), false),
        Some(code) => (format!("голосовое соединение закрыто ({code})"), true),
        None => ("голосовое соединение закрыто".to_string(), true),
    }
}

enum Incoming {
    Json(Value),
    Binary(Vec<u8>),
    Closed(Option<u16>),
    Ignored,
}

async fn next_incoming(read: &mut Read, timeout: Duration) -> Result<Incoming> {
    let message = match time::timeout(timeout, read.next()).await {
        Ok(Some(Ok(message))) => message,
        Ok(Some(Err(error))) => return Err(Error::Ws(error)),
        Ok(None) => return Ok(Incoming::Closed(None)),
        Err(_) => return Err(Error::Timeout),
    };
    Ok(match message {
        Message::Text(text) => match serde_json::from_str(text.as_str()) {
            Ok(value) => Incoming::Json(value),
            Err(error) => {
                warn!("bad voice json: {error}");
                Incoming::Ignored
            }
        },
        Message::Binary(bytes) => Incoming::Binary(bytes.to_vec()),
        Message::Close(frame) => Incoming::Closed(frame.map(|frame| u16::from(frame.code))),
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Incoming::Ignored,
    })
}

async fn wait_for(
    read: &mut Read,
    timeout: Duration,
    want_op: u64,
    external_sender: &mut Option<Vec<u8>>,
) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::Timeout);
        }
        match next_incoming(read, remaining).await? {
            Incoming::Json(value) if value["op"].as_u64() == Some(want_op) => return Ok(value),
            Incoming::Json(_) | Incoming::Ignored => {}
            Incoming::Binary(bytes) => {
                if bytes.len() >= 3 && bytes[2] == 25 {
                    *external_sender = Some(bytes[3..].to_vec());
                }
            }
            Incoming::Closed(code) => {
                let (reason, _) = close_outcome(code);
                return Err(Error::Closed(reason));
            }
        }
    }
}

use std::{collections::HashSet, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    sync::mpsc,
    time::{self, Instant},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{
        Message,
        protocol::{CloseFrame, WebSocketConfig},
    },
};
use tracing::{debug, info, warn};

use crate::{
    events::{ConnectionState, Event},
    fingerprint::Fingerprint,
    models::{Channel, Guild, Message as DiscordMessage, Ready, Relationship, User},
};

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=9&encoding=json&compress=zlib-stream";
const CAPABILITIES: u32 = 253;
const MAX_FRAME: usize = 64 * 1024 * 1024;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct GatewayHandle {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn spawn(token: String, fp: Arc<Fingerprint>, events: mpsc::Sender<Event>) -> GatewayHandle {
    GatewayHandle {
        task: tokio::spawn(run(token, fp, events)),
    }
}

struct Session {
    id: String,
    url: String,
    seq: Option<u64>,
}

enum Action {
    Resume,
    Fresh,
    Fatal(String),
}

async fn run(token: String, fp: Arc<Fingerprint>, tx: mpsc::Sender<Event>) {
    let mut session: Option<Session> = None;
    let mut attempt = 0u32;
    let mut backoff = Duration::from_millis(500);

    loop {
        let (established, action) = connect_once(&token, &fp, &tx, &mut session).await;
        if established {
            attempt = 0;
            backoff = Duration::from_millis(500);
        }
        match action {
            Action::Fatal(reason) => {
                warn!("gateway fatal: {reason}");
                let _ = tx
                    .send(Event::Connection(ConnectionState::Disconnected { reason }))
                    .await;
                return;
            }
            Action::Fresh => session = None,
            Action::Resume => {}
        }

        attempt += 1;
        let _ = tx
            .send(Event::Connection(ConnectionState::Reconnecting { attempt }))
            .await;
        let jitter = Duration::from_millis(rand::random::<u64>() % 400);
        time::sleep(backoff + jitter).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn connect_once(
    token: &str,
    fp: &Fingerprint,
    tx: &mpsc::Sender<Event>,
    session: &mut Option<Session>,
) -> (bool, Action) {
    let url = match session {
        Some(s) => resume_url(&s.url),
        None => GATEWAY_URL.to_string(),
    };

    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));

    let ws = match connect_async_tls_with_config(&url, Some(config), false, None).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            warn!("gateway connect failed: {e}");
            return (false, Action::Resume);
        }
    };

    run_session(ws, token, fp, tx, session).await
}

fn resume_url(base: &str) -> String {
    if base.is_empty() {
        return GATEWAY_URL.to_string();
    }
    format!(
        "{}/?v=9&encoding=json&compress=zlib-stream",
        base.trim_end_matches('/')
    )
}

async fn run_session(
    ws: Ws,
    token: &str,
    fp: &Fingerprint,
    tx: &mpsc::Sender<Event>,
    session: &mut Option<Session>,
) -> (bool, Action) {
    let (mut write, mut read) = ws.split();
    let mut zlib = Zlib::new();
    let mut last_seq = session.as_ref().and_then(|s| s.seq);
    let mut established = false;
    let mut subscribed: HashSet<String> = HashSet::new();

    let hello =
        match time::timeout(Duration::from_secs(15), next_payload(&mut read, &mut zlib)).await {
            Ok(Incoming::Payload(v)) if v["op"].as_u64() == Some(10) => v,
            _ => {
                return (
                    established,
                    if session.is_some() {
                        Action::Resume
                    } else {
                        Action::Fresh
                    },
                );
            }
        };

    let interval_ms = hello["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250);
    let interval = Duration::from_millis(interval_ms.max(1_000));

    if let Some(s) = session.as_ref() {
        let payload = json!({
            "op": 6,
            "d": { "token": token, "session_id": s.id, "seq": s.seq }
        });
        if !send_json(&mut write, payload).await {
            return (established, Action::Resume);
        }
    } else if !send_json(&mut write, identify(token, fp)).await {
        return (established, Action::Resume);
    }

    let first_beat = interval.mul_f64(rand::random::<f64>().max(0.1));
    let mut heartbeat = time::interval_at(Instant::now() + first_beat, interval);
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    let mut awaiting_ack = false;

    loop {
        tokio::select! {
            incoming = next_payload(&mut read, &mut zlib) => {
                let value = match incoming {
                    Incoming::Payload(v) => v,
                    Incoming::Closed(code) => {
                        return (established, close_action(code));
                    }
                };
                let op = value["op"].as_u64().unwrap_or(u64::MAX);
                match op {
                    0 => {
                        if let Some(seq) = value["s"].as_u64() {
                            last_seq = Some(seq);
                            if let Some(s) = session.as_mut() { s.seq = Some(seq); }
                        }
                        let name = value["t"].as_str().unwrap_or_default();
                        match name {
                            "READY" => match serde_json::from_value::<Ready>(value["d"].clone()) {
                                Ok(ready) => {
                                    let ids: Vec<String> = ready.guilds.iter().map(|g| g.id.clone()).collect();
                                    *session = Some(Session {
                                        id: ready.session_id.clone(),
                                        url: ready.resume_gateway_url.clone(),
                                        seq: last_seq,
                                    });
                                    established = true;
                                    let _ = tx.send(Event::Ready(Box::new(ready))).await;
                                    let _ = tx.send(Event::Connection(ConnectionState::Connected)).await;
                                    for id in &ids { subscribed.insert(id.clone()); }
                                    if !ids.is_empty() && !send_json(&mut write, subscribe_payload(&ids)).await {
                                        return (established, Action::Resume);
                                    }
                                    info!("gateway ready, {} guilds", ids.len());
                                }
                                Err(e) => warn!("READY parse failed: {e}"),
                            },
                            "RESUMED" => {
                                established = true;
                                let _ = tx.send(Event::Connection(ConnectionState::Resumed)).await;
                                info!("gateway resumed");
                            }
                            "GUILD_CREATE" => {
                                if let Some(guild) = parse::<Guild>("GUILD_CREATE", &value["d"]) {
                                    if !guild.unavailable
                                        && subscribed.insert(guild.id.clone())
                                        && !send_json(
                                            &mut write,
                                            subscribe_payload(std::slice::from_ref(&guild.id)),
                                        )
                                        .await
                                    {
                                        return (established, Action::Resume);
                                    }
                                    let _ = tx.send(Event::GuildCreate(Box::new(guild))).await;
                                }
                            }
                            _ => dispatch(name, &value["d"], tx).await,
                        }
                    }
                    1 => {
                        if !send_json(&mut write, json!({"op": 1, "d": last_seq})).await {
                            return (established, Action::Resume);
                        }
                    }
                    7 => return (established, Action::Resume),
                    9 => {
                        let resumable = value["d"].as_bool().unwrap_or(false);
                        return (established, if resumable { Action::Resume } else { Action::Fresh });
                    }
                    11 => awaiting_ack = false,
                    10 => {}
                    other => debug!("unhandled gateway op {other}"),
                }
            }
            _ = heartbeat.tick() => {
                if awaiting_ack {
                    warn!("heartbeat not acknowledged, reconnecting");
                    return (established, Action::Resume);
                }
                if !send_json(&mut write, json!({"op": 1, "d": last_seq})).await {
                    return (established, Action::Resume);
                }
                awaiting_ack = true;
            }
        }
    }
}

fn close_action(code: Option<u16>) -> Action {
    match code {
        Some(4004) | Some(4010..=4014) => Action::Fatal(format!(
            "connection rejected ({})",
            code.unwrap_or_default()
        )),
        Some(4003) | Some(4007) | Some(4009) | Some(1000) | Some(1001) => Action::Fresh,
        Some(4002) => Action::Fatal("invalid payload, gateway closing".to_string()),
        _ => Action::Resume,
    }
}

fn identify(token: &str, fp: &Fingerprint) -> Value {
    json!({
        "op": 2,
        "d": {
            "token": token,
            "capabilities": CAPABILITIES,
            "properties": fp.identify_properties(),
            "presence": { "status": "online", "since": 0, "activities": [], "afk": false },
            "compress": false,
            "client_state": {
                "guild_versions": {},
                "highest_last_message_id": "0",
                "read_state_version": 0,
                "user_guild_settings_version": -1,
                "user_settings_version": -1,
                "private_channels_version": "0",
                "api_code_version": 0
            }
        }
    })
}

fn subscribe_payload(ids: &[String]) -> Value {
    let mut subscriptions = serde_json::Map::new();
    for id in ids {
        subscriptions.insert(
            id.clone(),
            json!({
                "typing": true,
                "activities": true,
                "threads": true,
                "member_updates": false,
                "channels": {},
                "thread_member_lists": []
            }),
        );
    }
    json!({ "op": 37, "d": { "subscriptions": subscriptions } })
}

async fn dispatch(name: &str, data: &Value, tx: &mpsc::Sender<Event>) {
    let event = match name {
        "MESSAGE_CREATE" => {
            parse::<DiscordMessage>(name, data).map(|m| Event::MessageCreate(Box::new(m)))
        }
        "MESSAGE_UPDATE" => {
            parse::<DiscordMessage>(name, data).map(|m| Event::MessageUpdate(Box::new(m)))
        }
        "MESSAGE_DELETE" => match (data["id"].as_str(), data["channel_id"].as_str()) {
            (Some(id), Some(channel_id)) => Some(Event::MessageDelete {
                id: id.to_string(),
                channel_id: channel_id.to_string(),
            }),
            _ => None,
        },
        "GUILD_UPDATE" => parse::<Guild>(name, data).map(|g| Event::GuildUpdate(Box::new(g))),
        "GUILD_DELETE" => data["id"].as_str().map(|id| Event::GuildRemove {
            id: id.to_string(),
            unavailable: data["unavailable"].as_bool().unwrap_or(false),
        }),
        "CHANNEL_CREATE" => parse::<Channel>(name, data).map(|c| Event::ChannelCreate(Box::new(c))),
        "CHANNEL_UPDATE" => parse::<Channel>(name, data).map(|c| Event::ChannelUpdate(Box::new(c))),
        "CHANNEL_DELETE" => parse::<Channel>(name, data).map(|c| Event::ChannelDelete(Box::new(c))),
        "TYPING_START" => match (data["channel_id"].as_str(), data["user_id"].as_str()) {
            (Some(channel_id), Some(user_id)) => Some(Event::TypingStart {
                channel_id: channel_id.to_string(),
                user_id: user_id.to_string(),
            }),
            _ => None,
        },
        "RELATIONSHIP_ADD" => {
            parse::<Relationship>(name, data).map(|r| Event::RelationshipAdd(Box::new(r)))
        }
        "RELATIONSHIP_REMOVE" => data["id"]
            .as_str()
            .map(|id| Event::RelationshipRemove { id: id.to_string() }),
        "USER_UPDATE" => parse::<User>(name, data).map(|u| Event::UserUpdate(Box::new(u))),
        _ => None,
    };

    if let Some(event) = event {
        let _ = tx.send(event).await;
    }
}

fn parse<T: DeserializeOwned>(name: &str, value: &Value) -> Option<T> {
    match serde_json::from_value(value.clone()) {
        Ok(parsed) => Some(parsed),
        Err(e) => {
            warn!("{name} parse failed: {e}");
            None
        }
    }
}

async fn send_json<S>(sink: &mut S, value: Value) -> bool
where
    S: SinkExt<Message> + Unpin,
{
    sink.send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}

enum Incoming {
    Payload(Value),
    Closed(Option<u16>),
}

async fn next_payload<R>(read: &mut R, zlib: &mut Zlib) -> Incoming
where
    R: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let message = match read.next().await {
            Some(Ok(message)) => message,
            Some(Err(e)) => {
                debug!("gateway read error: {e}");
                return Incoming::Closed(None);
            }
            None => return Incoming::Closed(None),
        };
        match message {
            Message::Text(text) => match serde_json::from_str(text.as_str()) {
                Ok(value) => return Incoming::Payload(value),
                Err(e) => warn!("bad gateway json: {e}"),
            },
            Message::Binary(data) => {
                if let Some(output) = zlib.push(&data) {
                    return match serde_json::from_slice(&output) {
                        Ok(value) => Incoming::Payload(value),
                        Err(e) => {
                            warn!("bad gateway json after inflate: {e}");
                            Incoming::Closed(None)
                        }
                    };
                }
            }
            Message::Close(frame) => {
                return Incoming::Closed(close_code(frame));
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}

fn close_code(frame: Option<CloseFrame>) -> Option<u16> {
    frame.map(|f| f.code.into())
}

struct Zlib {
    decompressor: flate2::Decompress,
    buffer: Vec<u8>,
}

impl Zlib {
    fn new() -> Self {
        Self {
            decompressor: flate2::Decompress::new(true),
            buffer: Vec::new(),
        }
    }

    fn push(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        self.buffer.extend_from_slice(data);
        if self.buffer.len() < 4 || !self.buffer.ends_with(&[0, 0, 0xff, 0xff]) {
            return None;
        }

        let mut output = Vec::with_capacity(64 * 1024);
        let mut consumed = 0usize;
        loop {
            let input_before = self.decompressor.total_in();
            let output_before = self.decompressor.total_out();
            match self.decompressor.decompress_vec(
                &self.buffer[consumed..],
                &mut output,
                flate2::FlushDecompress::Sync,
            ) {
                Ok(_) => {
                    consumed += (self.decompressor.total_in() - input_before) as usize;
                    let produced = self.decompressor.total_out() - output_before;
                    if consumed >= self.buffer.len() {
                        break;
                    }
                    if produced == 0 {
                        warn!("zlib made no progress");
                        return None;
                    }
                }
                Err(e) => {
                    warn!("zlib inflate failed: {e}");
                    return None;
                }
            }
        }
        self.buffer.clear();
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    #[test]
    fn decodes_fragmented_zlib_stream() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(br#"{"op":0,"t":"A"}"#).unwrap();
        encoder.flush().unwrap();
        let first = encoder.get_mut().clone();

        encoder.write_all(br#"{"op":0,"t":"B"}"#).unwrap();
        encoder.flush().unwrap();
        let all = encoder.get_mut().clone();
        let second = &all[first.len()..];

        let mut zlib = Zlib::new();
        assert!(zlib.push(&first[..first.len() / 2]).is_none());
        let output = zlib.push(&first[first.len() / 2..]).expect("first flush");
        assert_eq!(output, br#"{"op":0,"t":"A"}"#);
        let output = zlib.push(second).expect("second flush");
        assert_eq!(output, br#"{"op":0,"t":"B"}"#);
    }

    #[test]
    fn close_codes_map_to_actions() {
        assert!(matches!(close_action(Some(4004)), Action::Fatal(_)));
        assert!(matches!(close_action(Some(4014)), Action::Fatal(_)));
        assert!(matches!(close_action(Some(4003)), Action::Fresh));
        assert!(matches!(close_action(Some(4000)), Action::Resume));
        assert!(matches!(close_action(None), Action::Resume));
    }
}

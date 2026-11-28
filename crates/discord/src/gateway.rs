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
    models::{
        Channel, Emoji, Guild, Member, Message as DiscordMessage, Presence, Ready,
        ReadySupplemental, Relationship, User, VoiceState,
    },
};

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=9&encoding=json&compress=zlib-stream";
const CAPABILITIES: u32 = 253;
const MAX_FRAME: usize = 64 * 1024 * 1024;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct GatewayHandle {
    commands: mpsc::UnboundedSender<Value>,
    task: tokio::task::JoinHandle<()>,
}

impl GatewayHandle {
    pub fn set_status(&self, status: &str) {
        let payload = json!({
            "op": 3,
            "d": {
                "status": status,
                "since": 0,
                "activities": [],
                "afk": false
            }
        });
        let _ = self.commands.send(payload);
    }

    pub fn voice_state(
        &self,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
        self_mute: bool,
        self_deaf: bool,
    ) {
        let payload = json!({
            "op": 4,
            "d": {
                "guild_id": guild_id,
                "channel_id": channel_id,
                "self_mute": self_mute,
                "self_deaf": self_deaf,
                "self_video": false
            }
        });
        let _ = self.commands.send(payload);
    }

    pub fn subscribe_members(&self, guild_id: &str, channel_id: &str) {
        let mut channels = serde_json::Map::new();
        channels.insert(channel_id.to_string(), json!([[0, 99]]));
        let payload = json!({
            "op": 14,
            "d": {
                "guild_id": guild_id,
                "typing": true,
                "activities": true,
                "threads": true,
                "channels": channels,
                "thread_member_lists": []
            }
        });
        let _ = self.commands.send(payload);
    }
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn spawn(token: String, fp: Arc<Fingerprint>, events: mpsc::Sender<Event>) -> GatewayHandle {
    let (commands, command_rx) = mpsc::unbounded_channel();
    GatewayHandle {
        commands,
        task: tokio::spawn(run(token, fp, events, command_rx)),
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

async fn run(
    token: String,
    fp: Arc<Fingerprint>,
    tx: mpsc::Sender<Event>,
    mut commands: mpsc::UnboundedReceiver<Value>,
) {
    let mut session: Option<Session> = None;
    let mut attempt = 0u32;
    let mut backoff = Duration::from_millis(500);

    loop {
        let (established, action) =
            connect_once(&token, &fp, &tx, &mut session, &mut commands).await;
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
    commands: &mut mpsc::UnboundedReceiver<Value>,
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

    run_session(ws, token, fp, tx, session, commands).await
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
    commands: &mut mpsc::UnboundedReceiver<Value>,
) -> (bool, Action) {
    let (mut write, mut read) = ws.split();
    let mut zlib = Zlib::new();
    let mut last_seq = session.as_ref().and_then(|s| s.seq);
    let mut established = false;
    let mut subscribed: HashSet<String> = HashSet::new();
    let mut logged_events: HashSet<String> = HashSet::new();

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
                            "READY" => {
                                if let Some(ready) = parse::<Ready>("READY", &value["d"]) {
                                    let ids: Vec<String> = ready.guilds.iter().map(|g| g.id.clone()).collect();
                                    let member_lens: Vec<usize> =
                                        ready.merged_members.iter().map(Vec::len).collect();
                                    let guild_member_lens: Vec<usize> =
                                        ready.guilds.iter().map(|g| g.members.len()).collect();
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
                                    info!(
                                        "gateway ready, {} guilds, merged_members={:?}, guild_members={:?}",
                                        ids.len(),
                                        member_lens,
                                        guild_member_lens
                                    );
                                }
                            }
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
                            "READY_SUPPLEMENTAL" => {
                                if let Some(supplemental) =
                                    parse::<ReadySupplemental>("READY_SUPPLEMENTAL", &value["d"])
                                {
                                    debug!(
                                        "READY_SUPPLEMENTAL: guilds={:?} member_groups={:?} lazy_channels={} users={}",
                                        supplemental.guilds.iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
                                        supplemental.merged_members.iter().map(Vec::len).collect::<Vec<_>>(),
                                        supplemental.lazy_private_channels.len(),
                                        supplemental.users.len()
                                    );
                                    if !supplemental.users.is_empty() {
                                        let _ = tx.send(Event::Users(supplemental.users)).await;
                                    }
                                    if !supplemental.lazy_private_channels.is_empty() {
                                        let _ = tx
                                            .send(Event::PrivateChannels(
                                                supplemental.lazy_private_channels,
                                            ))
                                            .await;
                                    }
                                    for (guild, members) in supplemental
                                        .guilds
                                        .iter()
                                        .zip(supplemental.merged_members.iter())
                                    {
                                        if !members.is_empty() {
                                            let _ = tx
                                                .send(Event::MemberChunk {
                                                    guild_id: guild.id.clone(),
                                                    members: members.clone(),
                                                })
                                                .await;
                                        }
                                    }
                                }
                            }
                            other => {
                                if logged_events.insert(other.to_string()) {
                                    info!("gateway dispatch {other}");
                                }
                                dispatch(other, &value["d"], tx).await;
                            }
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
            Some(payload) = commands.recv() => {
                if !send_json(&mut write, payload).await {
                    return (established, Action::Resume);
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
        "MESSAGE_REACTION_ADD" => reaction_event(data, true),
        "MESSAGE_REACTION_REMOVE" => reaction_event(data, false),
        "MESSAGE_REACTION_REMOVE_ALL" => {
            match (data["message_id"].as_str(), data["channel_id"].as_str()) {
                (Some(message_id), Some(channel_id)) => Some(Event::ReactionRemoveAll {
                    message_id: message_id.to_string(),
                    channel_id: channel_id.to_string(),
                }),
                _ => None,
            }
        }
        "MESSAGE_REACTION_REMOVE_EMOJI" => match (
            data["message_id"].as_str(),
            data["channel_id"].as_str(),
            serde_json::from_value::<Emoji>(data["emoji"].clone()).ok(),
        ) {
            (Some(message_id), Some(channel_id), Some(emoji)) => Some(Event::ReactionRemoveEmoji {
                message_id: message_id.to_string(),
                channel_id: channel_id.to_string(),
                emoji,
            }),
            _ => None,
        },
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
        "PRESENCE_UPDATE" => {
            parse::<Presence>(name, data).map(|p| Event::PresenceUpdate(Box::new(p)))
        }
        "GUILD_MEMBER_ADD" | "GUILD_MEMBER_UPDATE" => member_event(data),
        "VOICE_STATE_UPDATE" => {
            serde_json::from_value::<VoiceState>(data.clone())
                .ok()
                .map(|state| Event::VoiceStateUpdate {
                    guild_id: data["guild_id"].as_str().map(str::to_string),
                    state: Box::new(state),
                })
        }
        "VOICE_SERVER_UPDATE" => match (data["token"].as_str(), data["endpoint"].as_str()) {
            (Some(token), Some(endpoint)) => Some(Event::VoiceServerUpdate {
                guild_id: data["guild_id"].as_str().map(str::to_string),
                token: token.to_string(),
                endpoint: endpoint.to_string(),
            }),
            _ => None,
        },
        "CALL_CREATE" => serde_json::from_value::<crate::models::Call>(data.clone())
            .ok()
            .map(|call| Event::CallCreate(Box::new(call))),
        "CALL_UPDATE" => serde_json::from_value::<crate::models::Call>(data.clone())
            .ok()
            .map(|call| Event::CallUpdate(Box::new(call))),
        "CALL_DELETE" => data["channel_id"]
            .as_str()
            .map(|channel_id| Event::CallDelete {
                channel_id: channel_id.to_string(),
            }),
        "GUILD_MEMBER_LIST_UPDATE" => member_list_event(data),
        "GUILD_MEMBERS_CHUNK" => match data["guild_id"].as_str() {
            Some(guild_id) => serde_json::from_value::<Vec<Member>>(data["members"].clone())
                .ok()
                .map(|members| Event::MemberChunk {
                    guild_id: guild_id.to_string(),
                    members,
                }),
            None => None,
        },
        "GUILD_MEMBER_REMOVE" => match (data["guild_id"].as_str(), data["user"]["id"].as_str()) {
            (Some(guild_id), Some(user_id)) => Some(Event::MemberRemove {
                guild_id: guild_id.to_string(),
                user_id: user_id.to_string(),
            }),
            _ => None,
        },
        _ => None,
    };

    if let Some(event) = event {
        let _ = tx.send(event).await;
    }
}

fn member_list_event(data: &Value) -> Option<Event> {
    let guild_id = data["guild_id"].as_str()?.to_string();
    let mut members = Vec::new();
    for op in data["ops"].as_array()? {
        if !matches!(
            op["op"].as_str(),
            Some("SYNC") | Some("INSERT") | Some("UPDATE")
        ) {
            continue;
        }
        for item in op["items"].as_array().into_iter().flatten() {
            if let Ok(member) = serde_json::from_value::<Member>(item["member"].clone())
                && !member.id().is_empty()
            {
                members.push(member);
            }
        }
    }
    if members.is_empty() {
        None
    } else {
        Some(Event::MemberChunk { guild_id, members })
    }
}

fn member_event(data: &Value) -> Option<Event> {
    let guild_id = data["guild_id"].as_str()?.to_string();
    let member = serde_json::from_value::<Member>(data.clone()).ok()?;
    Some(Event::MemberUpdate { guild_id, member })
}

fn reaction_event(data: &Value, add: bool) -> Option<Event> {
    let message_id = data["message_id"].as_str()?.to_string();
    let channel_id = data["channel_id"].as_str()?.to_string();
    let user_id = data["user_id"].as_str()?.to_string();
    let emoji = serde_json::from_value::<Emoji>(data["emoji"].clone()).ok()?;
    Some(if add {
        Event::ReactionAdd {
            message_id,
            channel_id,
            user_id,
            emoji,
        }
    } else {
        Event::ReactionRemove {
            message_id,
            channel_id,
            user_id,
            emoji,
        }
    })
}

fn parse<T: DeserializeOwned>(name: &str, value: &Value) -> Option<T> {
    let text = value.to_string();
    let mut deserializer = serde_json::Deserializer::from_str(&text);
    match serde_path_to_error::deserialize(&mut deserializer) {
        Ok(parsed) => Some(parsed),
        Err(e) => {
            warn!("{name} parse failed at {}: {}", e.path(), e.inner());
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

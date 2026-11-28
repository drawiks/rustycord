use std::time::Duration;

use rustycord_discord::{Discord, Event};
use rustycord_voice::{VoiceEvent, VoiceSession, spawn};
use tokio::sync::mpsc;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires DISCORD_TOKEN and a real voice channel"]
async fn voice_handshake_smoke() {
    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN");
    let channel_id = std::env::var("VOICE_CHANNEL_ID").expect("VOICE_CHANNEL_ID");
    let guild_id = std::env::var("VOICE_GUILD_ID")
        .ok()
        .filter(|id| !id.is_empty());
    let require_dave = std::env::var("VOICE_REQUIRE_DAVE")
        .map(|value| value != "0")
        .unwrap_or(true);

    let (client, me) = Discord::login(token).await.expect("discord login");
    let (gateway_events, mut gateway_rx) = mpsc::channel(256);
    let gateway = client.spawn_gateway(gateway_events);
    gateway.voice_state(guild_id.as_deref(), Some(&channel_id), true, true);

    let mut session_id = None;
    let mut server = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while session_id.is_none() || server.is_none() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for voice server events"
        );
        let event = tokio::time::timeout(remaining, gateway_rx.recv())
            .await
            .expect("gateway timeout")
            .expect("gateway closed");
        match event {
            Event::VoiceStateUpdate { state, .. }
                if state.user_id == me.id
                    && state.channel_id.as_deref() == Some(channel_id.as_str()) =>
            {
                session_id = Some(state.session_id.clone());
            }
            Event::VoiceServerUpdate {
                guild_id: event_guild,
                token,
                endpoint,
            } if event_guild.as_deref() == guild_id.as_deref() => {
                server = Some((token, endpoint));
            }
            Event::Connection(state) => {
                println!("gateway state: {state:?}");
            }
            Event::CallCreate(_) | Event::CallUpdate(_) | Event::CallDelete { .. } => {}
            _ => {}
        }
    }

    let (token, endpoint) = server.expect("voice server");
    let (voice_events, mut voice_rx) = mpsc::unbounded_channel();
    let voice = spawn(
        VoiceSession {
            server_id: guild_id.clone().unwrap_or_else(|| channel_id.clone()),
            channel_id: channel_id.clone(),
            user_id: me.id.clone(),
            session_id: session_id.expect("session id"),
            token,
            endpoint,
            self_mute: true,
            self_deaf: true,
            input_device: None,
            output_device: None,
        },
        voice_events,
    );

    let mut connected = false;
    let mut dave_ready = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
    while !connected || (require_dave && !dave_ready) {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for voice handshake (connected={connected}, dave={dave_ready})"
        );
        let event = tokio::time::timeout(remaining, voice_rx.recv())
            .await
            .expect("voice timeout")
            .expect("voice closed");
        match event {
            VoiceEvent::Ready { mode, dave } => {
                println!("voice ready: mode={mode} dave={dave}");
                connected = true;
            }
            VoiceEvent::DaveReady => {
                println!("dave ready");
                dave_ready = true;
            }
            VoiceEvent::Error(error) => panic!("voice error: {error}"),
            VoiceEvent::Disconnected { reason, .. } => panic!("voice disconnected: {reason}"),
            _ => {}
        }
    }

    voice.leave();
    gateway.voice_state(guild_id.as_deref(), None, true, true);
    tokio::time::sleep(Duration::from_secs(1)).await;
}

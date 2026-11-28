use std::path::PathBuf;

use rustycord_discord::{Discord, Event, Message, User};
use rustycord_voice::{VoiceEvent, VoiceHandle, VoiceSession, spawn as spawn_voice};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub enum Command {
    LoadHistory {
        channel_id: String,
        before: Option<String>,
    },
    Edit {
        channel_id: String,
        message_id: String,
        content: String,
    },
    Delete {
        channel_id: String,
        message_id: String,
    },
    Send {
        channel_id: String,
        content: String,
        reply_to: Option<String>,
    },
    Ack {
        channel_id: String,
        message_id: String,
        token: Option<String>,
    },
    Typing {
        channel_id: String,
    },
    SubscribeMembers {
        guild_id: String,
        channel_id: String,
    },
    SetStatus {
        status: String,
    },
    UploadFile {
        channel_id: String,
        path: PathBuf,
        name: String,
        content: String,
    },
    DownloadFile {
        url: String,
        dest: PathBuf,
        name: String,
    },
    React {
        channel_id: String,
        message_id: String,
        emoji: String,
    },
    Unreact {
        channel_id: String,
        message_id: String,
        emoji: String,
    },
    VoiceJoin {
        guild_id: Option<String>,
        channel_id: String,
        ring: bool,
        self_mute: bool,
        self_deaf: bool,
        input_device: Option<String>,
        output_device: Option<String>,
    },
    SetDevices {
        input: Option<String>,
        output: Option<String>,
    },
    VoiceLeave,
    SetMute {
        mute: bool,
        deafen: bool,
    },
    CallDecline {
        channel_id: String,
    },
}

pub enum UiEvent {
    LoggedIn,
    LoginFailed(String),
    Discord(Box<Event>),
    History {
        channel_id: String,
        messages: Vec<Message>,
    },
    AckToken {
        channel_id: String,
        token: String,
    },
    TransferDone {
        note: Option<String>,
        error: Option<String>,
    },
    Voice(VoiceEvent),
    Error(String),
}

struct PendingVoice {
    guild_id: Option<String>,
    channel_id: String,
    session_id: Option<String>,
    server: Option<(String, String)>,
    ring: bool,
    self_mute: bool,
    self_deaf: bool,
    input_device: Option<String>,
    output_device: Option<String>,
}

struct ActiveVoice {
    guild_id: Option<String>,
    channel_id: String,
    self_mute: bool,
    self_deaf: bool,
}

pub struct Net {
    commands: UnboundedSender<Command>,
}

impl Net {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

pub fn spawn(token: String, ui: UnboundedSender<UiEvent>) -> Net {
    let (commands, command_rx) = unbounded_channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        runtime.block_on(run(token, command_rx, ui));
    });
    Net { commands }
}

async fn run(
    token: String,
    mut commands: UnboundedReceiver<Command>,
    ui: UnboundedSender<UiEvent>,
) {
    let (client, me) = match Discord::login(token).await {
        Ok(value) => value,
        Err(error) => {
            let _ = ui.send(UiEvent::LoginFailed(error.to_string()));
            return;
        }
    };
    let _ = ui.send(UiEvent::LoggedIn);

    {
        let client = client.clone();
        let ui = ui.clone();
        tokio::spawn(async move {
            if let Ok(channels) = client.private_channels().await {
                let users: Vec<User> = channels
                    .iter()
                    .flat_map(|channel| channel.recipients.iter().cloned())
                    .collect();
                let _ = ui.send(UiEvent::Discord(Box::new(Event::Users(users))));
                let _ = ui.send(UiEvent::Discord(Box::new(Event::PrivateChannels(channels))));
            }
            if let Ok(relationships) = client.relationships().await {
                let _ = ui.send(UiEvent::Discord(Box::new(Event::Relationships(
                    relationships,
                ))));
            }
        });
    }

    let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::channel(1024);
    let gateway = client.spawn_gateway(gateway_tx);
    let mut pending: Option<PendingVoice> = None;
    let mut active: Option<ActiveVoice> = None;
    let mut voice: Option<VoiceHandle> = None;

    loop {
        tokio::select! {
            Some(event) = gateway_rx.recv() => {
                match &event {
                    Event::VoiceStateUpdate { state, .. } if state.user_id == me.id => {
                        if state.channel_id.is_none() {
                            if let Some(handle) = voice.take() {
                                handle.leave();
                            }
                            pending = None;
                            active = None;
                        } else if let Some(pending) = &mut pending
                            && state.channel_id.as_deref() == Some(pending.channel_id.as_str())
                        {
                            pending.session_id = Some(state.session_id.clone());
                        }
                    }
                    Event::VoiceServerUpdate { guild_id, token, endpoint } => {
                        if let Some(pending) = &mut pending
                            && pending.guild_id.as_deref() == guild_id.as_deref()
                        {
                            pending.server = Some((token.clone(), endpoint.clone()));
                        }
                    }
                    _ => {}
                }
                let ready = pending
                    .as_ref()
                    .is_some_and(|pending| pending.session_id.is_some() && pending.server.is_some());
                if ready
                    && let Some((handle, started)) = start_voice(pending.take(), &client, &me.id, &ui)
                {
                    if let Some(previous) = voice.replace(handle) {
                        previous.leave();
                    }
                    active = Some(ActiveVoice {
                        guild_id: started.guild_id,
                        channel_id: started.channel_id,
                        self_mute: started.self_mute,
                        self_deaf: started.self_deaf,
                    });
                }
                let _ = ui.send(UiEvent::Discord(Box::new(event)));
            }
            Some(command) = commands.recv() => {
                match command {
                    Command::SubscribeMembers { guild_id, channel_id } => {
                        gateway.subscribe_members(&guild_id, &channel_id);
                    }
                    Command::SetStatus { status } => gateway.set_status(&status),
                    Command::VoiceJoin { guild_id, channel_id, ring, self_mute, self_deaf, input_device, output_device } => {
                        if let Some(handle) = voice.take() {
                            handle.leave();
                        }
                        pending = Some(PendingVoice {
                            guild_id,
                            channel_id: channel_id.clone(),
                            session_id: None,
                            server: None,
                            ring,
                            self_mute,
                            self_deaf,
                            input_device,
                            output_device,
                        });
                        active = None;
                        gateway.voice_state(
                            pending.as_ref().and_then(|p| p.guild_id.as_deref()),
                            Some(&channel_id),
                            self_mute,
                            self_deaf,
                        );
                    }
                    Command::VoiceLeave => {
                        if let Some(handle) = voice.take() {
                            handle.leave();
                        }
                        if let Some(current) = active.take() {
                            gateway.voice_state(current.guild_id.as_deref(), None, current.self_mute, current.self_deaf);
                        }
                        if let Some(current) = pending.take() {
                            gateway.voice_state(current.guild_id.as_deref(), None, current.self_mute, current.self_deaf);
                        }
                    }
                    Command::SetMute { mute, deafen } => {
                        if let Some(current) = active.as_mut() {
                            current.self_mute = mute;
                            current.self_deaf = deafen;
                            gateway.voice_state(current.guild_id.as_deref(), Some(&current.channel_id), mute, deafen);
                        } else if let Some(current) = pending.as_mut() {
                            current.self_mute = mute;
                            current.self_deaf = deafen;
                            gateway.voice_state(current.guild_id.as_deref(), Some(&current.channel_id), mute, deafen);
                        }
                        if let Some(handle) = &voice {
                            handle.set_mute(mute);
                            handle.set_deafen(deafen);
                        }
                    }
                    Command::SetDevices { input, output } => {
                        if let Some(handle) = &voice {
                            handle.set_devices(input, output);
                        }
                    }
                    Command::CallDecline { channel_id } => {
                        let client = client.clone();
                        tokio::spawn(async move {
                            let _ = client.stop_ringing(&channel_id, None).await;
                        });
                    }
                    command => {
                        let client = client.clone();
                        let ui = ui.clone();
                        tokio::spawn(async move {
                            if let Err(error) = handle(client, command, &ui).await {
                                let _ = ui.send(UiEvent::Error(error.to_string()));
                            }
                        });
                    }
                }
            }
            else => break,
        }
    }
}

fn start_voice(
    pending: Option<PendingVoice>,
    client: &Discord,
    user_id: &str,
    ui: &UnboundedSender<UiEvent>,
) -> Option<(VoiceHandle, PendingVoice)> {
    let pending = pending?;
    let session_id = pending.session_id.clone()?;
    let (token, endpoint) = pending.server.clone()?;
    let session = VoiceSession {
        server_id: pending
            .guild_id
            .clone()
            .unwrap_or_else(|| pending.channel_id.clone()),
        channel_id: pending.channel_id.clone(),
        user_id: user_id.to_string(),
        session_id,
        token,
        endpoint,
        self_mute: pending.self_mute,
        self_deaf: pending.self_deaf,
        input_device: pending.input_device.clone(),
        output_device: pending.output_device.clone(),
    };
    let ring = pending.ring;
    let channel_id = pending.channel_id.clone();
    let (events, mut event_rx) = unbounded_channel();
    let handle = spawn_voice(session, events);
    let client = client.clone();
    let ui = ui.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if ring && matches!(&event, VoiceEvent::Ready { .. }) {
                let _ = client.ring_call(&channel_id, None).await;
            }
            let _ = ui.send(UiEvent::Voice(event));
        }
    });
    Some((handle, pending))
}

async fn handle(
    client: Discord,
    command: Command,
    ui: &UnboundedSender<UiEvent>,
) -> rustycord_discord::Result<()> {
    match command {
        Command::LoadHistory { channel_id, before } => {
            let messages = client.history(&channel_id, before.as_deref(), 50).await?;
            let _ = ui.send(UiEvent::History {
                channel_id,
                messages,
            });
        }
        Command::Send {
            channel_id,
            content,
            reply_to,
        } => {
            client
                .send(&channel_id, &content, reply_to.as_deref())
                .await?;
        }
        Command::Edit {
            channel_id,
            message_id,
            content,
        } => {
            client.edit(&channel_id, &message_id, &content).await?;
        }
        Command::Delete {
            channel_id,
            message_id,
        } => {
            client.delete(&channel_id, &message_id).await?;
        }
        Command::Ack {
            channel_id,
            message_id,
            token,
        } => {
            let value = client
                .ack(&channel_id, &message_id, token.as_deref())
                .await?;
            if let Some(token) = value["token"].as_str() {
                let _ = ui.send(UiEvent::AckToken {
                    channel_id,
                    token: token.to_string(),
                });
            }
        }
        Command::Typing { channel_id } => {
            client.typing(&channel_id).await?;
        }
        Command::SubscribeMembers { .. } => {}
        Command::SetStatus { .. } => {}
        Command::UploadFile {
            channel_id,
            path,
            name,
            content,
        } => match client.send_file(&channel_id, &content, &path).await {
            Ok(_) => {
                let _ = ui.send(UiEvent::TransferDone {
                    note: Some(format!("Загружено: {name}")),
                    error: None,
                });
            }
            Err(error) => {
                let _ = ui.send(UiEvent::TransferDone {
                    note: None,
                    error: Some(error.to_string()),
                });
            }
        },
        Command::DownloadFile { url, dest, name } => {
            let result = match client.download(&url).await {
                Ok(bytes) => tokio::fs::write(&dest, bytes)
                    .await
                    .map_err(|error| error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            match result {
                Ok(()) => {
                    let _ = ui.send(UiEvent::TransferDone {
                        note: Some(format!("Сохранено: {name}")),
                        error: None,
                    });
                }
                Err(error) => {
                    let _ = ui.send(UiEvent::TransferDone {
                        note: None,
                        error: Some(error),
                    });
                }
            }
        }
        Command::React {
            channel_id,
            message_id,
            emoji,
        } => {
            client.react(&channel_id, &message_id, &emoji).await?;
        }
        Command::Unreact {
            channel_id,
            message_id,
            emoji,
        } => {
            client.unreact(&channel_id, &message_id, &emoji).await?;
        }
        Command::VoiceJoin { .. }
        | Command::VoiceLeave
        | Command::SetMute { .. }
        | Command::SetDevices { .. }
        | Command::CallDecline { .. } => {}
    }
    Ok(())
}

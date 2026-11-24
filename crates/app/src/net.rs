use rustycord_discord::{Discord, Event, Message, User};
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
}

pub enum UiEvent {
    LoggedIn,
    LoginFailed(String),
    Discord(Event),
    History {
        channel_id: String,
        messages: Vec<Message>,
    },
    AckToken {
        channel_id: String,
        token: String,
    },
    Error(String),
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
    let (client, _me) = match Discord::login(token).await {
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
                let _ = ui.send(UiEvent::Discord(Event::Users(users)));
                let _ = ui.send(UiEvent::Discord(Event::PrivateChannels(channels)));
            }
            if let Ok(relationships) = client.relationships().await {
                let _ = ui.send(UiEvent::Discord(Event::Relationships(relationships)));
            }
        });
    }

    let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::channel(1024);
    let gateway = client.spawn_gateway(gateway_tx);

    loop {
        tokio::select! {
            Some(event) = gateway_rx.recv() => {
                let _ = ui.send(UiEvent::Discord(event));
            }
            Some(command) = commands.recv() => {
                if let Command::SubscribeMembers {
                    guild_id,
                    channel_id,
                } = command
                {
                    gateway.subscribe_members(&guild_id, &channel_id);
                } else {
                    let client = client.clone();
                    let ui = ui.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle(client, command, &ui).await {
                            let _ = ui.send(UiEvent::Error(error.to_string()));
                        }
                    });
                }
            }
            else => break,
        }
    }
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
    }
    Ok(())
}

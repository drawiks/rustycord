use std::{env, error::Error};

use rustycord_discord::{ConnectionState, Discord, Event, State};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let token = env::args()
        .nth(1)
        .or_else(|| env::var("DISCORD_TOKEN").ok())
        .ok_or("usage: headless <token> (or set DISCORD_TOKEN)")?;

    let (client, me) = Discord::login(token).await?;
    println!("logged in as {} ({})", me.display_name(), me.id);

    let (tx, mut rx) = mpsc::channel(1024);
    let _gateway = client.spawn_gateway(tx);
    let mut state = State::default();

    loop {
        let event = tokio::select! {
            event = rx.recv() => match event { Some(event) => event, None => break },
            _ = tokio::signal::ctrl_c() => break,
        };
        state.apply(&event);

        match &event {
            Event::Ready(_) => {
                println!(
                    "ready: {} guilds, {} private channels, {} users cached",
                    state.guilds.len(),
                    state.channels.len(),
                    state.users.len()
                );
                for guild in &state.guilds {
                    println!(
                        "guild: {} ({} channels, {} members reported)",
                        guild.name,
                        guild.channels.len(),
                        guild.member_count
                    );
                    for member in &guild.members {
                        println!(
                            "  member {} ({}) roles={:?}",
                            member.display_name(),
                            member.user.id,
                            member.roles
                        );
                    }
                    for channel in &guild.channels {
                        println!(
                            "  {:?} type={} parent={:?} pos={}",
                            channel.name, channel.kind, channel.parent_id, channel.position
                        );
                    }
                }
                for channel in &state.channels {
                    println!("dm: {}", state.channel_display_name(channel));
                }
            }
            Event::GuildCreate(guild) => println!("guild: {}", guild.name),
            Event::MemberChunk { guild_id, members } => {
                for member in members {
                    println!(
                        "member: {} ({}) roles={:?}",
                        member.display_name(),
                        member.user.id,
                        member.roles
                    );
                }
                println!("members chunk: guild {guild_id}");
            }
            Event::MessageCreate(message) => {
                let channel = state
                    .channel(&message.channel_id)
                    .map(|c| c.display_name())
                    .unwrap_or_else(|| message.channel_id.clone());
                println!(
                    "[{channel}] {}: {}",
                    message.author.display_name(),
                    message.content
                );
            }
            Event::Connection(ConnectionState::Disconnected { reason }) => {
                eprintln!("disconnected: {reason}");
                break;
            }
            Event::Connection(ConnectionState::Reconnecting { attempt }) => {
                eprintln!("reconnecting (attempt {attempt})")
            }
            _ => {}
        }
    }

    Ok(())
}

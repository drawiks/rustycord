mod chat;
mod login;
mod rail;
mod sidebar;
mod widgets;

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use chrono::{DateTime, Local, NaiveDate};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::{
    ClipboardItem, Focusable, FontWeight, ScrollHandle, Subscription, Window, div, prelude::*, px,
    rgb,
};
use rustycord_discord::{Channel, ConnectionState, Event, Id, Message, State, User, snowflake};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::{
    markdown,
    net::{Command, Net, UiEvent},
    theme::*,
};

const TYPING_COOLDOWN: Duration = Duration::from_secs(8);
const ACK_INTERVAL: Duration = Duration::from_secs(1);

enum Screen {
    Login,
    Connecting,
    Main,
}

pub struct App {
    screen: Screen,
    state: State,
    net: Option<Net>,
    ui_tx: UnboundedSender<UiEvent>,
    token_input: gpui_kit::Entity<InputState>,
    composer: gpui_kit::Entity<TextareaState>,
    selected_guild: Option<Id>,
    selected_channel: Option<Id>,
    error: Option<String>,
    connection: String,
    scroll: ScrollHandle,
    last_typing: Option<Instant>,
    editing: Option<(Id, Id)>,
    replying: Option<Message>,
    collapsed: HashSet<Id>,
    image_viewer: Option<String>,
    unread_marker: Option<Id>,
    pending_token: Option<String>,
    ack_tokens: HashMap<Id, String>,
    last_ack: Option<Instant>,
    loading_older: bool,
    pending_anchor: Option<Id>,
    subscriptions: Vec<Subscription>,
}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let token_input = cx.new(|cx| InputState::new(window, cx).placeholder("User token"));
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Message")
                .submit_on_enter(true)
        });
        let (ui_tx, mut ui_rx) = unbounded_channel();

        cx.spawn(async move |this, cx| {
            while let Some(event) = ui_rx.recv().await {
                if this.update(cx, |app, cx| app.handle(event, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        let mut app = Self {
            screen: Screen::Login,
            state: State::default(),
            net: None,
            ui_tx,
            token_input,
            composer,
            selected_guild: None,
            selected_channel: None,
            error: None,
            connection: String::new(),
            scroll: ScrollHandle::new(),
            last_typing: None,
            editing: None,
            replying: None,
            collapsed: HashSet::new(),
            image_viewer: None,
            unread_marker: None,
            pending_token: None,
            ack_tokens: HashMap::new(),
            last_ack: None,
            loading_older: false,
            pending_anchor: None,
            subscriptions: Vec::new(),
        };

        app.subscriptions.push(cx.subscribe(
            &app.token_input,
            |this, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    this.login(cx);
                }
            },
        ));
        app.subscriptions.push(cx.subscribe_in(
            &app.composer,
            window,
            |this, _input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. } => this.send_message(window, cx),
                InputEvent::Change => this.notify_typing(cx),
                _ => {}
            },
        ));

        let handle = app.token_input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);

        if let Some(token) = std::env::var("DISCORD_TOKEN").ok().or_else(load_token) {
            app.token_input
                .update(cx, |input, cx| input.set_value(token, window, cx));
        }

        app
    }

    fn login(&mut self, cx: &mut Context<Self>) {
        let token = self.token_input.read(cx).value().trim().to_string();
        if token.is_empty() {
            self.error = Some("Token is empty".to_string());
            cx.notify();
            return;
        }
        self.screen = Screen::Connecting;
        self.error = None;
        self.connection = "connecting".to_string();
        self.pending_token = Some(token.clone());
        self.net = Some(crate::net::spawn(token, self.ui_tx.clone()));
        cx.notify();
    }

    fn handle(&mut self, event: UiEvent, cx: &mut Context<Self>) {
        match event {
            UiEvent::AckToken { channel_id, token } => {
                self.ack_tokens.insert(channel_id, token);
            }
            UiEvent::LoggedIn => {
                self.screen = Screen::Main;
                self.connection = "connecting".to_string();
                if let Some(token) = self.pending_token.take() {
                    save_token(&token);
                }
            }
            UiEvent::LoginFailed(error) | UiEvent::Error(error) => {
                if matches!(self.screen, Screen::Connecting) {
                    self.screen = Screen::Login;
                    self.net = None;
                }
                self.error = Some(error);
            }
            UiEvent::Discord(event) => {
                if let Event::Connection(state) = &event {
                    self.connection = match state {
                        ConnectionState::Connected => "connected".to_string(),
                        ConnectionState::Resumed => "connected".to_string(),
                        ConnectionState::Reconnecting { attempt } => {
                            format!("reconnecting ({attempt})")
                        }
                        ConnectionState::Disconnected { reason } => format!("offline: {reason}"),
                    };
                }
                let ready = matches!(event, Event::Ready(_));
                let (scroll, mine) = match &event {
                    Event::MessageCreate(message) => (
                        Some(&message.channel_id) == self.selected_channel.as_ref(),
                        self.state
                            .me
                            .as_ref()
                            .is_some_and(|me| me.id == message.author.id),
                    ),
                    _ => (false, false),
                };
                self.state.apply(&event);
                if ready {
                    self.pick_default_channel(cx);
                }
                if scroll {
                    if mine || self.at_bottom() {
                        self.scroll.scroll_to_bottom();
                    }
                    if let Some(channel_id) = self.selected_channel.clone() {
                        self.mark_read(&channel_id, false);
                    }
                }
            }
            UiEvent::History {
                channel_id,
                messages,
            } => {
                let first_load = self.state.messages(&channel_id).is_empty();
                let received = messages.len();
                self.state.insert_history(&channel_id, messages);
                self.loading_older = false;
                if received == 0 {
                    self.pending_anchor = None;
                }
                if Some(&channel_id) == self.selected_channel.as_ref() && first_load {
                    self.scroll.scroll_to_bottom();
                    self.mark_read(&channel_id, true);
                }
            }
        }
        cx.notify();
    }

    fn pick_default_channel(&mut self, cx: &mut Context<Self>) {
        let guild = self.state.guilds.first().map(|g| g.id.clone());
        self.selected_guild = guild.clone();
        let channel = guild
            .and_then(|id| self.state.text_channels(&id).first().map(|c| c.id.clone()))
            .or_else(|| self.state.channels.first().map(|c| c.id.clone()));
        if let Some(channel) = channel {
            self.select_channel(channel, cx);
        }
    }

    fn select_guild(&mut self, guild: Option<Id>, cx: &mut Context<Self>) {
        self.selected_guild = guild.clone();
        let channel = guild
            .as_ref()
            .and_then(|id| self.state.text_channels(id).first().map(|c| c.id.clone()))
            .or_else(|| {
                if guild.is_none() {
                    self.state.channels.first().map(|c| c.id.clone())
                } else {
                    None
                }
            });
        self.selected_channel = None;
        if let Some(channel) = channel {
            self.select_channel(channel, cx);
        } else {
            cx.notify();
        }
    }

    fn select_channel(&mut self, channel_id: Id, cx: &mut Context<Self>) {
        if self.selected_channel.as_ref() == Some(&channel_id) {
            return;
        }
        self.selected_channel = Some(channel_id.clone());
        self.unread_marker = self
            .state
            .read_states
            .get(&channel_id)
            .and_then(|read| read.last_message_id.clone());
        self.last_typing = None;
        if let Some(net) = &self.net {
            net.send(Command::LoadHistory {
                channel_id: channel_id.clone(),
                before: None,
            });
        }
        self.mark_read(&channel_id, true);
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn channel_unread(&self, channel: &Channel) -> (bool, u32) {
        let read = self.state.read_states.get(&channel.id);
        let last = channel
            .last_message_id
            .as_deref()
            .map(snowflake)
            .unwrap_or(0);
        let read_last = read
            .and_then(|state| state.last_message_id.as_deref())
            .map(snowflake)
            .unwrap_or(0);
        let mentions = read.map(|state| state.mention_count).unwrap_or(0);
        (last > read_last, mentions)
    }

    fn guild_unread(&self, guild_id: &str) -> (bool, u32) {
        let Some(guild) = self.state.guild(guild_id) else {
            return (false, 0);
        };
        let mut unread = false;
        let mut mentions = 0;
        for channel in guild.channels.iter().filter(|channel| channel.is_text()) {
            let (channel_unread, channel_mentions) = self.channel_unread(channel);
            unread |= channel_unread;
            mentions += channel_mentions;
        }
        (unread, mentions)
    }

    fn at_bottom(&self) -> bool {
        (self.scroll.max_offset().y - self.scroll.offset().y) < px(60.)
    }

    fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.composer.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    fn mark_read(&mut self, channel_id: &str, force: bool) {
        if !force && self.last_ack.is_some_and(|at| at.elapsed() < ACK_INTERVAL) {
            return;
        }
        let Some(message_id) = self
            .state
            .messages(channel_id)
            .last()
            .map(|message| message.id.clone())
        else {
            return;
        };
        let token = self.ack_tokens.get(channel_id).cloned();
        if let Some(net) = &self.net {
            net.send(Command::Ack {
                channel_id: channel_id.to_string(),
                message_id: message_id.clone(),
                token,
            });
            self.last_ack = Some(Instant::now());
            let read = self
                .state
                .read_states
                .entry(channel_id.to_string())
                .or_default();
            read.id = channel_id.to_string();
            read.last_message_id = Some(message_id);
        }
    }

    fn maybe_load_older(&mut self) {
        if self.loading_older {
            return;
        }
        let Some(channel_id) = self.selected_channel.clone() else {
            return;
        };
        if self.state.messages(&channel_id).len() < 50 || self.scroll.offset().y > px(120.) {
            return;
        }
        self.pending_anchor = self
            .state
            .messages(&channel_id)
            .first()
            .map(|message| message.id.clone());
        self.loading_older = true;
        self.load_older();
    }

    fn load_older(&self) {
        let Some(channel_id) = self.selected_channel.clone() else {
            return;
        };
        let before = self
            .state
            .messages(&channel_id)
            .first()
            .map(|message| message.id.clone());
        if let (Some(net), Some(before)) = (&self.net, before) {
            net.send(Command::LoadHistory {
                channel_id,
                before: Some(before),
            });
        }
    }

    fn start_reply(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        self.replying = Some(message);
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn cancel_reply(&mut self, cx: &mut Context<Self>) {
        self.replying = None;
        cx.notify();
    }

    fn logout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.net = None;
        self.screen = Screen::Login;
        self.state = State::default();
        self.selected_guild = None;
        self.selected_channel = None;
        self.editing = None;
        self.replying = None;
        self.pending_token = None;
        self.ack_tokens.clear();
        self.last_ack = None;
        self.composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        delete_token();
        cx.notify();
    }

    fn open_target(
        &mut self,
        target: markdown::Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let channel_id = match target {
            markdown::Target::Channel(id) => Some(id),
            markdown::Target::User(id) => self
                .state
                .channels
                .iter()
                .find(|channel| {
                    channel.kind == Channel::DM
                        && (channel.recipient_ids.contains(&id)
                            || channel.recipients.iter().any(|user| user.id == id))
                })
                .map(|channel| channel.id.clone()),
            markdown::Target::Url(_) => None,
        };
        if let Some(channel_id) = channel_id {
            self.select_channel(channel_id, cx);
            self.focus_composer(window, cx);
        }
    }

    fn start_edit(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        self.replying = None;
        self.editing = Some((message.channel_id.clone(), message.id.clone()));
        self.composer.update(cx, |composer, cx| {
            composer.set_value(message.content.clone(), window, cx);
        });
        let handle = self.composer.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        cx.notify();
    }

    fn delete_message(&self, message: &Message) {
        if let Some(net) = &self.net {
            net.send(Command::Delete {
                channel_id: message.channel_id.clone(),
                message_id: message.id.clone(),
            });
        }
    }

    fn notify_typing(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel.clone() else {
            return;
        };
        if self
            .last_typing
            .is_some_and(|at| at.elapsed() < TYPING_COOLDOWN)
        {
            return;
        }
        self.last_typing = Some(Instant::now());
        if let Some(net) = &self.net {
            net.send(Command::Typing { channel_id });
        }
        cx.notify();
    }

    fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.composer.read(cx).value().trim().to_string();
        let Some(channel_id) = self.selected_channel.clone() else {
            return;
        };
        if content.is_empty() {
            return;
        }
        let reply_to = self.replying.take().map(|message| message.id.clone());
        if let Some((edit_channel, edit_message)) = self.editing.take() {
            if let Some(net) = &self.net {
                net.send(Command::Edit {
                    channel_id: edit_channel,
                    message_id: edit_message,
                    content,
                });
            }
        } else if let Some(net) = &self.net {
            net.send(Command::Send {
                channel_id,
                content,
                reply_to,
            });
        }
        self.composer.update(cx, |composer, cx| {
            composer.set_value("", window, cx);
        });
        cx.notify();
    }

    fn react(&self, message: &Message, emoji: &rustycord_discord::Emoji) {
        let Some(net) = &self.net else {
            return;
        };
        let channel_id = message.channel_id.clone();
        let message_id = message.id.clone();
        let emoji = emoji.text();
        if emoji.is_empty() {
            return;
        }
        if message
            .reactions
            .iter()
            .any(|r| r.me && r.emoji.text() == emoji)
        {
            net.send(Command::Unreact {
                channel_id,
                message_id,
                emoji,
            });
        } else {
            net.send(Command::React {
                channel_id,
                message_id,
                emoji,
            });
        }
    }

    fn render_main(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .bg(rgb(RAIL))
            .child(self.render_rail(cx))
            .child(self.render_sidebar(cx))
            .child(self.render_chat(cx));

        if let Some(url) = self.image_viewer.clone() {
            root = root.child(
                div()
                    .id("image-viewer")
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui_kit::rgba(0x000000d9))
                    .cursor_pointer()
                    .child(
                        gpui_kit::img(url)
                            .max_w(px(900.))
                            .max_h(px(700.))
                            .rounded(px(6.)),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.image_viewer = None;
                        cx.notify();
                    })),
            );
        }

        root
    }
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.screen {
            Screen::Login | Screen::Connecting => self.render_login(cx).into_any_element(),
            Screen::Main => self.render_main(cx).into_any_element(),
        }
    }
}

fn load_token() -> Option<String> {
    keyring::Entry::new("rustycord", "token")
        .ok()?
        .get_password()
        .ok()
}

fn delete_token() {
    if let Ok(entry) = keyring::Entry::new("rustycord", "token") {
        let _ = entry.delete_credential();
    }
}

fn save_token(token: &str) {
    match keyring::Entry::new("rustycord", "token").and_then(|entry| entry.set_password(token)) {
        Ok(()) => tracing::info!("token saved to keyring"),
        Err(error) => tracing::warn!("could not save token: {error}"),
    }
}

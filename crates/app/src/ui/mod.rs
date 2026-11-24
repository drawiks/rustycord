mod chat;
mod login;
mod members;
mod rail;
mod sidebar;
mod widgets;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local, NaiveDate};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::{
    ClipboardItem, Focusable, FontWeight, Subscription, Window, div, prelude::*, px, rgb,
};
use rustycord_discord::{Channel, ConnectionState, Event, Id, Message, State, User, snowflake};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use self::widgets::{day_label, parse_timestamp};
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
    transcript: gpui_kit::Entity<MessageScrollerState>,
    rows: Vec<Row>,
    rows_channel: Option<Id>,
    text_cache: HashMap<Id, markdown::Layout>,
    last_typing: Option<Instant>,
    editing: Option<(Id, Id)>,
    replying: Option<Message>,
    collapsed: HashSet<Id>,
    image_viewer: Option<String>,
    unread_marker: Option<Id>,
    right_pane: Option<RightPane>,
    member_rows: Vec<MemberRow>,
    transfers: usize,
    notice: Option<String>,
    pending_token: Option<String>,
    ack_tokens: HashMap<Id, String>,
    last_ack: Option<Instant>,
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
        let transcript = cx.new(|cx| MessageScrollerState::new(0, cx));
        let (ui_tx, mut ui_rx) = unbounded_channel();
        let debug = debug_render();

        cx.spawn(async move |this, cx| {
            while let Some(first) = ui_rx.recv().await {
                let mut batch = vec![first];
                while let Ok(event) = ui_rx.try_recv() {
                    batch.push(event);
                }
                let count = batch.len();
                let started = Instant::now();
                let updated = this.update(cx, |app, cx| {
                    for event in batch {
                        app.handle(event, cx);
                    }
                    cx.notify();
                });
                if debug {
                    tracing::info!("apply {count} events in {:?}", started.elapsed());
                }
                if updated.is_err() {
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
            transcript,
            rows: Vec::new(),
            rows_channel: None,
            text_cache: HashMap::new(),
            last_typing: None,
            editing: None,
            replying: None,
            collapsed: HashSet::new(),
            image_viewer: None,
            unread_marker: None,
            right_pane: None,
            member_rows: Vec::new(),
            transfers: 0,
            notice: None,
            pending_token: None,
            ack_tokens: HashMap::new(),
            last_ack: None,
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
            UiEvent::TransferDone { note, error } => {
                self.transfers = self.transfers.saturating_sub(1);
                if let Some(error) = error {
                    self.error = Some(error);
                } else if let Some(note) = note {
                    self.notice = Some(note);
                }
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
                let mut names_changed = false;
                match &event {
                    Event::MessageUpdate(message) => {
                        self.text_cache.remove(&message.id);
                    }
                    Event::MessageDelete { id, .. } => {
                        self.text_cache.remove(id);
                    }
                    Event::Users(_)
                    | Event::PrivateChannels(_)
                    | Event::Relationships(_)
                    | Event::GuildCreate(_)
                    | Event::GuildUpdate(_)
                    | Event::ChannelCreate(_)
                    | Event::ChannelUpdate(_)
                    | Event::ChannelDelete(_)
                    | Event::MemberUpdate { .. }
                    | Event::MemberRemove { .. }
                    | Event::UserUpdate(_) => {
                        self.text_cache.clear();
                        names_changed = true;
                    }
                    _ => {}
                }
                self.state.apply(&event);
                if ready {
                    self.pick_default_channel(cx);
                }
                self.sync_transcript(cx);
                if names_changed {
                    self.transcript.update(cx, |state, cx| state.remeasure(cx));
                }
                let members_changed = matches!(
                    &event,
                    Event::MemberUpdate { .. }
                        | Event::MemberRemove { .. }
                        | Event::MemberChunk { .. }
                        | Event::PresenceUpdate(_)
                        | Event::GuildCreate(_)
                        | Event::GuildUpdate(_)
                        | Event::Users(_)
                        | Event::Ready(_)
                );
                if members_changed && matches!(self.right_pane, Some(RightPane::Members)) {
                    self.sync_members();
                }
                if ready && matches!(self.right_pane, Some(RightPane::Members)) {
                    self.request_members();
                }
                if let Event::MessageCreate(message) = &event
                    && Some(&message.channel_id) == self.selected_channel.as_ref()
                {
                    self.mark_read(&message.channel_id, false);
                }
            }
            UiEvent::History {
                channel_id,
                messages,
            } => {
                let first_load = self.state.messages(&channel_id).is_empty();
                self.state.insert_history(&channel_id, messages);
                if Some(&channel_id) == self.selected_channel.as_ref() {
                    self.sync_transcript(cx);
                    if first_load {
                        self.transcript
                            .update(cx, |state, cx| state.scroll_to_end(cx));
                        self.mark_read(&channel_id, true);
                    }
                }
            }
        }
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
        if let Some(RightPane::Profile {
            guild_id: Some(profile_guild),
            ..
        }) = &self.right_pane
            && Some(profile_guild) != guild.as_ref()
        {
            self.right_pane = None;
        }
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
        if matches!(self.right_pane, Some(RightPane::Members)) {
            self.sync_members();
            self.request_members();
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
        self.sync_transcript(cx);
        if matches!(self.right_pane, Some(RightPane::Members)) {
            self.request_members();
        }
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

    fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = match self
            .selected_channel
            .as_ref()
            .and_then(|id| self.state.channel(id))
        {
            Some(channel) if channel.kind == Channel::DM || channel.kind == Channel::GROUP_DM => {
                format!("Message @{}", self.state.channel_display_name(channel))
            }
            Some(channel) => format!("Message #{}", self.state.channel_display_name(channel)),
            None => "Message".to_string(),
        };
        self.composer.update(cx, |composer, cx| {
            composer.set_placeholder(placeholder, window, cx)
        });
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

    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach files".into()),
        });
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                for path in paths {
                    let _ = entity.update(cx, |app, cx| app.enqueue_upload(path, cx));
                }
            }
        })
        .detach();
    }

    fn enqueue_upload(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel.clone() else {
            self.error = Some("Select a channel first".to_string());
            cx.notify();
            return;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());

        match std::fs::metadata(&path) {
            Ok(meta) if meta.len() > 10 * 1024 * 1024 => {
                self.error = Some(format!("{name} exceeds the 10 MB upload limit"));
            }
            Ok(_) => {
                if let Some(net) = &self.net {
                    net.send(Command::UploadFile {
                        channel_id,
                        path,
                        name: name.clone(),
                        content: String::new(),
                    });
                    self.transfers += 1;
                    self.notice = Some(format!("Uploading {name}…"));
                }
            }
            Err(error) => self.error = Some(format!("Cannot read {name}: {error}")),
        }
        cx.notify();
    }

    fn pick_download(&mut self, url: String, name: String, cx: &mut Context<Self>) {
        let directory = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&directory, Some(&name));
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(dest))) = receiver.await {
                let _ = entity.update(cx, |app, cx| app.start_download(url, dest, name, cx));
            }
        })
        .detach();
    }

    fn start_download(&mut self, url: String, dest: PathBuf, name: String, cx: &mut Context<Self>) {
        if let Some(net) = &self.net {
            net.send(Command::DownloadFile {
                url,
                dest,
                name: name.clone(),
            });
            self.transfers += 1;
            self.notice = Some(format!("Downloading {name}…"));
        }
        cx.notify();
    }

    fn request_members(&self) {
        let Some(guild_id) = self.selected_guild.clone() else {
            return;
        };
        let channel_id = self
            .selected_channel
            .clone()
            .filter(|id| {
                self.state
                    .channel(id)
                    .and_then(|channel| channel.guild_id.as_deref())
                    == Some(guild_id.as_str())
            })
            .or_else(|| {
                self.state
                    .text_channels(&guild_id)
                    .first()
                    .map(|channel| channel.id.clone())
            });
        if let (Some(net), Some(channel_id)) = (&self.net, channel_id) {
            net.send(Command::SubscribeMembers {
                guild_id,
                channel_id,
            });
        }
    }

    fn sync_members(&mut self) {
        self.member_rows.clear();
        let Some(guild_id) = self.selected_guild.clone() else {
            return;
        };

        let mut online: Vec<(i32, String, Id)> = Vec::new();
        let mut offline: Vec<(String, Id)> = Vec::new();
        for member in self.state.members(&guild_id) {
            let name = member.display_name().to_lowercase();
            if self.state.presence(member.id()) == "offline" {
                offline.push((name, member.id().to_string()));
            } else {
                let position = self
                    .state
                    .member_roles(&guild_id, member.id())
                    .first()
                    .map(|role| role.position)
                    .unwrap_or(0);
                online.push((position, name, member.user.id.clone()));
            }
        }
        online.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        offline.sort_by(|a, b| a.0.cmp(&b.0));

        if !online.is_empty() {
            self.member_rows.push(MemberRow::Header {
                label: "ONLINE",
                count: online.len(),
            });
            self.member_rows.extend(
                online
                    .into_iter()
                    .map(|(_, _, user_id)| MemberRow::Member { user_id }),
            );
        }
        if !offline.is_empty() {
            self.member_rows.push(MemberRow::Header {
                label: "OFFLINE",
                count: offline.len(),
            });
            self.member_rows.extend(
                offline
                    .into_iter()
                    .map(|(_, user_id)| MemberRow::Member { user_id }),
            );
        }
    }

    fn sync_transcript(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel.clone() else {
            self.rows.clear();
            self.rows_channel = None;
            self.transcript.update(cx, |state, cx| state.reset(0, cx));
            return;
        };

        let new_rows = self.compute_rows(&channel_id);
        let same_channel = self.rows_channel.as_deref() == Some(channel_id.as_str());

        if !same_channel {
            self.rows = new_rows;
            self.rows_channel = Some(channel_id.clone());
            let count = self.rows.len();
            self.transcript.update(cx, |state, cx| {
                state.reset(count, cx);
                state.scroll_to_end(cx);
            });
        } else if self.rows != new_rows {
            let old = std::mem::replace(&mut self.rows, new_rows);
            let (prefix, suffix) = common_affixes(&old, &self.rows);
            let old_range = prefix..old.len().saturating_sub(suffix);
            let new_count = self.rows.len().saturating_sub(prefix + suffix);
            let total = self.rows.len();
            let spliced = self.transcript.update(cx, |state, cx| {
                if old_range.is_empty() {
                    state.splice(prefix..prefix, new_count, cx)
                } else {
                    state.splice(old_range, new_count, cx)
                }
            });
            if !spliced {
                self.transcript
                    .update(cx, |state, cx| state.reset(total, cx));
            }
        }

        self.ensure_text_cache(&channel_id);
    }

    fn compute_rows(&self, channel_id: &str) -> Vec<Row> {
        let messages = self.state.messages(channel_id);
        let mut rows = Vec::with_capacity(messages.len() + 8);
        if messages.len() >= 50 {
            rows.push(Row::LoadOlder);
        }

        let read_marker = self.unread_marker.clone();
        let mut previous: Option<(Id, DateTime<Local>)> = None;
        let mut last_date: Option<NaiveDate> = None;
        let mut unread_shown = false;
        let mut first = true;

        for message in messages {
            let timestamp = parse_timestamp(&message.timestamp);
            let date = timestamp.map(|time| time.date_naive());
            if let Some(date) = date
                && last_date != Some(date)
            {
                rows.push(Row::Date(day_label(date)));
                last_date = Some(date);
            }
            if !unread_shown
                && !first
                && let Some(marker) = &read_marker
                && snowflake(&message.id) > snowflake(marker)
            {
                rows.push(Row::Unread);
                unread_shown = true;
            }
            let compact = match (&previous, timestamp) {
                (Some((author, at)), Some(now)) => {
                    author == &message.author.id
                        && now.signed_duration_since(*at) < chrono::Duration::minutes(5)
                }
                _ => false,
            };
            previous = timestamp.map(|time| (message.author.id.clone(), time));
            first = false;
            rows.push(Row::Message {
                id: message.id.clone(),
                compact,
            });
        }

        rows
    }

    fn ensure_text_cache(&mut self, channel_id: &str) {
        let ids: HashSet<&str> = self
            .state
            .messages(channel_id)
            .iter()
            .map(|message| message.id.as_str())
            .collect();
        self.text_cache.retain(|id, _| ids.contains(id.as_str()));

        let mut missing: Vec<(Id, String)> = Vec::new();
        for message in self.state.messages(channel_id) {
            if !message.content.is_empty() && !self.text_cache.contains_key(&message.id) {
                missing.push((message.id.clone(), message.content.clone()));
            }
        }
        for (id, content) in missing {
            let layout = markdown::layout(&content, &self.state);
            self.text_cache.insert(id, layout);
        }
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

    fn current_message(&self, message_id: &str) -> Option<Message> {
        let channel = self
            .rows_channel
            .clone()
            .or_else(|| self.selected_channel.clone())?;
        self.state.message(&channel, message_id).cloned()
    }

    fn start_reply_by_id(&mut self, message_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(message) = self.current_message(message_id) {
            self.start_reply(message, window, cx);
        }
    }

    fn start_edit_by_id(&mut self, message_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(message) = self.current_message(message_id) {
            self.start_edit(message, window, cx);
        }
    }

    fn delete_by_id(&self, message_id: &str, cx: &mut Context<Self>) {
        let Some(channel) = self.rows_channel.clone() else {
            return;
        };
        if let Some(net) = &self.net {
            net.send(Command::Delete {
                channel_id: channel,
                message_id: message_id.to_string(),
            });
            cx.notify();
        }
    }

    fn copy_message_text(&self, message_id: &str, cx: &mut Context<Self>) {
        let Some(channel) = self.rows_channel.as_deref() else {
            return;
        };
        if let Some(message) = self.state.message(channel, message_id) {
            cx.write_to_clipboard(ClipboardItem::new_string(message.content.clone()));
        }
    }

    fn toggle_reaction(
        &mut self,
        message_id: &str,
        emoji: &rustycord_discord::Emoji,
        cx: &mut Context<Self>,
    ) {
        let Some(channel) = self.rows_channel.clone() else {
            return;
        };
        let Some(message) = self.state.message(&channel, message_id) else {
            return;
        };
        let key = emoji.text();
        let mine = message
            .reactions
            .iter()
            .any(|reaction| reaction.emoji.text() == key && reaction.me);
        if let Some(net) = &self.net {
            if mine {
                net.send(Command::Unreact {
                    channel_id: channel,
                    message_id: message_id.to_string(),
                    emoji: key,
                });
            } else {
                net.send(Command::React {
                    channel_id: channel,
                    message_id: message_id.to_string(),
                    emoji: key,
                });
            }
        }
        cx.notify();
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

    fn render_main(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .font_family("Inter Variable")
            .bg(rgb(RAIL))
            .child(self.render_rail(cx))
            .child(self.render_sidebar(cx))
            .child(self.render_chat(cx));

        if self.right_pane.is_some() {
            root = root.child(self.render_right_pane(cx));
        }

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

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Row {
    LoadOlder,
    Date(String),
    Unread,
    Message { id: Id, compact: bool },
}

fn common_affixes(old: &[Row], new: &[Row]) -> (usize, usize) {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let limit = old.len().min(new.len()).saturating_sub(prefix);
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(limit);
    (prefix, suffix)
}

fn debug_render() -> bool {
    use std::sync::OnceLock;

    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var_os("RUSTCORD_DEBUG_RENDER").is_some())
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum RightPane {
    Members,
    Profile { user_id: Id, guild_id: Option<Id> },
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum MemberRow {
    Header { label: &'static str, count: usize },
    Member { user_id: Id },
}

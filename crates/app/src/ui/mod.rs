mod chat;
mod login;
mod members;
mod overlays;
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
use rustycord_discord::{
    Call, Channel, ConnectionState, Event, Id, Member, Message, Role, State, User, snowflake,
};
use rustycord_voice::VoiceEvent;
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
    search_input: gpui_kit::Entity<InputState>,
    search_query: String,
    window_title: Option<String>,
    profile_popup: Option<(Id, Option<Id>)>,
    emoji_picker: bool,
    status_menu: bool,
    transfers: usize,
    notice: Option<String>,
    pending_token: Option<String>,
    ack_tokens: HashMap<Id, String>,
    last_ack: Option<Instant>,
    voice: Option<VoiceUi>,
    voice_status: String,
    voice_mode: Option<String>,
    voice_dave: bool,
    voice_latency: Option<u32>,
    speaking: HashSet<Id>,
    muted: bool,
    deafened: bool,
    incoming_call: Option<Call>,
    settings_open: bool,
    input_device: Option<String>,
    output_device: Option<String>,
    input_devices: Vec<String>,
    output_devices: Vec<String>,
    subscriptions: Vec<Subscription>,
}

#[derive(Clone, PartialEq, Eq)]
struct VoiceUi {
    channel_id: Id,
}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let token_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Токен пользователя"));
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Написать")
                .submit_on_enter(true)
        });
        let transcript = cx.new(|cx| MessageScrollerState::new(0, cx));
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Поиск")
                .clean_on_escape()
        });
        let (input_device, output_device) = load_audio_devices();
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
            search_input,
            search_query: String::new(),
            window_title: None,
            profile_popup: None,
            emoji_picker: false,
            status_menu: false,
            transfers: 0,
            notice: None,
            pending_token: None,
            ack_tokens: HashMap::new(),
            last_ack: None,
            voice: None,
            voice_status: String::new(),
            voice_mode: None,
            voice_dave: false,
            voice_latency: None,
            speaking: HashSet::new(),
            muted: false,
            deafened: false,
            incoming_call: None,
            settings_open: false,
            input_device,
            output_device,
            input_devices: Vec::new(),
            output_devices: Vec::new(),
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
        app.subscriptions.push(cx.subscribe(
            &app.search_input,
            |this, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.search_query = this.search_input.read(cx).value().to_string();
                    this.sync_transcript(cx);
                    cx.notify();
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
            self.error = Some("Токен пуст".to_string());
            cx.notify();
            return;
        }
        self.screen = Screen::Connecting;
        self.error = None;
        self.connection = "подключение…".to_string();
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
                self.connection = "подключение…".to_string();
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
            UiEvent::Voice(event) => match event {
                VoiceEvent::Connecting => {
                    self.voice_status = "подключение…".to_string();
                }
                VoiceEvent::Ready { mode, dave } => {
                    self.voice_status = "подключено".to_string();
                    self.voice_mode = Some(mode);
                    self.voice_dave = dave;
                }
                VoiceEvent::Speaking {
                    user_id, speaking, ..
                } => {
                    if speaking {
                        self.speaking.insert(user_id);
                    } else {
                        self.speaking.remove(&user_id);
                    }
                }
                VoiceEvent::Ping(latency) => self.voice_latency = Some(latency),
                VoiceEvent::Disconnected { reason, .. } => {
                    self.voice = None;
                    self.voice_status.clear();
                    self.voice_mode = None;
                    self.voice_latency = None;
                    self.speaking.clear();
                    self.notice = Some(reason);
                }
                VoiceEvent::Error(error) => self.error = Some(error),
                VoiceEvent::ClientConnect { .. } | VoiceEvent::ClientDisconnect { .. } => {}
            },
            UiEvent::Discord(event) => {
                let event = *event;
                if let Event::Connection(state) = &event {
                    self.connection = match state {
                        ConnectionState::Connected => "подключено".to_string(),
                        ConnectionState::Resumed => "подключено".to_string(),
                        ConnectionState::Reconnecting { attempt } => {
                            format!("переподключение ({attempt})")
                        }
                        ConnectionState::Disconnected { reason } => format!("не в сети: {reason}"),
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
                if let Event::CallCreate(call) | Event::CallUpdate(call) = &event {
                    self.update_call(call);
                }
                if let Event::CallDelete { channel_id } = &event
                    && self
                        .incoming_call
                        .as_ref()
                        .is_some_and(|call| &call.channel_id == channel_id)
                {
                    self.incoming_call = None;
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
                if ready || members_changed {
                    self.ensure_visible_selection(cx);
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

    fn update_call(&mut self, call: &Call) {
        let Some(me) = self.state.me.as_ref() else {
            return;
        };
        let ringing = call.ringing.iter().any(|id| id == &me.id);
        let in_call = self
            .voice
            .as_ref()
            .is_some_and(|voice| voice.channel_id == call.channel_id);
        if ringing && !in_call {
            self.incoming_call = Some(call.clone());
        } else if self
            .incoming_call
            .as_ref()
            .is_some_and(|current| current.channel_id == call.channel_id)
            && (!ringing || call.ended_timestamp.is_some())
        {
            self.incoming_call = None;
        }
    }

    fn toggle_voice(&mut self, guild_id: Option<Id>, channel_id: Id, cx: &mut Context<Self>) {
        if self
            .voice
            .as_ref()
            .is_some_and(|voice| voice.channel_id == channel_id)
        {
            self.leave_voice(cx);
            return;
        }
        self.join_voice(guild_id, channel_id, false, cx);
    }

    fn join_voice(
        &mut self,
        guild_id: Option<Id>,
        channel_id: Id,
        ring: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(net) = &self.net {
            net.send(Command::VoiceJoin {
                guild_id: guild_id.clone(),
                channel_id: channel_id.clone(),
                ring,
                input_device: self.input_device.clone(),
                output_device: self.output_device.clone(),
            });
        }
        self.voice = Some(VoiceUi { channel_id });
        self.voice_status = "подключение…".to_string();
        self.voice_mode = None;
        self.voice_latency = None;
        self.speaking.clear();
        self.incoming_call = None;
        cx.notify();
    }

    fn leave_voice(&mut self, cx: &mut Context<Self>) {
        if let Some(net) = &self.net {
            net.send(Command::VoiceLeave);
        }
        self.voice = None;
        self.voice_status.clear();
        self.voice_mode = None;
        self.voice_latency = None;
        self.speaking.clear();
        cx.notify();
    }

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        if let Some(net) = &self.net {
            net.send(Command::SetMute {
                mute: self.muted,
                deafen: self.deafened,
            });
        }
        cx.notify();
    }

    fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.deafened = !self.deafened;
        if let Some(net) = &self.net {
            net.send(Command::SetMute {
                mute: self.muted,
                deafen: self.deafened,
            });
        }
        cx.notify();
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.input_devices = rustycord_voice::input_devices();
        self.output_devices = rustycord_voice::output_devices();
        self.settings_open = true;
        self.status_menu = false;
        self.emoji_picker = false;
        cx.notify();
    }

    fn select_input_device(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.input_device = name;
        save_audio_devices(&self.input_device, &self.output_device);
        if let Some(net) = &self.net {
            net.send(Command::SetDevices {
                input: self.input_device.clone(),
                output: self.output_device.clone(),
            });
        }
        cx.notify();
    }

    fn select_output_device(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.output_device = name;
        save_audio_devices(&self.input_device, &self.output_device);
        if let Some(net) = &self.net {
            net.send(Command::SetDevices {
                input: self.input_device.clone(),
                output: self.output_device.clone(),
            });
        }
        cx.notify();
    }

    fn start_call(&mut self, channel_id: Id, cx: &mut Context<Self>) {
        self.join_voice(None, channel_id, true, cx);
    }

    fn accept_call(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = self.incoming_call.take() {
            self.join_voice(None, call.channel_id, false, cx);
        }
    }

    fn decline_call(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = self.incoming_call.take()
            && let Some(net) = &self.net
        {
            net.send(Command::CallDecline {
                channel_id: call.channel_id,
            });
        }
        cx.notify();
    }

    fn pick_default_channel(&mut self, cx: &mut Context<Self>) {
        let guild = self.state.guilds.first().map(|g| g.id.clone());
        self.selected_guild = guild.clone();
        self.profile_popup = None;
        let channel = guild
            .and_then(|id| {
                self.state
                    .visible_text_channels(&id)
                    .first()
                    .map(|c| c.id.clone())
            })
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
        self.profile_popup = None;
        let channel = guild
            .as_ref()
            .and_then(|id| {
                self.state
                    .visible_text_channels(id)
                    .first()
                    .map(|c| c.id.clone())
            })
            .or_else(|| {
                if guild.is_none() {
                    self.state.channels.first().map(|c| c.id.clone())
                } else {
                    None
                }
            });
        self.selected_channel = None;
        if guild.is_some() {
            match &self.right_pane {
                None | Some(RightPane::Profile { guild_id: None, .. }) => {
                    self.right_pane = Some(RightPane::Members);
                }
                _ => {}
            }
        }
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
        self.profile_popup = None;
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
        let dm_recipient = self
            .state
            .channel(&channel_id)
            .filter(|channel| channel.kind == Channel::DM || channel.kind == Channel::GROUP_DM)
            .and_then(|channel| {
                let me = self.state.me.as_ref().map(|user| user.id.as_str());
                channel
                    .recipients
                    .iter()
                    .map(|user| &user.id)
                    .chain(channel.recipient_ids.iter())
                    .find(|id| Some(id.as_str()) != me)
                    .cloned()
            });
        match dm_recipient {
            Some(user_id) => {
                if self.right_pane.is_none() || matches!(self.right_pane, Some(RightPane::Members))
                {
                    self.right_pane = Some(RightPane::Profile {
                        user_id,
                        guild_id: None,
                    });
                }
            }
            None => {
                if self.selected_guild.is_some() && self.right_pane.is_none() {
                    self.right_pane = Some(RightPane::Members);
                    self.sync_members();
                }
            }
        }
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
                format!("Написать @{}", self.state.channel_display_name(channel))
            }
            Some(channel) => format!("Написать в #{}", self.state.channel_display_name(channel)),
            None => "Написать".to_string(),
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

    fn insert_emoji(&mut self, emoji: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut value = self.composer.read(cx).value().to_string();
        value.push_str(emoji);
        self.composer
            .update(cx, |composer, cx| composer.set_value(value, window, cx));
        self.emoji_picker = false;
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn set_status(&mut self, status: &str, cx: &mut Context<Self>) {
        if let Some(me) = self.state.me.as_ref() {
            let id = me.id.clone();
            self.state.set_status(&id, status);
            if let Some(net) = &self.net {
                net.send(Command::SetStatus {
                    status: status.to_string(),
                });
            }
        }
        self.status_menu = false;
        cx.notify();
    }

    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Прикрепить файлы".into()),
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
            self.error = Some("Сначала выберите канал".to_string());
            cx.notify();
            return;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "файл".to_string());

        match std::fs::metadata(&path) {
            Ok(meta) if meta.len() > 10 * 1024 * 1024 => {
                self.error = Some(format!("{name}: превышен лимит 10 МБ"));
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
                    self.notice = Some(format!("Загрузка {name}…"));
                }
            }
            Err(error) => self.error = Some(format!("Не удалось прочитать {name}: {error}")),
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
            self.notice = Some(format!("Скачивание {name}…"));
        }
        cx.notify();
    }

    fn ensure_visible_selection(&mut self, cx: &mut Context<Self>) {
        let Some(guild_id) = self.selected_guild.clone() else {
            return;
        };
        let hidden = self
            .selected_channel
            .as_ref()
            .and_then(|id| self.state.channel(id).map(|channel| (id.clone(), channel)))
            .is_some_and(|(id, channel)| {
                channel.guild_id.as_deref() == Some(guild_id.as_str())
                    && channel.is_text()
                    && !self.state.can_view(&guild_id, channel)
                    && self.state.channel(&id).is_some()
            });
        if hidden {
            match self
                .state
                .visible_text_channels(&guild_id)
                .first()
                .map(|channel| channel.id.clone())
            {
                Some(next) => self.select_channel(next, cx),
                None => {
                    self.selected_channel = None;
                    cx.notify();
                }
            }
        }
    }

    fn toggle_members(&mut self, cx: &mut Context<Self>) {
        if matches!(self.right_pane, Some(RightPane::Members)) {
            self.right_pane = None;
        } else {
            self.right_pane = Some(RightPane::Members);
            self.sync_members();
            self.request_members();
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
                    .visible_text_channels(&guild_id)
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
        let Some(guild) = self.state.guild(&guild_id) else {
            return;
        };

        let mut online: Vec<(&Member, Option<&Role>, String)> = Vec::new();
        let mut offline: Vec<String> = Vec::new();
        let mut hoisted: Vec<&Role> = guild.roles.iter().filter(|role| role.hoist).collect();
        hoisted.sort_by_key(|role| std::cmp::Reverse(role.position));

        for member in &guild.members {
            let name = member.display_name().to_lowercase();
            if self.state.presence(member.id()) == "offline" {
                offline.push(member.id().to_string());
            } else {
                let role = self.state.member_top_hoisted_role(&guild_id, member.id());
                online.push((member, role, name));
            }
        }
        offline.sort_by_key(|id| {
            self.state
                .member_name(&guild_id, id)
                .unwrap_or_default()
                .to_lowercase()
        });

        for role in hoisted {
            let mut members: Vec<(&Member, String)> = online
                .iter()
                .filter(|(_, member_role, _)| member_role.is_some_and(|r| r.id == role.id))
                .map(|(member, _, name)| (*member, name.clone()))
                .collect();
            if members.is_empty() {
                continue;
            }
            members.sort_by(|a, b| a.1.cmp(&b.1));
            self.member_rows.push(MemberRow::Header {
                label: role.name.clone(),
                color: role.color,
                count: members.len(),
            });
            self.member_rows
                .extend(members.into_iter().map(|(member, _)| MemberRow::Member {
                    user_id: member.id().to_string(),
                }));
        }

        let mut rest: Vec<(&Member, String)> = online
            .into_iter()
            .filter(|(_, role, _)| role.is_none())
            .map(|(member, _, name)| (member, name))
            .collect();
        rest.sort_by(|a, b| a.1.cmp(&b.1));
        if !rest.is_empty() {
            self.member_rows.push(MemberRow::Header {
                label: "В СЕТИ".to_string(),
                color: 0x949ba4,
                count: rest.len(),
            });
            self.member_rows
                .extend(rest.into_iter().map(|(member, _)| MemberRow::Member {
                    user_id: member.id().to_string(),
                }));
        }

        if !offline.is_empty() {
            self.member_rows.push(MemberRow::Header {
                label: "НЕ В СЕТИ".to_string(),
                color: 0x6d6f78,
                count: offline.len(),
            });
            self.member_rows.extend(
                offline
                    .into_iter()
                    .map(|user_id| MemberRow::Member { user_id }),
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

        let query = self.search_query.trim().to_lowercase();
        let searching = !query.is_empty();
        let read_marker = if searching {
            None
        } else {
            self.unread_marker.clone()
        };
        let mut previous: Option<(Id, DateTime<Local>)> = None;
        let mut last_date: Option<NaiveDate> = None;
        let mut unread_shown = false;
        let mut first = true;

        for message in messages {
            if searching {
                let author = self
                    .state
                    .member_name(channel_id, &message.author.id)
                    .unwrap_or_else(|| message.author.display_name())
                    .to_lowercase();
                if !message.content.to_lowercase().contains(&query) && !author.contains(&query) {
                    continue;
                }
            }
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

        if self.incoming_call.is_some() {
            root = root.child(self.render_incoming_call(cx));
        }

        if self.profile_popup.is_some() {
            root = root.child(self.render_profile_popup(cx));
        }

        if self.emoji_picker {
            root = root.child(self.render_emoji_picker(cx));
        }

        if self.status_menu {
            root = root.child(self.render_status_menu(cx));
        }

        if self.settings_open {
            root = root.child(self.render_settings(cx));
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match (&self.selected_guild, &self.selected_channel) {
            (Some(guild_id), _) => self.state.guild(guild_id).map(|guild| guild.name.clone()),
            (None, Some(channel_id)) => self
                .state
                .channel(channel_id)
                .map(|channel| self.state.channel_display_name(channel)),
            _ => None,
        }
        .unwrap_or_else(|| "rustycord".to_string());
        if self.window_title.as_deref() != Some(title.as_str()) {
            window.set_window_title(&title);
            self.window_title = Some(title);
        }

        match self.screen {
            Screen::Login | Screen::Connecting => self.render_login(cx).into_any_element(),
            Screen::Main => self.render_main(cx).into_any_element(),
        }
    }
}

fn audio_config_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|dir| dir.join("rustycord").join("audio.conf"))
}

fn load_audio_devices() -> (Option<String>, Option<String>) {
    let Some(path) = audio_config_path() else {
        return (None, None);
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    let mut lines = text.lines();
    let input = lines
        .next()
        .filter(|line| !line.is_empty())
        .map(str::to_string);
    let output = lines
        .next()
        .filter(|line| !line.is_empty())
        .map(str::to_string);
    (input, output)
}

fn save_audio_devices(input: &Option<String>, output: &Option<String>) {
    let Some(path) = audio_config_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let text = format!(
        "{}\n{}\n",
        input.as_deref().unwrap_or_default(),
        output.as_deref().unwrap_or_default()
    );
    if let Err(error) = std::fs::write(path, text) {
        tracing::warn!("could not save audio devices: {error}");
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
    Header {
        label: String,
        color: u32,
        count: usize,
    },
    Member {
        user_id: Id,
    },
}

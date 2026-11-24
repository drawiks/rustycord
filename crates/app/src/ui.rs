use std::time::{Duration, Instant};

use chrono::DateTime;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::{
    Focusable, FontWeight, ScrollHandle, Subscription, Window, div, prelude::*, px, rgb,
};
use rustycord_discord::{Channel, ConnectionState, Event, Id, Message, State, User, snowflake};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::{
    markdown,
    net::{Command, Net, UiEvent},
    theme::*,
};

const TYPING_COOLDOWN: Duration = Duration::from_secs(8);

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
    pending_token: Option<String>,
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
            pending_token: None,
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
                        self.mark_read(&channel_id);
                    }
                }
            }
            UiEvent::History {
                channel_id,
                messages,
            } => {
                let first_load = self.state.messages(&channel_id).is_empty();
                self.state.insert_history(&channel_id, messages);
                if Some(&channel_id) == self.selected_channel.as_ref() {
                    if first_load {
                        self.scroll.scroll_to_bottom();
                    }
                    self.mark_read(&channel_id);
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
        self.last_typing = None;
        if let Some(net) = &self.net {
            net.send(Command::LoadHistory {
                channel_id: channel_id.clone(),
                before: None,
            });
        }
        self.mark_read(&channel_id);
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn at_bottom(&self) -> bool {
        (self.scroll.max_offset().y - self.scroll.offset().y) < px(60.)
    }

    fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.composer.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    fn mark_read(&self, channel_id: &str) {
        let Some(message) = self.state.messages(channel_id).last() else {
            return;
        };
        if let Some(net) = &self.net {
            net.send(Command::Ack {
                channel_id: channel_id.to_string(),
                message_id: message.id.clone(),
            });
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

    fn start_edit(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
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

    fn render_login(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let connecting = matches!(self.screen, Screen::Connecting);
        let mut content = div()
            .w(px(440.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(28.))
            .rounded(px(12.))
            .bg(rgb(SIDEBAR))
            .child(
                div()
                    .text_size(px(26.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xf2f3f5))
                    .child("rustycord"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(MUTED))
                    .child("Sign in with your Discord account token."),
            )
            .child(
                Input::new(&self.token_input)
                    .appearance(true)
                    .bordered(true),
            )
            .child(self.render_primary_button("Login", connecting, cx))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(FAINT))
                    .child("Browser: DevTools (F12) -> Application -> Local Storage -> token"),
            );

        if let Some(error) = &self.error {
            content = content.child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(DANGER))
                    .child(error.clone()),
            );
        }

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(RAIL))
            .child(content)
    }

    fn render_primary_button(
        &self,
        label: &'static str,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let text = if busy { "Connecting..." } else { label };
        div()
            .id("primary-button")
            .flex()
            .justify_center()
            .py(px(10.))
            .rounded(px(6.))
            .bg(if busy { rgb(0x4752c4) } else { rgb(ACCENT) })
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(0xffffff))
            .cursor_pointer()
            .child(text)
            .on_click(cx.listener(|this, _, _, cx| {
                if matches!(this.screen, Screen::Login) {
                    this.login(cx);
                }
            }))
    }

    fn render_main(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .bg(rgb(RAIL))
            .child(self.render_rail(cx))
            .child(self.render_sidebar(cx))
            .child(self.render_chat(cx))
    }

    fn render_rail(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rail = div()
            .w(px(72.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .py(px(12.))
            .bg(rgb(RAIL));

        let dm_selected = self.selected_guild.is_none();
        rail = rail.child(
            div()
                .id("rail-dms")
                .size(px(48.))
                .rounded(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .bg(if dm_selected {
                    rgb(ACCENT)
                } else {
                    rgb(SIDEBAR)
                })
                .text_size(px(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xffffff))
                .cursor_pointer()
                .child("@")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.select_guild(None, cx);
                    this.focus_composer(window, cx);
                })),
        );

        let guilds: Vec<(String, String, Option<String>)> = self
            .state
            .guilds
            .iter()
            .map(|g| (g.id.clone(), g.name.clone(), g.icon.clone()))
            .collect();
        for (id, name, icon) in guilds {
            let selected = self.selected_guild.as_ref() == Some(&id);
            let element_id = gpui_kit::SharedString::from(format!("rail-{id}"));
            let click_id = id.clone();
            let visual = match icon {
                Some(icon) => gpui_kit::img(guild_icon_url(&id, &icon))
                    .size(px(48.))
                    .into_any_element(),
                None => div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xffffff))
                    .child(initials(&name))
                    .into_any_element(),
            };
            rail = rail.child(
                div()
                    .id(element_id)
                    .size(px(48.))
                    .rounded(px(16.))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(if selected { rgb(ACCENT) } else { rgb(SIDEBAR) })
                    .cursor_pointer()
                    .child(visual)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_guild(Some(click_id.clone()), cx);
                        this.focus_composer(window, cx);
                    })),
            );
        }

        rail
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match &self.selected_guild {
            Some(id) => self
                .state
                .guild(id)
                .map(|g| g.name.clone())
                .unwrap_or_default(),
            None => "Direct Messages".to_string(),
        };

        let channels: Vec<Channel> = match &self.selected_guild {
            Some(id) => self.state.text_channels(id).into_iter().cloned().collect(),
            None => {
                let mut channels: Vec<Channel> = self
                    .state
                    .channels
                    .iter()
                    .filter(|c| c.kind == Channel::DM || c.kind == Channel::GROUP_DM)
                    .cloned()
                    .collect();
                channels.sort_by_key(|c| {
                    std::cmp::Reverse(c.last_message_id.as_deref().map(snowflake).unwrap_or(0))
                });
                channels
            }
        };

        let has_channels = !channels.is_empty();
        let mut list = div().flex().flex_col().gap(px(1.)).p(px(8.));
        if !has_channels {
            let label = if self.selected_guild.is_some() {
                "No channels"
            } else {
                "No direct messages"
            };
            list = list.child(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(rgb(FAINT))
                    .child(label),
            );
        }
        for channel in channels {
            let selected = self.selected_channel.as_ref() == Some(&channel.id);
            let id = channel.id.clone();
            let unread = self
                .state
                .read_states
                .get(&channel.id)
                .map(|read| read.mention_count > 0)
                .unwrap_or(false);
            let mut row = div()
                .id(gpui_kit::SharedString::from(format!(
                    "channel-{}",
                    channel.id
                )))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.))
                .px(px(8.))
                .py(px(5.))
                .rounded(px(4.))
                .text_size(px(14.))
                .text_color(rgb(if selected { 0xffffff } else { MUTED }))
                .cursor_pointer()
                .bg(if selected { rgb(ACTIVE) } else { rgb(SIDEBAR) })
                .hover(|style| style.bg(rgb(HOVER)))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .child(format!("#{}", self.state.channel_display_name(&channel))),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_channel(id.clone(), cx);
                    this.focus_composer(window, cx);
                }));
            if unread {
                row = row.child(div().size(px(6.)).rounded(px(3.)).bg(rgb(DANGER)));
            }
            list = list.child(row);
        }

        div()
            .w(px(240.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(SIDEBAR))
            .child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .border_b(px(1.))
                    .border_color(rgb(RAIL))
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xf2f3f5))
                    .truncate()
                    .child(title),
            )
            .child(
                div()
                    .id("channels-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(list),
            )
    }

    fn render_top_bar(&self, title: Option<String>) -> impl IntoElement {
        let mut bar = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(16.))
            .py(px(12.))
            .border_b(px(1.))
            .border_color(rgb(RAIL))
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xf2f3f5))
                    .truncate()
                    .child(title.unwrap_or_else(|| "rustycord".to_string())),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(self.connection.clone()),
            );
        if let Some(error) = &self.error {
            bar = bar.child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(DANGER))
                    .child(error.clone()),
            );
        }
        bar
    }

    fn render_chat(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(channel_id) = self.selected_channel.clone() else {
            return div()
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .bg(rgb(CHAT))
                .child(self.render_top_bar(None))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(rgb(MUTED))
                                .child("Select a channel"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(FAINT))
                                .child(self.connection.clone()),
                        ),
                )
                .into_any_element();
        };

        let channel_name = self
            .state
            .channel(&channel_id)
            .map(|c| c.display_name())
            .unwrap_or_else(|| channel_id.clone());

        let messages: Vec<(Message, Vec<rustycord_discord::Reaction>)> = self
            .state
            .messages(&channel_id)
            .iter()
            .map(|m| (m.clone(), m.reactions.clone()))
            .collect();

        let typing = self
            .state
            .typing
            .get(&channel_id)
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(8))
            .map(|(user_id, _)| user_id.clone());

        let mut list = div()
            .id("messages")
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(16.))
            .py(px(16.));

        if self.state.messages(&channel_id).len() >= 50 {
            list = list.child(
                div()
                    .id("load-older")
                    .flex()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(rgb(LINK))
                    .cursor_pointer()
                    .child("Load older messages")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.load_older();
                        cx.notify();
                    })),
            );
        }
        if messages.is_empty() {
            list = list.child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(FAINT))
                    .child("No messages loaded"),
            );
        }
        for (message, reactions) in messages {
            list = list.child(self.render_message(message, reactions, cx));
        }

        let header = self.render_top_bar(Some(format!("#{channel_name}")));

        let status = match &typing {
            Some(user_id) => {
                let name = self
                    .state
                    .user(user_id)
                    .map(|user| user.display_name().to_string())
                    .unwrap_or_else(|| "Someone".to_string());
                format!("{name} is typing...")
            }
            None => String::new(),
        };

        let mut composer_block = div().px(px(16.)).pb(px(14.)).flex().flex_col().gap(px(6.));
        if let Some(message_id) = self.editing.as_ref().map(|(_, id)| id.clone()) {
            composer_block = composer_block.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .child(format!("Editing message {message_id}")),
                    )
                    .child(
                        div()
                            .id("cancel-edit")
                            .text_size(px(12.))
                            .text_color(rgb(LINK))
                            .cursor_pointer()
                            .child("Cancel")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.cancel_edit(window, cx)),
                            ),
                    ),
            );
        }
        composer_block =
            composer_block.child(Textarea::new(&self.composer).appearance(true).h(px(72.)));

        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(CHAT))
            .child(header)
            .child(list)
            .child(
                div()
                    .px(px(16.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(status),
            )
            .child(composer_block)
            .into_any_element()
    }

    fn render_message(
        &self,
        message: Message,
        reactions: Vec<rustycord_discord::Reaction>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let author = message.author.display_name().to_string();
        let time = format_time(&message.timestamp);
        let edited = message.edited_timestamp.is_some();

        let is_mine = self
            .state
            .me
            .as_ref()
            .is_some_and(|me| me.id == message.author.id);
        let content = if message.content.is_empty() {
            "(no content)".to_string()
        } else {
            message.content.clone()
        };

        let mut body = div()
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(rgb(TEXT))
            .child(markdown::build(&content, &self.state));

        for attachment in &message.attachments {
            if is_image(&attachment.content_type, &attachment.filename) {
                body = body.child(
                    div().mt(px(4.)).child(
                        gpui_kit::img(attachment.url.clone())
                            .max_w(px(420.))
                            .max_h(px(320.))
                            .rounded(px(4.)),
                    ),
                );
            } else {
                body = body.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(LINK))
                        .child(format!("[file] {}", attachment.filename)),
                );
            }
        }
        for embed in &message.embeds {
            if let Some(title) = &embed.title {
                body = body.child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(title.clone()),
                );
            }
            if let Some(description) = &embed.description {
                body = body.child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(MUTED))
                        .child(description.clone()),
                );
            }
            if let Some(image) = &embed.image {
                body = body.child(
                    div().mt(px(4.)).child(
                        gpui_kit::img(image.url.clone())
                            .max_w(px(420.))
                            .max_h(px(320.))
                            .rounded(px(4.)),
                    ),
                );
            }
            if let Some(thumbnail) = &embed.thumbnail {
                body = body.child(
                    div().mt(px(4.)).child(
                        gpui_kit::img(thumbnail.url.clone())
                            .max_w(px(80.))
                            .max_h(px(80.))
                            .rounded(px(4.)),
                    ),
                );
            }
        }

        if is_mine {
            let edit_message = message.clone();
            let delete_message = message.clone();
            body = body.child(
                div()
                    .flex()
                    .gap(px(10.))
                    .mt(px(2.))
                    .child(
                        div()
                            .id(gpui_kit::SharedString::from(format!("edit-{}", message.id)))
                            .text_size(px(11.))
                            .text_color(rgb(FAINT))
                            .cursor_pointer()
                            .hover(|style| style.text_color(rgb(TEXT)))
                            .child("edit")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_edit(edit_message.clone(), window, cx)
                            })),
                    )
                    .child(
                        div()
                            .id(gpui_kit::SharedString::from(format!(
                                "delete-{}",
                                message.id
                            )))
                            .text_size(px(11.))
                            .text_color(rgb(FAINT))
                            .cursor_pointer()
                            .hover(|style| style.text_color(rgb(DANGER)))
                            .child("delete")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.delete_message(&delete_message);
                                cx.notify();
                            })),
                    ),
            );
        }

        if !reactions.is_empty() {
            let message_for_react = message.clone();
            let mut chips = div().flex().gap(px(4.)).mt(px(4.));
            for reaction in reactions {
                let emoji = reaction.emoji.clone();
                let emoji_text = emoji.text();
                let message = message_for_react.clone();
                chips = chips.child(
                    div()
                        .id(gpui_kit::SharedString::from(format!(
                            "reaction-{}-{emoji_text}",
                            message.id
                        )))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(if reaction.me {
                            rgb(0x3c4270)
                        } else {
                            rgb(RAIL)
                        })
                        .text_size(px(12.))
                        .cursor_pointer()
                        .child(emoji_text)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.react(&message, &emoji);
                            cx.notify();
                        })),
                );
            }
            body = body.child(chips);
        }

        div()
            .flex()
            .gap(px(12.))
            .child(avatar(&message.author))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xf2f3f5))
                                    .child(author),
                            )
                            .child(div().text_size(px(11.)).text_color(rgb(FAINT)).child(
                                if edited {
                                    format!("{time} (edited)")
                                } else {
                                    time
                                },
                            )),
                    )
                    .child(body),
            )
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

fn save_token(token: &str) {
    match keyring::Entry::new("rustycord", "token").and_then(|entry| entry.set_password(token)) {
        Ok(()) => tracing::info!("token saved to keyring"),
        Err(error) => tracing::warn!("could not save token: {error}"),
    }
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .collect::<String>()
        .to_uppercase()
        .chars()
        .take(2)
        .collect()
}

fn avatar(user: &User) -> impl IntoElement {
    div()
        .size(px(40.))
        .flex_none()
        .rounded(px(20.))
        .overflow_hidden()
        .child(gpui_kit::img(avatar_url(user)).size(px(40.)))
}

fn avatar_url(user: &User) -> String {
    match &user.avatar {
        Some(hash) if hash.starts_with("a_") => format!(
            "https://cdn.discordapp.com/avatars/{}/{hash}.gif?size=64",
            user.id
        ),
        Some(hash) => format!(
            "https://cdn.discordapp.com/avatars/{}/{hash}.png?size=64",
            user.id
        ),
        None => {
            let index = user.discriminator.parse::<u32>().unwrap_or(0) % 5;
            format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
        }
    }
}

fn guild_icon_url(guild_id: &str, hash: &str) -> String {
    if hash.starts_with("a_") {
        format!("https://cdn.discordapp.com/icons/{guild_id}/{hash}.gif?size=96")
    } else {
        format!("https://cdn.discordapp.com/icons/{guild_id}/{hash}.png?size=96")
    }
}

fn is_image(content_type: &Option<String>, filename: &str) -> bool {
    if let Some(content_type) = content_type {
        return content_type.starts_with("image/");
    }
    let extension = filename.rsplit('.').next().unwrap_or_default();
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp"
    )
}

fn format_time(timestamp: &str) -> String {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

use super::widgets::*;
use super::*;

impl App {
    pub(super) fn render_chat(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(channel_id) = self.selected_channel.clone() else {
            return div()
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .bg(rgb(CHAT))
                .child(self.render_top_bar(None, cx))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .child(empty_state(
                            IconName::MessageSquare,
                            "Select a channel",
                            "Pick a channel from the sidebar to start reading",
                        ))
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
            .map(|channel| self.state.channel_display_name(channel))
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
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll(
                cx.listener(|this, _event: &gpui_kit::ScrollEvent, _window, cx| {
                    this.maybe_load_older();
                    cx.notify();
                }),
            )
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(10.))
            .py(px(16.));

        let mut child_index = 0usize;
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
            child_index += 1;
        }
        if messages.is_empty() {
            list = list.child(empty_state(
                IconName::MessageSquare,
                "No messages yet",
                "Start the conversation",
            ));
            child_index += 1;
        }
        let read_marker = self.unread_marker.clone();
        let mut previous: Option<(Id, DateTime<Local>)> = None;
        let mut last_date: Option<NaiveDate> = None;
        let mut unread_shown = false;
        let mut first = true;

        for (message, reactions) in messages {
            let timestamp = parse_timestamp(&message.timestamp);
            let date = timestamp.map(|time| time.date_naive());
            if let Some(date) = date
                && last_date != Some(date)
            {
                list = list.child(divider(&day_label(date), FAINT));
                last_date = Some(date);
                child_index += 1;
            }
            if !unread_shown
                && !first
                && let Some(marker) = &read_marker
                && snowflake(&message.id) > snowflake(marker)
            {
                list = list.child(divider("NEW", DANGER));
                unread_shown = true;
                child_index += 1;
            }
            let compact = match (&previous, timestamp) {
                (Some((author, at)), Some(now)) => {
                    author == &message.author.id
                        && now.signed_duration_since(*at) < chrono::Duration::minutes(5)
                }
                _ => false,
            };
            if self.pending_anchor.as_ref() == Some(&message.id) {
                self.scroll.scroll_to_top_of_item(child_index);
                self.pending_anchor = None;
            }
            previous = timestamp.map(|time| (message.author.id.clone(), time));
            first = false;
            child_index += 1;
            list = list.child(self.render_message(message, reactions, compact, cx));
        }

        let mut list_container = div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(list);
        if !self.at_bottom() {
            list_container = list_container.child(
                div()
                    .id("jump-bottom")
                    .absolute()
                    .bottom(px(14.))
                    .right(px(24.))
                    .size(px(36.))
                    .rounded(px(18.))
                    .bg(rgb(SIDEBAR))
                    .border_1()
                    .border_color(rgb(RAIL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(HOVER)))
                    .child(gpui_kit::component::Icon::new(IconName::ChevronDown).size(px(18.)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.scroll.scroll_to_bottom();
                        cx.notify();
                    })),
            );
        }

        let header = self.render_top_bar(Some(channel_name.clone()), cx);

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
        if let Some(reply) = &self.replying {
            let name = reply.author.display_name().to_string();
            let preview = snippet(&reply.content, 60);
            composer_block = composer_block.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(4.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(format!("Replying to {name}: {preview}")),
                    )
                    .child(
                        div()
                            .id("cancel-reply")
                            .text_size(px(12.))
                            .text_color(rgb(LINK))
                            .cursor_pointer()
                            .child("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_reply(cx))),
                    ),
            );
        }
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
        let draft = self.composer.read(cx).value().to_string();
        let count = draft.chars().count();
        let status_color = if count > 2000 { DANGER } else { MUTED };
        if count > 1800 {
            composer_block = composer_block.child(
                div()
                    .self_end()
                    .text_size(px(11.))
                    .text_color(rgb(status_color))
                    .child(format!("{count}/2000")),
            );
        }
        composer_block = composer_block.child(
            div()
                .flex()
                .items_end()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .rounded(px(8.))
                        .bg(rgb(0x383a40))
                        .px(px(12.))
                        .py(px(4.))
                        .child(
                            Textarea::new(&self.composer)
                                .appearance(false)
                                .bordered(false)
                                .h(px(40.)),
                        ),
                )
                .child(
                    div()
                        .id("send-message")
                        .size(px(40.))
                        .rounded(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(if count > 0 { rgb(ACCENT) } else { rgb(SIDEBAR) })
                        .text_color(rgb(if count > 0 { 0xffffff } else { FAINT }))
                        .cursor_pointer()
                        .child(gpui_kit::component::Icon::new(IconName::Send).size(px(18.)))
                        .on_click(cx.listener(|this, _, window, cx| this.send_message(window, cx))),
                ),
        );

        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(CHAT))
            .child(header)
            .child(list_container)
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

    fn render_top_bar(&self, title: Option<String>, cx: &Context<Self>) -> impl IntoElement {
        let channel = self
            .selected_channel
            .as_ref()
            .and_then(|id| self.state.channel(id));
        let is_dm = channel.is_some_and(|channel| {
            channel.kind == Channel::DM || channel.kind == Channel::GROUP_DM
        });
        let icon = if is_dm {
            IconName::AtSign
        } else {
            IconName::Hash
        };
        let topic = channel.and_then(|channel| channel.topic.clone());
        let status = if self.connection.starts_with("connected") {
            "online"
        } else if self.connection.starts_with("reconnecting") || self.connection == "connecting" {
            "idle"
        } else {
            "offline"
        };

        let mut bar = div()
            .h(px(48.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .border_b(px(1.))
            .border_color(rgb(RAIL))
            .child(
                gpui_kit::component::Icon::new(icon)
                    .size(px(20.))
                    .text_color(rgb(ICON)),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(TEXT_BRIGHT))
                    .child(title.unwrap_or_default()),
            );

        if let Some(topic) = topic {
            bar = bar.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(topic),
            );
        } else {
            bar = bar.child(div().flex_1());
        }

        bar = bar.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(presence_dot(status))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .child(self.connection.clone()),
                ),
        );

        if let Some(error) = &self.error {
            let text = error.clone();
            bar = bar.child(
                div()
                    .id("dismiss-error")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .bg(rgb(DANGER))
                    .cursor_pointer()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(0xffffff))
                            .child(text),
                    )
                    .child(
                        gpui_kit::component::Icon::new(IconName::X)
                            .size(px(12.))
                            .text_color(rgb(0xffffff)),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.error = None;
                        cx.notify();
                    })),
            );
        }

        bar
    }

    fn render_message(
        &self,
        message: Message,
        reactions: Vec<rustycord_discord::Reaction>,
        compact: bool,
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

        let mut content = div()
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(rgb(TEXT));

        if let Some(reference) = &message.referenced_message {
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .mb(px(2.))
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(div().text_color(rgb(FAINT)).child("↪"))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(reference.author.display_name().to_string()),
                    )
                    .child(div().truncate().child(snippet(&reference.content, 90))),
            );
        } else if message.message_reference.is_some() {
            content = content.child(
                div()
                    .mb(px(2.))
                    .text_size(px(12.))
                    .text_color(rgb(FAINT))
                    .child("↪ replied to a message"),
            );
        }

        let text = if message.content.is_empty() {
            "(no content)".to_string()
        } else {
            message.content.clone()
        };
        let rendered = markdown::build(&text, &self.state);
        let targets = rendered.targets;
        let click_ranges: Vec<std::ops::Range<usize>> =
            targets.iter().map(|(range, _)| range.clone()).collect();
        let entity = cx.entity().downgrade();
        content = content.child(
            gpui_kit::InteractiveText::new(
                gpui_kit::SharedString::from(format!("content-{}", message.id)),
                rendered.text,
            )
            .on_click(click_ranges, move |index, window, cx| {
                let Some((_, target)) = targets.get(index) else {
                    return;
                };
                match target {
                    markdown::Target::Url(url) => {
                        if let Err(error) = open::that(url) {
                            tracing::warn!("failed to open url: {error}");
                        }
                    }
                    other => {
                        let _ = entity
                            .update(cx, |this, cx| this.open_target(other.clone(), window, cx));
                    }
                }
            }),
        );

        for attachment in &message.attachments {
            if is_image(&attachment.content_type, &attachment.filename) {
                let url = attachment.url.clone();
                let click_url = url.clone();
                let (image_w, image_h) = fit_size(attachment.width, attachment.height, 420., 320.);
                content = content.child(
                    div()
                        .id(gpui_kit::SharedString::from(format!(
                            "attachment-{}",
                            attachment.id
                        )))
                        .mt(px(4.))
                        .cursor_pointer()
                        .child(
                            gpui_kit::img(url)
                                .w(px(image_w))
                                .h(px(image_h))
                                .object_fit(gpui_kit::ObjectFit::Contain)
                                .rounded(px(4.)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.image_viewer = Some(click_url.clone());
                            cx.notify();
                        })),
                );
            } else {
                content = content.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(LINK))
                        .child(format!("[file] {}", attachment.filename)),
                );
            }
        }
        for embed in &message.embeds {
            if let Some(title) = &embed.title {
                content = content.child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(title.clone()),
                );
            }
            if let Some(description) = &embed.description {
                content = content.child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(MUTED))
                        .child(description.clone()),
                );
            }
            if let Some(image) = &embed.image {
                let url = image.url.clone();
                let click_url = url.clone();
                let (image_w, image_h) = fit_size(image.width, image.height, 420., 320.);
                content = content.child(
                    div()
                        .id(gpui_kit::SharedString::from(format!(
                            "embed-image-{}",
                            message.id
                        )))
                        .mt(px(4.))
                        .cursor_pointer()
                        .child(
                            gpui_kit::img(url)
                                .w(px(image_w))
                                .h(px(image_h))
                                .object_fit(gpui_kit::ObjectFit::Contain)
                                .rounded(px(4.)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.image_viewer = Some(click_url.clone());
                            cx.notify();
                        })),
                );
            }
            if let Some(thumbnail) = &embed.thumbnail {
                let (image_w, image_h) = fit_size(thumbnail.width, thumbnail.height, 80., 80.);
                content = content.child(
                    div().mt(px(4.)).child(
                        gpui_kit::img(thumbnail.url.clone())
                            .w(px(image_w))
                            .h(px(image_h))
                            .object_fit(gpui_kit::ObjectFit::Contain)
                            .rounded(px(4.)),
                    ),
                );
            }
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
                        .child(format!("{emoji_text} {}", reaction.count))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.react(&message, &emoji);
                            cx.notify();
                        })),
                );
            }
            content = content.child(chips);
        }

        let mut actions = div()
            .id(gpui_kit::SharedString::from(format!(
                "actions-{}",
                message.id
            )))
            .absolute()
            .top(px(-10.))
            .right(px(10.))
            .flex()
            .gap(px(1.))
            .px(px(3.))
            .py(px(2.))
            .rounded(px(6.))
            .bg(rgb(SIDEBAR))
            .border_1()
            .border_color(rgb(RAIL))
            .opacity(0.)
            .group_hover("message-row", |style| style.opacity(1.));

        let reply_target = message.clone();
        actions = actions.child(self.action_button(
            gpui_kit::SharedString::from(format!("reply-{}", message.id)),
            IconName::Reply,
            move |this, window, cx| this.start_reply(reply_target.clone(), window, cx),
            cx,
        ));

        let react_target = message.clone();
        actions = actions.child(self.action_button(
            gpui_kit::SharedString::from(format!("react-{}", message.id)),
            IconName::ThumbsUp,
            move |this, _window, cx| {
                let emoji = rustycord_discord::Emoji {
                    id: None,
                    name: Some("👍".to_string()),
                };
                this.react(&react_target, &emoji);
                cx.notify();
            },
            cx,
        ));

        let copy_text = message.content.clone();
        actions = actions.child(self.action_button(
            gpui_kit::SharedString::from(format!("copy-{}", message.id)),
            IconName::Copy,
            move |_this, _window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
            },
            cx,
        ));

        let copy_id = message.id.clone();
        actions = actions.child(self.action_button(
            gpui_kit::SharedString::from(format!("copy-id-{}", message.id)),
            IconName::Hash,
            move |_this, _window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_id.clone()));
            },
            cx,
        ));

        if is_mine {
            let edit_target = message.clone();
            actions = actions.child(self.action_button(
                gpui_kit::SharedString::from(format!("edit-{}", message.id)),
                IconName::Pencil,
                move |this, window, cx| this.start_edit(edit_target.clone(), window, cx),
                cx,
            ));

            let delete_target = message.clone();
            actions = actions.child(self.action_button(
                gpui_kit::SharedString::from(format!("delete-{}", message.id)),
                IconName::Trash,
                move |this, _window, cx| {
                    this.delete_message(&delete_target);
                    cx.notify();
                },
                cx,
            ));
        }

        let mut header = div().flex().items_baseline().gap(px(8.)).child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0xf2f3f5))
                .child(author),
        );
        header = header.child(
            div()
                .text_size(px(11.))
                .text_color(rgb(FAINT))
                .child(if edited {
                    format!("{time} (edited)")
                } else {
                    time.clone()
                }),
        );

        let leading = if compact {
            let gutter_time = time.clone();
            div()
                .id(gpui_kit::SharedString::from(format!("time-{}", message.id)))
                .w(px(40.))
                .flex_none()
                .flex()
                .justify_end()
                .text_size(px(10.))
                .text_color(rgb(FAINT))
                .opacity(0.)
                .group_hover("message-row", |style| style.opacity(1.))
                .child(gutter_time)
                .into_any_element()
        } else {
            avatar(&message.author).into_any_element()
        };

        let mut column = div().flex().flex_col().flex_1().min_w(px(0.));
        if !compact {
            column = column.child(header);
        }
        column = column.child(content);

        div()
            .group("message-row")
            .relative()
            .flex()
            .gap(px(12.))
            .px(px(10.))
            .py(px(3.))
            .rounded(px(4.))
            .hover(|style| style.bg(rgb(0x2e3035)))
            .child(actions)
            .child(leading)
            .child(column)
    }

    fn action_button(
        &self,
        id: gpui_kit::SharedString,
        icon: IconName,
        handler: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let listener = std::rc::Rc::new(handler);
        div()
            .id(id)
            .size(px(26.))
            .rounded(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(MUTED))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(HOVER)).text_color(rgb(TEXT)))
            .child(gpui_kit::component::Icon::new(icon).size(px(15.)))
            .on_click(cx.listener(move |this, _, window, cx| listener(this, window, cx)))
    }
}

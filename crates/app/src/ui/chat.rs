use super::widgets::*;
use super::*;
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::{AnyElement, ElementId, StyleRefinement, WeakEntity};

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
                            "Выберите канал",
                            "Выберите канал в списке слева, чтобы начать читать",
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
        let header = self.render_top_bar(Some(channel_name), cx);

        let typing = self
            .state
            .typing
            .get(&channel_id)
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(8))
            .map(|(user_id, _)| user_id.clone());
        let status = if self.transfers > 0 {
            format!("Передача файлов: {}", self.transfers)
        } else if let Some(notice) = &self.notice {
            notice.clone()
        } else {
            match &typing {
                Some(user_id) => {
                    let name = self
                        .state
                        .user(user_id)
                        .map(|user| user.display_name().to_string())
                        .unwrap_or_else(|| "Кто-то".to_string());
                    format!("{name} печатает…")
                }
                None => String::new(),
            }
        };

        let debug = debug_render();
        let render_started = Instant::now();
        let weak = cx.entity().downgrade();
        let scroller = MessageScroller::new(
            "transcript",
            self.transcript.clone(),
            move |index, _window, cx| {
                let row_started = Instant::now();
                let element = match weak.upgrade() {
                    Some(entity) => entity.read(cx).render_row(index, &weak),
                    None => div().into_any_element(),
                };
                if debug {
                    let elapsed = row_started.elapsed();
                    if elapsed > Duration::from_millis(1) {
                        tracing::warn!("render_row {index}: {elapsed:?}");
                    }
                }
                element
            },
        )
        .scrollbar(true)
        .jump_button(true)
        .with_bottom_fade(rgb(CHAT))
        .with_row_style(
            StyleRefinement::default()
                .pt(px(0.))
                .pb(px(0.))
                .pl(px(0.))
                .pr(px(0.)),
        )
        .with_list_style(
            StyleRefinement::default()
                .pt(px(12.))
                .pb(px(6.))
                .pl(px(0.))
                .pr(px(0.)),
        );

        let composer = self.render_composer(cx);
        let drop_entity = cx.entity().downgrade();

        if debug {
            tracing::info!(
                "render_chat: {:?}, rows={}",
                render_started.elapsed(),
                self.rows.len()
            );
        }

        div()
            .id("chat")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(CHAT))
            .on_drop::<gpui_kit::ExternalPaths>(move |paths, _window, cx| {
                for path in paths.paths() {
                    let path = path.clone();
                    let _ = drop_entity.update(cx, |app, cx| app.enqueue_upload(path, cx));
                }
            })
            .child(header)
            .child(div().flex_1().min_h(px(0.)).child(scroller))
            .child(
                div()
                    .px(px(16.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(status),
            )
            .child(composer)
            .into_any_element()
    }

    fn render_row(&self, index: usize, entity: &WeakEntity<App>) -> AnyElement {
        let Some(row) = self.rows.get(index) else {
            return div().into_any_element();
        };
        match row {
            Row::LoadOlder => {
                let weak = entity.clone();
                div()
                    .flex()
                    .justify_center()
                    .py(px(6.))
                    .child(
                        div()
                            .id("load-older")
                            .px(px(10.))
                            .py(px(4.))
                            .rounded(px(10.))
                            .bg(rgb(SIDEBAR))
                            .text_size(px(12.))
                            .text_color(rgb(LINK))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(RAIL_HOVER)))
                            .child("Загрузить историю")
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.load_older();
                                    cx.notify();
                                });
                            }),
                    )
                    .into_any_element()
            }
            Row::Date(label) => divider(label, FAINT).into_any_element(),
            Row::Unread => divider("НОВОЕ", DANGER).into_any_element(),
            Row::Message { id, compact } => {
                let channel = self.rows_channel.as_deref().unwrap_or_default();
                match self.state.message(channel, id) {
                    Some(message) => self.render_message(message, *compact, entity),
                    None => div().into_any_element(),
                }
            }
        }
    }

    fn render_message(
        &self,
        message: &Message,
        compact: bool,
        entity: &WeakEntity<App>,
    ) -> AnyElement {
        let (author, author_color) = match &message.guild_id {
            Some(guild_id) => (
                self.state.member_display_name(guild_id, &message.author),
                self.state.member_role_color(guild_id, &message.author.id),
            ),
            None => (message.author.display_name().to_string(), 0),
        };
        let author_color = if author_color == 0 {
            0xf2f3f5
        } else {
            author_color
        };
        let time = format_timestamp(&message.timestamp);
        let edited = message.edited_timestamp.is_some();
        let is_mine = self
            .state
            .me
            .as_ref()
            .is_some_and(|me| me.id == message.author.id);
        let message_id = message.id.clone();
        let snowflake_id = snowflake(&message.id) as usize;

        let mut content = div()
            .text_size(px(16.))
            .line_height(px(22.))
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

        if !message.content.is_empty() {
            let temporary;
            let layout = match self.text_cache.get(&message.id) {
                Some(layout) => layout,
                None => {
                    temporary = markdown::layout(&message.content, &self.state);
                    &temporary
                }
            };
            let targets = layout.targets.clone();
            let ranges: Vec<std::ops::Range<usize>> =
                targets.iter().map(|(range, _)| range.clone()).collect();
            let weak = entity.clone();
            content = content.child(
                gpui_kit::InteractiveText::new(
                    ElementId::named_usize("content", snowflake_id),
                    layout.styled(),
                )
                .on_click(ranges, move |index, window, cx| {
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
                            let _ = weak
                                .update(cx, |app, cx| app.open_target(other.clone(), window, cx));
                        }
                    }
                }),
            );
        }

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
                        .on_click({
                            let weak = entity.clone();
                            move |_, _, cx| {
                                let url = click_url.clone();
                                let _ = weak.update(cx, |app, cx| {
                                    app.image_viewer = Some(url);
                                    cx.notify();
                                });
                            }
                        }),
                );
            } else {
                let file_name = attachment.filename.clone();
                let file_url = attachment.url.clone();
                let weak = entity.clone();
                let download_name = file_name.clone();
                content = content.child(
                    div()
                        .mt(px(4.))
                        .max_w(px(420.))
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(6.))
                        .bg(rgb(SIDEBAR))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child({
                            let extension = attachment
                                .filename
                                .rsplit('.')
                                .next()
                                .unwrap_or("")
                                .to_uppercase();
                            let label: String =
                                if extension.is_empty() || extension == attachment.filename {
                                    "FILE".to_string()
                                } else {
                                    extension.chars().take(4).collect()
                                };
                            div()
                                .size(px(40.))
                                .rounded(px(6.))
                                .bg(rgb(RAIL))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(10.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(LINK))
                                .child(label)
                        })
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(px(0.))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(rgb(LINK))
                                        .truncate()
                                        .child(file_name),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(FAINT))
                                        .child(format_bytes(attachment.size)),
                                ),
                        )
                        .child(
                            div()
                                .id(gpui_kit::SharedString::from(format!(
                                    "download-{}",
                                    attachment.id
                                )))
                                .size(px(28.))
                                .rounded(px(4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(rgb(MUTED))
                                .cursor_pointer()
                                .hover(|style| {
                                    style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT))
                                })
                                .child(
                                    gpui_kit::component::Icon::new(IconName::Download)
                                        .size(px(16.)),
                                )
                                .on_click(move |_, window, cx| {
                                    let _ = weak.update(cx, |app, cx| {
                                        app.pick_download(
                                            file_url.clone(),
                                            download_name.clone(),
                                            cx,
                                        )
                                    });
                                    let _ = window;
                                }),
                        ),
                );
            }
        }

        for embed in &message.embeds {
            let embed_color = embed.color.unwrap_or(ACCENT);
            let mut card = div()
                .mt(px(4.))
                .max_w(px(520.))
                .rounded(px(6.))
                .bg(rgb(SIDEBAR))
                .border_l(px(3.))
                .border_color(rgb(embed_color))
                .px(px(12.))
                .py(px(8.))
                .flex()
                .flex_col()
                .gap(px(4.));

            if let Some(author) = &embed.author {
                card = card.child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(author.name.clone()),
                );
            }
            if let Some(title) = &embed.title {
                card = card.child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(LINK))
                        .child(title.clone()),
                );
            }
            if let Some(description) = &embed.description {
                card = card.child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(TEXT))
                        .child(description.clone()),
                );
            }
            if !embed.fields.is_empty() {
                let fields: Vec<AnyElement> = embed
                    .fields
                    .iter()
                    .map(|field| {
                        div()
                            .flex()
                            .flex_col()
                            .min_w(px(120.))
                            .max_w(px(240.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .child(field.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(MUTED))
                                    .child(field.value.clone()),
                            )
                            .into_any_element()
                    })
                    .collect();
                card = card.child(div().flex().flex_wrap().gap(px(8.)).children(fields));
            }
            if let Some(image) = &embed.image {
                let url = image.url.clone();
                let click_url = url.clone();
                let (image_w, image_h) = fit_size(image.width, image.height, 420., 320.);
                card = card.child(
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
                        .on_click({
                            let weak = entity.clone();
                            move |_, _, cx| {
                                let url = click_url.clone();
                                let _ = weak.update(cx, |app, cx| {
                                    app.image_viewer = Some(url);
                                    cx.notify();
                                });
                            }
                        }),
                );
            }
            if let Some(thumbnail) = &embed.thumbnail {
                let (image_w, image_h) = fit_size(thumbnail.width, thumbnail.height, 80., 80.);
                card = card.child(
                    div().mt(px(4.)).child(
                        gpui_kit::img(thumbnail.url.clone())
                            .w(px(image_w))
                            .h(px(image_h))
                            .object_fit(gpui_kit::ObjectFit::Contain)
                            .rounded(px(4.)),
                    ),
                );
            }
            if let Some(footer) = &embed.footer {
                card = card.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(FAINT))
                        .child(footer.text.clone()),
                );
            }

            content = content.child(card);
        }

        if !message.reactions.is_empty() {
            let mut chips = div().flex().gap(px(4.)).mt(px(4.));
            for reaction in &message.reactions {
                let emoji = reaction.emoji.clone();
                let emoji_text = emoji.text();
                let reactions_id = message_id.clone();
                let weak = entity.clone();
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
                        .on_click(move |_, _, cx| {
                            let emoji = emoji.clone();
                            let id = reactions_id.clone();
                            let _ = weak.update(cx, |app, cx| {
                                app.toggle_reaction(&id, &emoji, cx);
                            });
                        }),
                );
            }
            content = content.child(chips);
        }

        let mut actions = div()
            .id(ElementId::named_usize("actions", snowflake_id))
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

        let reply_id = message_id.clone();
        actions = actions.child(self.action_button(
            ElementId::named_usize("reply", snowflake_id),
            IconName::Reply,
            entity.clone(),
            move |app, window, cx| app.start_reply_by_id(&reply_id, window, cx),
        ));

        let react_id = message_id.clone();
        actions = actions.child(self.action_button(
            ElementId::named_usize("react", snowflake_id),
            IconName::ThumbsUp,
            entity.clone(),
            move |app, _window, cx| {
                let emoji = rustycord_discord::Emoji {
                    id: None,
                    name: Some("👍".to_string()),
                };
                app.toggle_reaction(&react_id, &emoji, cx);
            },
        ));

        let copy_id = message_id.clone();
        actions = actions.child(self.action_button(
            ElementId::named_usize("copy", snowflake_id),
            IconName::Copy,
            entity.clone(),
            move |app, _window, cx| app.copy_message_text(&copy_id, cx),
        ));

        let raw_id = message_id.clone();
        actions = actions.child(self.action_button(
            ElementId::named_usize("copy-id", snowflake_id),
            IconName::Hash,
            entity.clone(),
            move |_app, _window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(raw_id.clone()));
            },
        ));

        if is_mine {
            let edit_id = message_id.clone();
            actions = actions.child(self.action_button(
                ElementId::named_usize("edit", snowflake_id),
                IconName::Pencil,
                entity.clone(),
                move |app, window, cx| app.start_edit_by_id(&edit_id, window, cx),
            ));

            let delete_id = message_id.clone();
            actions = actions.child(self.action_button(
                ElementId::named_usize("delete", snowflake_id),
                IconName::Trash,
                entity.clone(),
                move |app, _window, cx| app.delete_by_id(&delete_id, cx),
            ));
        }

        let role_badge: Option<AnyElement> = message.guild_id.as_ref().and_then(|guild_id| {
            self.state
                .member_top_role(guild_id, &message.author.id)
                .and_then(|role| match (&role.icon, &role.unicode_emoji) {
                    (Some(icon), _) => Some(
                        gpui_kit::img(role_icon_url(&role.id, icon))
                            .size(px(14.))
                            .rounded(px(3.))
                            .into_any_element(),
                    ),
                    (_, Some(emoji)) => Some(
                        div()
                            .text_size(px(13.))
                            .child(emoji.clone())
                            .into_any_element(),
                    ),
                    _ => None,
                })
        });

        let mut header = div().flex().items_baseline().gap(px(8.)).child({
            let weak = entity.clone();
            let user_id = message.author.id.clone();
            let guild_id = message.guild_id.clone();
            div()
                .id(ElementId::named_usize("author-name", snowflake_id))
                .cursor_pointer()
                .hover(|style| style.underline())
                .text_size(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(author_color))
                .child(author)
                .children(role_badge)
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.profile_popup = Some((user_id.clone(), guild_id.clone()));
                        cx.notify();
                    });
                })
        });
        header = header.child(
            div()
                .text_size(px(12.))
                .text_color(rgb(FAINT))
                .child(if edited {
                    format!("{time} (изменено)")
                } else {
                    time.clone()
                }),
        );

        let leading = if compact {
            div()
                .id(ElementId::named_usize("time", snowflake_id))
                .w(px(40.))
                .flex_none()
                .flex()
                .justify_end()
                .text_size(px(10.))
                .text_color(rgb(FAINT))
                .opacity(0.)
                .group_hover("message-row", |style| style.opacity(1.))
                .child(time)
                .into_any_element()
        } else {
            let weak = entity.clone();
            let user_id = message.author.id.clone();
            let guild_id = message.guild_id.clone();
            div()
                .id(ElementId::named_usize("avatar", snowflake_id))
                .cursor_pointer()
                .child(avatar(&message.author))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.profile_popup = Some((user_id.clone(), guild_id.clone()));
                        cx.notify();
                    });
                })
                .into_any_element()
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
            .pt(if compact { px(2.) } else { px(16.) })
            .pb(px(2.))
            .rounded(px(4.))
            .hover(|style| style.bg(rgb(0x2e3035)))
            .child(actions)
            .child(leading)
            .child(column)
            .into_any_element()
    }

    fn action_button(
        &self,
        id: ElementId,
        icon: IconName,
        entity: WeakEntity<App>,
        handler: impl Fn(&mut App, &mut Window, &mut Context<App>) + 'static,
    ) -> impl IntoElement {
        let handler = std::rc::Rc::new(handler);
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
            .on_click(move |_, window, cx| {
                let _ = entity.update(cx, |app, cx| handler(app, window, cx));
            })
    }

    fn render_top_bar(&self, title: Option<String>, cx: &Context<Self>) -> impl IntoElement {
        let channel = self
            .selected_channel
            .as_ref()
            .and_then(|id| self.state.channel(id));
        let is_dm = channel.is_some_and(|channel| {
            channel.kind == Channel::DM || channel.kind == Channel::GROUP_DM
        });
        let dm_user = if is_dm {
            channel.and_then(|channel| {
                let me = self.state.me.as_ref().map(|user| user.id.as_str());
                channel
                    .recipients
                    .iter()
                    .find(|user| Some(user.id.as_str()) != me)
                    .or_else(|| {
                        channel
                            .recipient_ids
                            .iter()
                            .find(|id| Some(id.as_str()) != me)
                            .and_then(|id| self.state.user(id))
                    })
            })
        } else {
            None
        };
        let topic = channel.and_then(|channel| channel.topic.clone());
        let status = if self.connection.starts_with("подключено") {
            "online"
        } else if self.connection.starts_with("переподключение")
            || self.connection.starts_with("подключение")
        {
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
            .child(match dm_user {
                Some(user) => mini_avatar(
                    Some(user),
                    user.display_name(),
                    self.state.presence(&user.id),
                )
                .into_any_element(),
                None => gpui_kit::component::Icon::new(IconName::Hash)
                    .size(px(20.))
                    .text_color(rgb(ICON))
                    .into_any_element(),
            })
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(TEXT_BRIGHT))
                    .child(title.unwrap_or_default()),
            )
            .children(dm_user.map(|user| {
                div()
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(status_text(self.state.presence(&user.id)))
            }));

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

        if is_dm && let Some(channel) = channel {
            let channel_id = channel.id.clone();
            let in_call = self
                .voice
                .as_ref()
                .is_some_and(|voice| voice.channel_id == channel_id);
            let call_channel = channel_id.clone();
            bar = bar.child(
                div()
                    .id("dm-call")
                    .size(px(28.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(if in_call { 0x23a55a } else { MUTED }))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
                    .child(
                        gpui_kit::component::Icon::new(if in_call {
                            IconName::PhoneOff
                        } else {
                            IconName::Phone
                        })
                        .size(px(17.)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this
                            .voice
                            .as_ref()
                            .is_some_and(|voice| voice.channel_id == call_channel)
                        {
                            this.leave_voice(cx);
                        } else {
                            this.start_call(call_channel.clone(), cx);
                        }
                    })),
            );
        }

        if let Some(user) = dm_user {
            let user_id = user.id.clone();
            bar = bar.child(
                div()
                    .id("dm-profile")
                    .size(px(28.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
                    .child(gpui_kit::component::Icon::new(IconName::UserRound).size(px(17.)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.right_pane = Some(RightPane::Profile {
                            user_id: user_id.clone(),
                            guild_id: None,
                        });
                        cx.notify();
                    })),
            );
        }

        if self.selected_guild.is_some() {
            let open = matches!(self.right_pane, Some(RightPane::Members));
            bar = bar.child(
                div()
                    .id("toggle-members")
                    .size(px(28.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(if open { 0xffffff } else { MUTED }))
                    .bg(if open { rgb(ACTIVE) } else { rgb(CHAT) })
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
                    .child(gpui_kit::component::Icon::new(IconName::Users).size(px(17.)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_members(cx))),
            );
        }

        bar = bar.child(self.header_button(
            "header-threads",
            IconName::ListFilter,
            "Список веток пока не поддерживается",
            cx,
        ));
        bar = bar.child(self.header_button(
            "header-bell",
            IconName::Bell,
            "Уведомления пока не поддерживаются",
            cx,
        ));
        bar = bar.child(self.header_button(
            "header-pin",
            IconName::Pin,
            "Закреплённые сообщения пока не поддерживаются",
            cx,
        ));

        bar = bar.child(
            div()
                .id("search-box")
                .w(px(220.))
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(4.))
                .bg(rgb(RAIL))
                .child(
                    gpui_kit::component::Icon::new(IconName::Search)
                        .size(px(15.))
                        .text_color(rgb(MUTED)),
                )
                .child(
                    div().flex_1().min_w(px(0.)).child(
                        Input::new(&self.search_input)
                            .appearance(false)
                            .bordered(false),
                    ),
                ),
        );

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

    fn composer_action(
        &self,
        id: &'static str,
        icon: AnyElement,
        notice: &'static str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .size(px(32.))
            .rounded(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(MUTED))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
            .child(icon)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.notice = Some(notice.to_string());
                cx.notify();
            }))
    }

    fn header_button(
        &self,
        id: &'static str,
        icon: IconName,
        notice: &'static str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .size(px(28.))
            .rounded(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(MUTED))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
            .child(gpui_kit::component::Icon::new(icon).size(px(18.)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.notice = Some(notice.to_string());
                cx.notify();
            }))
    }

    fn render_composer(&self, cx: &Context<Self>) -> impl IntoElement {
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
                            .child(format!("Ответ {name}: {preview}")),
                    )
                    .child(
                        div()
                            .id("cancel-reply")
                            .text_size(px(12.))
                            .text_color(rgb(LINK))
                            .cursor_pointer()
                            .child("Отмена")
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
                            .child(format!("Редактирование {message_id}")),
                    )
                    .child(
                        div()
                            .id("cancel-edit")
                            .text_size(px(12.))
                            .text_color(rgb(LINK))
                            .cursor_pointer()
                            .child("Отмена")
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

        composer_block.child(
            div()
                .flex()
                .items_end()
                .gap(px(8.))
                .child(
                    div()
                        .id("attach-file")
                        .size(px(36.))
                        .rounded(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(MUTED))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
                        .child(gpui_kit::component::Icon::new(IconName::Paperclip).size(px(18.)))
                        .on_click(cx.listener(|this, _, _window, cx| this.pick_files(cx))),
                )
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
                    self.composer_action(
                        "composer-gift",
                        gpui_kit::component::Icon::new(IconName::Gift)
                            .size(px(20.))
                            .into_any_element(),
                        "Подарки пока не поддерживаются",
                        cx,
                    ),
                )
                .child(
                    self.composer_action(
                        "composer-gif",
                        div()
                            .px(px(4.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(rgb(MUTED))
                            .text_size(px(9.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(MUTED))
                            .child("GIF")
                            .into_any_element(),
                        "GIF пока не поддерживаются",
                        cx,
                    ),
                )
                .child(
                    self.composer_action(
                        "composer-sticker",
                        gpui_kit::component::Icon::new(IconName::Sticker)
                            .size(px(20.))
                            .into_any_element(),
                        "Стикеры пока не поддерживаются",
                        cx,
                    ),
                )
                .child(
                    self.composer_action(
                        "composer-emoji",
                        gpui_kit::component::Icon::default()
                            .path("icons/smile.svg")
                            .size(px(20.))
                            .into_any_element(),
                        "Пикер эмодзи скоро",
                        cx,
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
        )
    }
}

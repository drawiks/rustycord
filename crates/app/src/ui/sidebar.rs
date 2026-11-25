use super::widgets::*;
use super::*;
use gpui_kit::{AnyElement, Div, Stateful};

impl App {
    pub(super) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_dm = self.selected_guild.is_none();
        let title = match &self.selected_guild {
            Some(id) => self
                .state
                .guild(id)
                .map(|guild| guild.name.clone())
                .unwrap_or_default(),
            None => "Личные сообщения".to_string(),
        };

        let mut list = div()
            .id("channels-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(1.))
            .px(px(8.))
            .pb(px(8.));

        if is_dm {
            list = self.dm_list(list, cx);
        } else if let Some(guild_id) = self.selected_guild.clone() {
            list = self.guild_channel_list(&guild_id, list, cx);
        }

        div()
            .w(px(240.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(SIDEBAR))
            .child({
                let visual = match &self.selected_guild {
                    Some(guild_id) => self
                        .state
                        .guild(guild_id)
                        .map(|guild| match &guild.icon {
                            Some(icon) => gpui_kit::img(guild_icon_url(guild_id, icon))
                                .size(px(32.))
                                .into_any_element(),
                            None => div()
                                .size(px(32.))
                                .rounded(px(16.))
                                .bg(rgb(RAIL))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xffffff))
                                .child(initials(&guild.name))
                                .into_any_element(),
                        })
                        .unwrap_or_else(|| div().size(px(32.)).into_any_element()),
                    None => div()
                        .size(px(32.))
                        .rounded(px(16.))
                        .bg(rgb(RAIL))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui_kit::component::Icon::new(IconName::MessageSquare)
                                .size(px(18.))
                                .text_color(rgb(MUTED)),
                        )
                        .into_any_element(),
                };
                div()
                    .h(px(48.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .border_b(px(1.))
                    .border_color(rgb(RAIL))
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(TEXT_BRIGHT))
                    .child(div().flex_none().child(visual))
                    .child(div().flex_1().truncate().child(title))
                    .child(
                        gpui_kit::component::Icon::new(IconName::ChevronDown)
                            .size(px(14.))
                            .text_color(rgb(ICON)),
                    )
                    .child(if self.selected_guild.is_some() {
                        let open = matches!(self.right_pane, Some(RightPane::Members));
                        div()
                            .id("sidebar-members")
                            .size(px(24.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(if open { 0xffffff } else { MUTED }))
                            .cursor_pointer()
                            .hover(|style| style.text_color(rgb(TEXT_BRIGHT)))
                            .child(gpui_kit::component::Icon::new(IconName::Users).size(px(18.)))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_members(cx)))
                            .into_any_element()
                    } else {
                        div().size(px(24.)).into_any_element()
                    })
            })
            .child(list)
            .child(self.render_user_panel(cx))
    }

    fn guild_channel_list(
        &mut self,
        guild_id: &str,
        mut list: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Some(guild) = self.state.guild(guild_id) else {
            return list;
        };
        let channels = &guild.channels;

        let visible_text = self.state.visible_text_channels(guild_id);
        let visible_voice = self.state.visible_voice_channels(guild_id);
        let visible: Vec<&Channel> = visible_text.into_iter().chain(visible_voice).collect();

        let mut categories: Vec<&Channel> = channels
            .iter()
            .filter(|channel| channel.is_category())
            .collect();
        categories.sort_by_key(|category| category.position);
        let category_ids: HashSet<&str> = categories
            .iter()
            .map(|category| category.id.as_str())
            .collect();

        let mut loose: Vec<&Channel> = visible
            .iter()
            .copied()
            .filter(|channel| {
                channel
                    .parent_id
                    .as_deref()
                    .is_none_or(|parent| !category_ids.contains(parent))
            })
            .collect();
        loose.sort_by_key(|channel| channel.position);

        if loose.is_empty() && categories.is_empty() {
            list = list.child(empty_state(
                IconName::Hash,
                "Нет каналов",
                "На этом сервере нет доступных каналов",
            ));
        }

        for channel in loose {
            list = list.child(self.channel_entry(channel, 0, cx));
        }

        for category in categories {
            let mut children: Vec<&Channel> = visible
                .iter()
                .copied()
                .filter(|channel| channel.parent_id.as_deref() == Some(category.id.as_str()))
                .collect();
            if children.is_empty() {
                continue;
            }
            children.sort_by_key(|channel| channel.position);
            let collapsed = self.collapsed.contains(&category.id);
            list = list.child(self.category_row(category, collapsed, cx));
            if collapsed {
                continue;
            }
            for channel in children {
                list = list.child(self.channel_entry(channel, 1, cx));
            }
        }

        list
    }

    fn channel_entry(&self, channel: &Channel, depth: usize, cx: &mut Context<Self>) -> AnyElement {
        if channel.is_voice() {
            self.voice_entry(channel, depth, cx)
        } else {
            self.channel_row(channel, depth, cx).into_any_element()
        }
    }

    fn voice_entry(&self, channel: &Channel, depth: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(guild_id) = channel
            .guild_id
            .clone()
            .or_else(|| self.selected_guild.clone())
        else {
            return div().into_any_element();
        };
        let name = channel.display_name();
        let members = self.state.voice_members(&guild_id, &channel.id);
        let count = members.len();
        let count_label = if count == 0 {
            String::new()
        } else if channel.user_limit > 0 {
            format!("{count} / {}", channel.user_limit)
        } else {
            count.to_string()
        };
        let notice = "Голосовые каналы пока не поддерживаются".to_string();

        let mut column = div().flex().flex_col().child(
            div()
                .id(gpui_kit::SharedString::from(format!(
                    "voice-{}",
                    channel.id
                )))
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(34.))
                .pl(px(8. + depth as f32 * 10.))
                .pr(px(8.))
                .rounded(px(4.))
                .text_size(px(15.))
                .text_color(rgb(MUTED))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(RAIL_HOVER)))
                .child(
                    gpui_kit::component::Icon::new(IconName::Volume2)
                        .size(px(18.))
                        .text_color(rgb(ICON)),
                )
                .child(div().flex_1().truncate().child(name))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(FAINT))
                        .child(count_label),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.notice = Some(notice.clone());
                    cx.notify();
                })),
        );

        for (user, state) in members {
            let user_name = self
                .state
                .member_name(&guild_id, &user.id)
                .map(str::to_string)
                .unwrap_or_else(|| user.display_name().to_string());
            let presence = self.state.presence(&user.id).to_string();
            let streaming = state.self_stream;
            let mut entry = div()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(30.))
                .pl(px(28. + depth as f32 * 10.))
                .pr(px(8.))
                .child(mini_avatar(Some(user), &user_name, &presence))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_size(px(13.))
                        .text_color(rgb(TEXT))
                        .child(user_name),
                );
            if streaming {
                entry = entry.child(
                    div()
                        .px(px(4.))
                        .rounded(px(3.))
                        .bg(rgb(0x8b5cf6))
                        .text_size(px(9.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xffffff))
                        .child("В ЭФИРЕ"),
                );
            }
            column = column.child(entry);
        }

        column.into_any_element()
    }

    fn category_row(
        &self,
        category: &Channel,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = category.id.clone();
        let name = category.name.clone().unwrap_or_default();
        let icon = if collapsed {
            IconName::ChevronRight
        } else {
            IconName::ChevronDown
        };
        div()
            .id(gpui_kit::SharedString::from(format!(
                "category-{}",
                category.id
            )))
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(4.))
            .pt(px(14.))
            .pb(px(4.))
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(MUTED))
            .cursor_pointer()
            .hover(|style| style.text_color(rgb(TEXT_BRIGHT)))
            .child(gpui_kit::component::Icon::new(icon).size(px(12.)))
            .child(div().truncate().child(name))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.collapsed.contains(&id) {
                    this.collapsed.remove(&id);
                } else {
                    this.collapsed.insert(id.clone());
                }
                cx.notify();
            }))
    }

    fn channel_row(
        &self,
        channel: &Channel,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.selected_channel.as_ref() == Some(&channel.id);
        let (unread, mentions) = self.channel_unread(channel);
        let id = channel.id.clone();
        let name = self.state.channel_display_name(channel);

        let mut row = div()
            .id(gpui_kit::SharedString::from(format!(
                "channel-{}",
                channel.id
            )))
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(30.))
            .pl(px(8. + depth as f32 * 10.))
            .pr(px(8.))
            .rounded(px(4.))
            .text_size(px(15.))
            .cursor_pointer()
            .bg(if selected { rgb(ACTIVE) } else { rgb(SIDEBAR) })
            .hover(|style| style.bg(rgb(RAIL_HOVER)))
            .child(
                gpui_kit::component::Icon::new(IconName::Hash)
                    .size(px(18.))
                    .text_color(rgb(ICON)),
            )
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .font_weight(if unread && !selected {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(rgb(if selected {
                        0xffffff
                    } else if unread {
                        TEXT_BRIGHT
                    } else {
                        MUTED
                    }))
                    .child(name),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_channel(id.clone(), cx);
                this.focus_composer(window, cx);
            }));

        if mentions > 0 {
            row = row.child(mention_badge(mentions));
        } else if unread {
            row = row.child(unread_dot());
        }
        row
    }

    fn dm_list(&mut self, mut list: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut channels: Vec<&Channel> = self
            .state
            .channels
            .iter()
            .filter(|channel| channel.kind == Channel::DM || channel.kind == Channel::GROUP_DM)
            .collect();
        channels.sort_by_key(|channel| {
            std::cmp::Reverse(
                channel
                    .last_message_id
                    .as_deref()
                    .map(snowflake)
                    .unwrap_or(0),
            )
        });

        if channels.is_empty() {
            list = list.child(empty_state(
                IconName::AtSign,
                "Нет личных сообщений",
                "Личные чаты появятся здесь",
            ));
        }

        for channel in channels {
            let selected = self.selected_channel.as_ref() == Some(&channel.id);
            let (unread, mentions) = self.channel_unread(channel);
            let id = channel.id.clone();
            let name = self.state.channel_display_name(channel);
            let user = dm_user(&self.state, channel);
            let presence = user
                .map(|user| self.state.presence(&user.id).to_string())
                .unwrap_or_default();
            let time = channel
                .last_message_id
                .as_deref()
                .and_then(snowflake_time)
                .map(short_time)
                .unwrap_or_default();

            let mut row = div()
                .id(gpui_kit::SharedString::from(format!("dm-{}", channel.id)))
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(46.))
                .px(px(8.))
                .rounded(px(4.))
                .cursor_pointer()
                .bg(if selected { rgb(ACTIVE) } else { rgb(SIDEBAR) })
                .hover(|style| style.bg(rgb(RAIL_HOVER)))
                .child(user_avatar(user, &name, &presence))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .truncate()
                                .font_weight(if unread && !selected {
                                    FontWeight::MEDIUM
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(rgb(if selected {
                                    0xffffff
                                } else if unread {
                                    TEXT_BRIGHT
                                } else {
                                    MUTED
                                }))
                                .child(name),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(if unread { TEXT } else { FAINT }))
                                .child(time),
                        ),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_channel(id.clone(), cx);
                    this.focus_composer(window, cx);
                }));

            if mentions > 0 {
                row = row.child(mention_badge(mentions));
            } else if unread {
                row = row.child(unread_dot());
            }
            list = list.child(row);
        }

        list
    }

    fn render_user_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let me = self.state.me.clone();
        let name = me
            .as_ref()
            .map(|user| user.display_name().to_string())
            .unwrap_or_else(|| "not signed in".to_string());
        let status = me
            .as_ref()
            .map(|user| self.state.presence(&user.id).to_string())
            .unwrap_or_else(|| "offline".to_string());

        div()
            .h(px(52.))
            .flex_none()
            .bg(rgb(PANEL))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id("user-panel")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .flex_1()
                    .min_w(px(0.))
                    .cursor_pointer()
                    .child(user_avatar(me.as_ref(), &name, &status))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_BRIGHT))
                                    .truncate()
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(MUTED))
                                    .child(status_text(&status)),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.status_menu = !this.status_menu;
                        this.emoji_picker = false;
                        this.profile_popup = None;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(panel_button(
                        "panel-mic",
                        IconName::Mic,
                        "Голосовые каналы пока не поддерживаются",
                        cx,
                    ))
                    .child(panel_button(
                        "panel-headphones",
                        IconName::Headphones,
                        "Голосовые каналы пока не поддерживаются",
                        cx,
                    ))
                    .child(panel_button(
                        "panel-settings",
                        IconName::Settings,
                        "Настройки скоро",
                        cx,
                    ))
                    .child(
                        div()
                            .id("logout")
                            .size(px(28.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(MUTED))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
                            .child(gpui_kit::component::Icon::new(IconName::LogOut).size(px(16.)))
                            .on_click(cx.listener(|this, _, window, cx| this.logout(window, cx))),
                    ),
            )
    }
}

fn panel_button(
    id: &'static str,
    icon: IconName,
    notice: &'static str,
    cx: &mut Context<App>,
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
        .child(gpui_kit::component::Icon::new(icon).size(px(16.)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.notice = Some(notice.to_string());
            cx.notify();
        }))
}

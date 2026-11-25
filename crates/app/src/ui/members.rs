use super::widgets::*;
use super::*;
use gpui_kit::{AnyElement, WeakEntity, linear_color_stop, linear_gradient};

impl App {
    pub(super) fn render_right_pane(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.right_pane.clone() {
            Some(RightPane::Members) => self.render_members_panel(cx).into_any_element(),
            Some(RightPane::Profile { user_id, guild_id }) => self
                .render_profile_panel(&user_id, guild_id.as_deref(), cx)
                .into_any_element(),
            None => div().into_any_element(),
        };

        div()
            .w(px(260.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(SIDEBAR))
            .border_l(px(1.))
            .border_color(rgb(RAIL))
            .child(content)
    }

    fn render_members_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let list = gpui_kit::uniform_list(
            "members-list",
            self.member_rows.len(),
            move |range, _window, cx| {
                let Some(entity) = weak.upgrade() else {
                    return Vec::new();
                };
                let app = entity.read(cx);
                range
                    .map(|index| app.render_member_row(index, &weak))
                    .collect::<Vec<_>>()
            },
        );

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(div().flex_1().min_h(px(0.)).child(list))
    }

    fn render_member_row(&self, index: usize, entity: &WeakEntity<App>) -> AnyElement {
        let Some(row) = self.member_rows.get(index) else {
            return div().into_any_element();
        };

        match row {
            MemberRow::Header {
                label,
                color,
                count,
            } => div()
                .h(px(48.))
                .flex()
                .items_end()
                .pb(px(6.))
                .px(px(12.))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(if *color == 0 { MUTED } else { *color }))
                .child(format!("{label} — {count}"))
                .into_any_element(),
            MemberRow::Member { user_id } => {
                let Some(guild_id) = self.selected_guild.clone() else {
                    return div().into_any_element();
                };
                let Some(user) = self.state.user(user_id) else {
                    return div().into_any_element();
                };
                let name = self
                    .state
                    .member_name(&guild_id, user_id)
                    .map(str::to_string)
                    .unwrap_or_else(|| user.display_name().to_string());
                let color = self.state.member_role_color(&guild_id, user_id);
                let color = if color == 0 { TEXT } else { color };
                let status = self.state.presence(user_id).to_string();
                let avatar_name = name.clone();
                let offline = status == "offline";
                let activity = self.state.presence_detail(user_id);
                let streaming = self.state.presence_streaming(user_id);
                let owner = self.state.is_guild_owner(&guild_id, user_id);
                let role_badge: Option<AnyElement> = self
                    .state
                    .member_top_role(&guild_id, user_id)
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
                    });
                let clan_tag = user
                    .clan
                    .as_ref()
                    .or(user.primary_guild.as_ref())
                    .and_then(|clan| clan.tag.clone())
                    .filter(|tag| !tag.is_empty());
                let weak = entity.clone();
                let click_id = user_id.clone();
                let click_guild = guild_id.clone();

                let mut name_line = div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .truncate()
                            .text_color(rgb(color))
                            .child(name),
                    )
                    .children(role_badge);

                if user.bot {
                    name_line = name_line.child(
                        div()
                            .px(px(4.))
                            .rounded(px(3.))
                            .bg(rgb(ACCENT))
                            .text_size(px(9.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xffffff))
                            .child("BOT"),
                    );
                }
                if let Some(tag) = clan_tag {
                    name_line = name_line.child(
                        div()
                            .px(px(4.))
                            .rounded(px(3.))
                            .bg(rgb(0x3c4270))
                            .text_size(px(9.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xc9cdfb))
                            .child(tag),
                    );
                }
                if owner {
                    name_line = name_line.child(
                        gpui_kit::component::Icon::new(IconName::Crown)
                            .size(px(12.))
                            .text_color(rgb(IDLE)),
                    );
                }
                if streaming {
                    name_line = name_line.child(
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

                let mut member_column = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .child(name_line);
                if let Some(activity) = activity {
                    member_column = member_column.child(
                        div()
                            .text_size(px(12.))
                            .truncate()
                            .text_color(rgb(MUTED))
                            .child(activity),
                    );
                }

                let mut row = div()
                    .id(gpui_kit::SharedString::from(format!("member-{user_id}")))
                    .h(px(48.))
                    .mx(px(4.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)))
                    .child(user_avatar(Some(user), &avatar_name, &status))
                    .child(member_column);

                if offline {
                    row = row.opacity(0.35);
                }

                row.on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.profile_popup = Some((click_id.clone(), Some(click_guild.clone())));
                        cx.notify();
                    });
                })
                .into_any_element()
            }
        }
    }

    fn render_profile_panel(
        &self,
        user_id: &str,
        guild_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let header = div()
            .h(px(48.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px(px(8.))
            .border_b(px(1.))
            .border_color(rgb(RAIL))
            .child(if guild_id.is_some() {
                div()
                    .id("profile-back")
                    .size(px(28.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
                    .child(gpui_kit::component::Icon::new(IconName::ChevronLeft).size(px(16.)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_pane = Some(RightPane::Members);
                        cx.notify();
                    }))
                    .into_any_element()
            } else {
                div().size(px(28.)).into_any_element()
            })
            .child(
                div()
                    .id("profile-close")
                    .size(px(28.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
                    .child(gpui_kit::component::Icon::new(IconName::X).size(px(14.)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_pane = None;
                        cx.notify();
                    })),
            );

        div()
            .id("profile-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .child(header)
            .child(
                div()
                    .p(px(12.))
                    .child(self.render_profile_card(user_id, guild_id, cx)),
            )
    }

    pub(super) fn render_profile_popup(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some((user_id, guild_id)) = self.profile_popup.clone() else {
            return div().into_any_element();
        };
        let right = if self.right_pane.is_some() {
            px(272.)
        } else {
            px(16.)
        };

        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("profile-backdrop")
                    .absolute()
                    .size_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.profile_popup = None;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top(px(56.))
                    .right(right)
                    .w(px(340.))
                    .shadow_lg()
                    .child(self.render_profile_card(&user_id, guild_id.as_deref(), cx)),
            )
            .into_any_element()
    }

    fn render_profile_card(
        &self,
        user_id: &str,
        guild_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let user = self.state.user(user_id).cloned();
        let name = match (&user, guild_id) {
            (Some(user), Some(guild_id)) => self.state.member_display_name(guild_id, user),
            (Some(user), None) => user.display_name().to_string(),
            _ => user_id.to_string(),
        };
        let username = user
            .as_ref()
            .map(|user| format!("@{}", user.username))
            .unwrap_or_default();
        let status = self.state.presence(user_id).to_string();
        let color = guild_id
            .map(|guild_id| self.state.member_role_color(guild_id, user_id))
            .unwrap_or(0);
        let color = if color == 0 { TEXT_BRIGHT } else { color };
        let roles: Vec<(String, u32)> = guild_id
            .map(|guild_id| {
                self.state
                    .member_roles(guild_id, user_id)
                    .into_iter()
                    .map(|role| (role.name.clone(), role.color))
                    .collect()
            })
            .unwrap_or_default();
        let joined = guild_id
            .and_then(|guild_id| self.state.member(guild_id, user_id))
            .and_then(|member| member.joined_at.clone())
            .and_then(|value| parse_timestamp(&value))
            .map(|time| time.format("%d.%m.%Y").to_string());
        let owner = guild_id.is_some_and(|guild_id| self.state.is_guild_owner(guild_id, user_id));
        let dm = self
            .state
            .channels
            .iter()
            .find(|channel| {
                (channel.kind == Channel::DM || channel.kind == Channel::GROUP_DM)
                    && (channel.recipient_ids.iter().any(|id| id == user_id)
                        || channel.recipients.iter().any(|user| user.id == user_id))
            })
            .map(|channel| channel.id.clone());
        let is_self = self.state.me.as_ref().is_some_and(|me| me.id == user_id);
        let current_dm = dm
            .as_ref()
            .is_some_and(|id| self.selected_channel.as_ref() == Some(id));

        let mut card = div()
            .rounded(px(10.))
            .overflow_hidden()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(RAIL))
            .flex()
            .flex_col()
            .child(div().h(px(64.)).bg(linear_gradient(
                90.,
                linear_color_stop(rgb(ACCENT), 0.),
                linear_color_stop(rgb(PANEL), 1.),
            )))
            .child(
                div()
                    .px(px(12.))
                    .pb(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .mt(px(-26.))
                            .child(large_avatar(user.as_ref(), &name, &status)),
                    )
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(color))
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .child(username),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .text_color(rgb(TEXT))
                            .child(presence_dot(&status))
                            .child(status_text(&status)),
                    ),
            );

        if owner {
            card = card.child(
                div()
                    .px(px(16.))
                    .pb(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(
                        gpui_kit::component::Icon::new(IconName::Crown)
                            .size(px(14.))
                            .text_color(rgb(IDLE)),
                    )
                    .child("Владелец сервера"),
            );
        }

        if let Some(joined) = joined {
            card = card.child(
                div()
                    .px(px(16.))
                    .pb(px(10.))
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(format!("На сервере с {joined}")),
            );
        }

        if !roles.is_empty() {
            let chips: Vec<AnyElement> = roles
                .iter()
                .map(|(name, color)| {
                    let color = if *color == 0 { MUTED } else { *color };
                    div()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(rgb(RAIL))
                        .text_size(px(11.))
                        .text_color(rgb(color))
                        .child(name.clone())
                        .into_any_element()
                })
                .collect();
            card = card.child(
                div()
                    .px(px(16.))
                    .pb(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(MUTED))
                            .child("РОЛИ"),
                    )
                    .child(div().flex().flex_wrap().gap(px(4.)).children(chips)),
            );
        }

        card = card.child(div().px(px(16.)).pb(px(16.)).child(if is_self {
            div()
                .text_size(px(12.))
                .text_color(rgb(FAINT))
                .child("Это вы")
                .into_any_element()
        } else if let Some(dm) = dm {
            if current_dm {
                div()
                    .text_size(px(12.))
                    .text_color(rgb(FAINT))
                    .child("Текущий чат")
                    .into_any_element()
            } else {
                let click_id = dm;
                div()
                    .id("profile-message")
                    .w_full()
                    .flex()
                    .justify_center()
                    .py(px(8.))
                    .rounded(px(6.))
                    .bg(rgb(ACCENT))
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xffffff))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x4752c4)))
                    .child("Написать")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_channel(click_id.clone(), cx);
                        this.right_pane = None;
                        this.profile_popup = None;
                        this.focus_composer(window, cx);
                    }))
                    .into_any_element()
            }
        } else {
            div()
                .text_size(px(12.))
                .text_color(rgb(FAINT))
                .child("Личных сообщений пока нет")
                .into_any_element()
        }));

        card
    }
}

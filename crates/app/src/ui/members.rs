use super::widgets::*;
use super::*;
use gpui_kit::{AnyElement, WeakEntity};

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
        let title = self
            .selected_guild
            .as_ref()
            .and_then(|id| self.state.guild(id))
            .map(|guild| format!("{} — {}", guild.name, self.state.members(&guild.id).len()))
            .unwrap_or_else(|| "Members".to_string());

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
            .child(
                div()
                    .h(px(48.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .border_b(px(1.))
                    .border_color(rgb(RAIL))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT_BRIGHT))
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .id("close-members")
                            .size(px(24.))
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
                    ),
            )
            .child(div().flex_1().min_h(px(0.)).child(list))
    }

    fn render_member_row(&self, index: usize, entity: &WeakEntity<App>) -> AnyElement {
        let Some(row) = self.member_rows.get(index) else {
            return div().into_any_element();
        };

        match row {
            MemberRow::Header { label, count } => div()
                .h(px(44.))
                .flex()
                .items_end()
                .pb(px(6.))
                .px(px(12.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(MUTED))
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
                let owner = self.state.is_guild_owner(&guild_id, user_id);
                let weak = entity.clone();
                let click_id = user_id.clone();
                let click_guild = guild_id.clone();

                let mut row = div()
                    .id(gpui_kit::SharedString::from(format!("member-{user_id}")))
                    .h(px(44.))
                    .mx(px(4.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)))
                    .child(user_avatar(Some(user), &name, &status))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .truncate()
                                    .text_color(rgb(color))
                                    .child(name),
                            ),
                    );

                if owner {
                    row = row.child(
                        gpui_kit::component::Icon::new(IconName::Crown)
                            .size(px(12.))
                            .text_color(rgb(IDLE)),
                    );
                }

                row.on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.right_pane = Some(RightPane::Profile {
                            user_id: click_id.clone(),
                            guild_id: Some(click_guild.clone()),
                        });
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
            .map(|time| time.format("%d %b %Y").to_string());
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

        let mut panel = div()
            .id("profile-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .child(header)
            .child(
                div()
                    .m(px(12.))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .bg(rgb(PANEL))
                    .flex()
                    .flex_col()
                    .child(div().h(px(56.)).bg(rgb(ACCENT)))
                    .child(
                        div()
                            .px(px(12.))
                            .pb(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().mt(px(-22.)).child(large_avatar(
                                user.as_ref(),
                                &name,
                                &status,
                            )))
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
                                    .child(status),
                            ),
                    ),
            );

        if owner {
            panel = panel.child(
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
                    .child("Server owner"),
            );
        }

        if let Some(joined) = joined {
            panel = panel.child(
                div()
                    .px(px(16.))
                    .pb(px(10.))
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(format!("Joined {joined}")),
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
            panel = panel.child(
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
                            .child("ROLES"),
                    )
                    .child(div().flex().flex_wrap().gap(px(4.)).children(chips)),
            );
        }

        panel = panel.child(div().px(px(16.)).pb(px(16.)).child(if let Some(dm) = dm {
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
                .child("Message")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_channel(click_id.clone(), cx);
                    this.right_pane = None;
                    this.focus_composer(window, cx);
                }))
                .into_any_element()
        } else {
            div()
                .text_size(px(12.))
                .text_color(rgb(FAINT))
                .child("No direct messages yet")
                .into_any_element()
        }));

        panel
    }
}

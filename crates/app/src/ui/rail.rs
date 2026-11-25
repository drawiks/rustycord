use super::widgets::*;
use super::*;

impl App {
    pub(super) fn render_rail(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let (dm_unread, dm_mentions) = self
            .state
            .channels
            .iter()
            .filter(|channel| channel.kind == Channel::DM || channel.kind == Channel::GROUP_DM)
            .fold((false, 0u32), |(unread, mentions), channel| {
                let (channel_unread, channel_mentions) = self.channel_unread(channel);
                (unread || channel_unread, mentions + channel_mentions)
            });
        let dm_visual = gpui_kit::component::Icon::new(IconName::MessageSquare)
            .size(px(24.))
            .text_color(rgb(0xffffff))
            .into_any_element();
        rail = rail.child(rail_button(
            gpui_kit::SharedString::from("rail-dms"),
            dm_selected,
            dm_unread,
            dm_mentions,
            dm_visual,
            cx.listener(|this, _, window, cx| {
                this.select_guild(None, cx);
                this.focus_composer(window, cx);
            }),
        ));
        rail = rail.child(
            div()
                .w(px(32.))
                .h(px(2.))
                .rounded(px(1.))
                .bg(rgb(RAIL_HOVER)),
        );

        for guild in &self.state.guilds {
            let id = guild.id.clone();
            let selected = self.selected_guild.as_ref() == Some(&id);
            let (unread, mentions) = self.guild_unread(&id);
            let visual = match &guild.icon {
                Some(icon) => gpui_kit::img(guild_icon_url(&id, icon))
                    .size(px(48.))
                    .into_any_element(),
                None => div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xffffff))
                    .child(initials(&guild.name))
                    .into_any_element(),
            };
            let click_id = id.clone();
            rail = rail.child(rail_button(
                gpui_kit::SharedString::from(format!("rail-{id}")),
                selected,
                unread,
                mentions,
                visual,
                cx.listener(move |this, _, window, cx| {
                    this.select_guild(Some(click_id.clone()), cx);
                    this.focus_composer(window, cx);
                }),
            ));
        }

        for (id, icon, color, notice) in [
            (
                "rail-add",
                IconName::Plus,
                ONLINE,
                "Создание серверов скоро",
            ),
            (
                "rail-explore",
                IconName::Compass,
                MUTED,
                "Обзор серверов скоро",
            ),
        ] {
            rail = rail.child(
                div()
                    .id(id)
                    .size(px(48.))
                    .rounded(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(SIDEBAR))
                    .text_color(rgb(color))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)))
                    .child(gpui_kit::component::Icon::new(icon).size(px(22.)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.notice = Some(notice.to_string());
                        cx.notify();
                    })),
            );
        }

        rail
    }
}

fn rail_button<F>(
    id: gpui_kit::SharedString,
    selected: bool,
    unread: bool,
    mentions: u32,
    visual: gpui_kit::AnyElement,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
{
    let pill_height = if selected {
        px(28.)
    } else if unread {
        px(8.)
    } else {
        px(0.)
    };

    div()
        .relative()
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .absolute()
                .left(px(0.))
                .w(px(4.))
                .h(pill_height)
                .rounded(px(2.))
                .bg(rgb(0xffffff)),
        )
        .child(
            div()
                .id(id)
                .relative()
                .size(px(48.))
                .cursor_pointer()
                .child(
                    div()
                        .size(px(48.))
                        .rounded(if selected { px(14.) } else { px(16.) })
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(if selected { rgb(ACCENT) } else { rgb(SIDEBAR) })
                        .hover(move |style| {
                            style.bg(if selected {
                                rgb(ACCENT)
                            } else {
                                rgb(RAIL_HOVER)
                            })
                        })
                        .child(visual),
                )
                .children(if mentions > 0 {
                    Some(
                        div()
                            .absolute()
                            .bottom(px(-2.))
                            .right(px(-2.))
                            .child(mention_badge(mentions)),
                    )
                } else {
                    None
                })
                .on_click(on_click),
        )
}

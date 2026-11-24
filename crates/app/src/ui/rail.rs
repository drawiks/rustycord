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
            let (unread, mentions) = self.guild_unread(&id);
            let mut button = div()
                .id(element_id)
                .relative()
                .size(px(48.))
                .rounded(px(16.))
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center()
                .bg(if selected { rgb(ACCENT) } else { rgb(SIDEBAR) })
                .cursor_pointer()
                .child(visual);
            if mentions > 0 {
                button = button.child(
                    div()
                        .absolute()
                        .bottom(px(0.))
                        .right(px(0.))
                        .child(mention_badge(mentions)),
                );
            } else if unread {
                button = button.child(
                    div()
                        .absolute()
                        .bottom(px(2.))
                        .right(px(2.))
                        .child(unread_dot()),
                );
            }
            rail = rail.child(button.on_click(cx.listener(move |this, _, window, cx| {
                this.select_guild(Some(click_id.clone()), cx);
                this.focus_composer(window, cx);
            })));
        }

        rail
    }
}

use super::widgets::*;
use super::*;
use gpui_kit::{AnyElement, SharedString};

const EMOJIS: &[&str] = &[
    "😀", "😂", "😊", "😍", "😘", "😎", "🤔", "😭", "😡", "👍", "👎", "👏", "🙏", "💪", "🔥", "✨",
    "🎉", "❤️", "💔", "😢", "😮", "🤣", "😴", "🤝", "👀", "💯", "✅", "❌", "⚡", "🌙", "☀️", "🍕",
    "🎮", "🎵", "📌", "🚀", "🐱", "🐶", "🍀", "🕹️", "💀", "🥳", "🤯", "😅", "🙃", "🫡", "👌", "🫶",
];

impl App {
    pub(super) fn render_emoji_picker(&mut self, cx: &mut Context<Self>) -> AnyElement {
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
                    .id("emoji-backdrop")
                    .absolute()
                    .size_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.emoji_picker = false;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(96.))
                    .right(right)
                    .w(px(320.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .bg(rgb(SIDEBAR))
                    .border_1()
                    .border_color(rgb(RAIL))
                    .shadow_lg()
                    .flex()
                    .flex_wrap()
                    .gap(px(2.))
                    .children(EMOJIS.iter().map(|emoji| {
                        let emoji = *emoji;
                        div()
                            .id(SharedString::from(format!("emoji-{emoji}")))
                            .size(px(34.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(20.))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(RAIL_HOVER)))
                            .child(emoji)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.insert_emoji(emoji, window, cx)
                            }))
                    })),
            )
            .into_any_element()
    }

    pub(super) fn render_status_menu(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let current = self
            .state
            .me
            .as_ref()
            .map(|user| self.state.presence(&user.id).to_string())
            .unwrap_or_default();

        let statuses = [
            ("online", "В сети"),
            ("idle", "Не активен"),
            ("dnd", "Не беспокоить"),
            ("invisible", "Невидимка"),
        ];

        let mut menu = div()
            .absolute()
            .bottom(px(60.))
            .left(px(8.))
            .w(px(220.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(rgb(SIDEBAR))
            .border_1()
            .border_color(rgb(RAIL))
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(1.));

        for (status, label) in statuses {
            let selected = current == status;
            menu = menu.child(
                div()
                    .id(SharedString::from(format!("status-{status}")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)))
                    .child(presence_dot(status))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .text_color(rgb(TEXT))
                            .child(label),
                    )
                    .child(if selected {
                        gpui_kit::component::Icon::new(IconName::Check)
                            .size(px(14.))
                            .text_color(rgb(ACCENT))
                            .into_any_element()
                    } else {
                        div().size(px(14.)).into_any_element()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_status(status, cx))),
            );
        }

        menu = menu
            .child(div().my(px(4.)).h(px(1.)).bg(rgb(RAIL)))
            .child(
                div()
                    .id("status-settings")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)))
                    .child(
                        gpui_kit::component::Icon::new(IconName::Settings)
                            .size(px(14.))
                            .text_color(rgb(MUTED)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(TEXT))
                            .child("Настройки"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notice = Some("Настройки скоро".to_string());
                        this.status_menu = false;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("status-logout")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(RAIL_HOVER)))
                    .child(
                        gpui_kit::component::Icon::new(IconName::LogOut)
                            .size(px(14.))
                            .text_color(rgb(DANGER)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(DANGER))
                            .child("Выйти"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.logout(window, cx))),
            );

        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("status-backdrop")
                    .absolute()
                    .size_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.status_menu = false;
                        cx.notify();
                    })),
            )
            .child(menu)
            .into_any_element()
    }
}

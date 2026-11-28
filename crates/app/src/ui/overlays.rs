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

    pub(super) fn render_incoming_call(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(call) = self.incoming_call.clone() else {
            return div().into_any_element();
        };
        let me = self.state.me.as_ref().map(|user| user.id.as_str());
        let channel = self.state.channel(&call.channel_id);
        let caller = channel.and_then(|channel| {
            channel
                .recipients
                .iter()
                .find(|user| Some(user.id.as_str()) != me)
                .cloned()
                .or_else(|| {
                    channel
                        .recipient_ids
                        .iter()
                        .find(|id| Some(id.as_str()) != me)
                        .and_then(|id| self.state.user(id).cloned())
                })
        });
        let name = caller
            .as_ref()
            .map(|user| user.display_name().to_string())
            .unwrap_or_else(|| "Неизвестный".to_string());
        let status = caller
            .as_ref()
            .map(|user| self.state.presence(&user.id).to_string())
            .unwrap_or_else(|| "offline".to_string());

        div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::rgba(0x00000099))
            .child(
                div()
                    .w(px(340.))
                    .p(px(20.))
                    .rounded(px(12.))
                    .bg(rgb(SIDEBAR))
                    .border_1()
                    .border_color(rgb(RAIL))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(MUTED))
                            .child("ВХОДЯЩИЙ ЗВОНОК"),
                    )
                    .child(user_avatar(caller.as_ref(), &name, &status))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(TEXT_BRIGHT))
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(MUTED))
                            .child(status_text(&status)),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("call-decline")
                                    .size(px(48.))
                                    .rounded(px(24.))
                                    .bg(rgb(DANGER))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .hover(|style| style.opacity(0.85))
                                    .child(
                                        gpui_kit::component::Icon::new(IconName::PhoneOff)
                                            .size(px(22.))
                                            .text_color(rgb(0xffffff)),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| this.decline_call(cx))),
                            )
                            .child(
                                div()
                                    .id("call-accept")
                                    .size(px(48.))
                                    .rounded(px(24.))
                                    .bg(rgb(0x23a55a))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .hover(|style| style.opacity(0.85))
                                    .child(
                                        gpui_kit::component::Icon::new(IconName::PhoneCall)
                                            .size(px(22.))
                                            .text_color(rgb(0xffffff)),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| this.accept_call(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut card = div()
            .w(px(460.))
            .max_h(px(600.))
            .p(px(18.))
            .rounded(px(12.))
            .bg(rgb(SIDEBAR))
            .border_1()
            .border_color(rgb(RAIL))
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(TEXT_BRIGHT))
                            .child("Голос и видео"),
                    )
                    .child(
                        div()
                            .id("settings-close")
                            .size(px(28.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(MUTED))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(RAIL_HOVER)).text_color(rgb(TEXT_BRIGHT)))
                            .child(gpui_kit::component::Icon::new(IconName::X).size(px(16.)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(section_label("УСТРОЙСТВО ВВОДА"))
            .child(device_option(
                "По умолчанию".to_string(),
                None,
                self.input_device.is_none(),
                true,
                cx,
            ));

        for name in self.input_devices.clone() {
            let selected = self.input_device.as_deref() == Some(name.as_str());
            card = card.child(device_option(name.clone(), Some(name), selected, true, cx));
        }
        if self.input_devices.is_empty() {
            card = card.child(hint("Устройства ввода не найдены"));
        }

        card = card
            .child(section_label("УСТРОЙСТВО ВЫВОДА"))
            .child(device_option(
                "По умолчанию".to_string(),
                None,
                self.output_device.is_none(),
                false,
                cx,
            ));
        for name in self.output_devices.clone() {
            let selected = self.output_device.as_deref() == Some(name.as_str());
            card = card.child(device_option(name.clone(), Some(name), selected, false, cx));
        }
        if self.output_devices.is_empty() {
            card = card.child(hint("Устройства вывода не найдены"));
        }

        div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("settings-backdrop")
                    .absolute()
                    .size_full()
                    .bg(gpui_kit::rgba(0x00000099))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings_open = false;
                        cx.notify();
                    })),
            )
            .child(card)
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
                    .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx))),
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

fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .mt(px(8.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(text)
}

fn hint(text: &'static str) -> impl IntoElement {
    div()
        .px(px(8.))
        .py(px(6.))
        .text_size(px(12.))
        .text_color(rgb(FAINT))
        .child(text)
}

fn device_option(
    label: String,
    value: Option<String>,
    selected: bool,
    input: bool,
    cx: &mut Context<App>,
) -> AnyElement {
    let click_value = value.clone();
    div()
        .id(SharedString::from(format!(
            "{}-{}",
            if input { "input" } else { "output" },
            value.as_deref().unwrap_or("default")
        )))
        .flex()
        .items_center()
        .justify_between()
        .px(px(8.))
        .py(px(7.))
        .rounded(px(4.))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(RAIL_HOVER)))
        .child(
            div()
                .text_size(px(13.))
                .text_color(rgb(if selected { TEXT_BRIGHT } else { TEXT }))
                .child(label),
        )
        .child(if selected {
            gpui_kit::component::Icon::new(IconName::Check)
                .size(px(15.))
                .text_color(rgb(ACCENT))
                .into_any_element()
        } else {
            div().size(px(15.)).into_any_element()
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            if input {
                this.select_input_device(click_value.clone(), cx);
            } else {
                this.select_output_device(click_value.clone(), cx);
            }
        }))
        .into_any_element()
}

use super::*;

impl App {
    pub(super) fn render_login(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child("Вход по токену Discord"),
            )
            .child(
                Input::new(&self.token_input)
                    .appearance(true)
                    .bordered(true),
            )
            .child(self.render_primary_button("Войти", connecting, cx))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(FAINT))
                    .child("Браузер: DevTools (F12) → Application → Local Storage → token"),
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
            .font_family("Inter Variable")
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
        let text = if busy {
            "Подключение…"
        } else {
            label
        };
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
}

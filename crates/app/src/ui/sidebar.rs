use super::widgets::*;
use super::*;

impl App {
    pub(super) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match &self.selected_guild {
            Some(id) => self
                .state
                .guild(id)
                .map(|g| g.name.clone())
                .unwrap_or_default(),
            None => "Direct Messages".to_string(),
        };

        let channels: Vec<Channel> = match &self.selected_guild {
            Some(id) => self.state.text_channels(id).into_iter().cloned().collect(),
            None => {
                let mut channels: Vec<Channel> = self
                    .state
                    .channels
                    .iter()
                    .filter(|c| c.kind == Channel::DM || c.kind == Channel::GROUP_DM)
                    .cloned()
                    .collect();
                channels.sort_by_key(|c| {
                    std::cmp::Reverse(c.last_message_id.as_deref().map(snowflake).unwrap_or(0))
                });
                channels
            }
        };

        let has_channels = !channels.is_empty();
        let mut list = div().flex().flex_col().gap(px(1.)).p(px(8.));
        if !has_channels {
            let label = if self.selected_guild.is_some() {
                "No channels"
            } else {
                "No direct messages"
            };
            list = list.child(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(rgb(FAINT))
                    .child(label),
            );
        }
        for channel in channels {
            let selected = self.selected_channel.as_ref() == Some(&channel.id);
            let id = channel.id.clone();
            let (unread, mentions) = self.channel_unread(&channel);
            let mut row = div()
                .id(gpui_kit::SharedString::from(format!(
                    "channel-{}",
                    channel.id
                )))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.))
                .px(px(8.))
                .py(px(5.))
                .rounded(px(4.))
                .text_size(px(14.))
                .text_color(rgb(if selected { 0xffffff } else { MUTED }))
                .cursor_pointer()
                .bg(if selected { rgb(ACTIVE) } else { rgb(SIDEBAR) })
                .hover(|style| style.bg(rgb(HOVER)))
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
                            0xf2f3f5
                        } else {
                            MUTED
                        }))
                        .child(format!("#{}", self.state.channel_display_name(&channel))),
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

        div()
            .w(px(240.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(SIDEBAR))
            .child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .border_b(px(1.))
                    .border_color(rgb(RAIL))
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xf2f3f5))
                    .truncate()
                    .child(title),
            )
            .child(
                div()
                    .id("channels-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(list),
            )
    }
}

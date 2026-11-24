use super::*;
use chrono::{Datelike, TimeZone};

pub(super) fn unread_dot() -> impl IntoElement {
    div().size(px(8.)).rounded(px(4.)).bg(rgb(0xf2f3f5))
}

pub(super) fn mention_badge(count: u32) -> impl IntoElement {
    div()
        .min_w(px(16.))
        .h(px(16.))
        .px(px(4.))
        .rounded(px(8.))
        .bg(rgb(DANGER))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(0xffffff))
        .child(if count > 99 {
            "99+".to_string()
        } else {
            count.to_string()
        })
}

pub(super) fn divider(label: &str, color: u32) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .py(px(6.))
        .child(div().flex_1().h(px(1.)).bg(rgb(RAIL)))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(color))
                .child(label.to_string()),
        )
        .child(div().flex_1().h(px(1.)).bg(rgb(RAIL)))
}

pub(super) fn parse_timestamp(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.with_timezone(&Local))
}

pub(super) fn day_label(date: NaiveDate) -> String {
    let today = Local::now().date_naive();
    if date == today {
        "Today".to_string()
    } else if Some(date) == today.pred_opt() {
        "Yesterday".to_string()
    } else {
        date.format("%d %B %Y").to_string()
    }
}

pub(super) fn snippet(text: &str, limit: usize) -> String {
    let single_line = text.replace('\n', " ");
    if single_line.chars().count() <= limit {
        single_line
    } else {
        format!("{}…", single_line.chars().take(limit).collect::<String>())
    }
}

pub(super) fn format_time(timestamp: &str) -> String {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

pub(super) fn initials(name: &str) -> String {
    name.split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .collect::<String>()
        .to_uppercase()
        .chars()
        .take(2)
        .collect()
}

pub(super) fn avatar(user: &User) -> impl IntoElement {
    div()
        .size(px(40.))
        .flex_none()
        .rounded(px(20.))
        .overflow_hidden()
        .child(gpui_kit::img(avatar_url(user)).size(px(40.)))
}

pub(super) fn avatar_url(user: &User) -> String {
    match &user.avatar {
        Some(hash) if hash.starts_with("a_") => format!(
            "https://cdn.discordapp.com/avatars/{}/{hash}.gif?size=64",
            user.id
        ),
        Some(hash) => format!(
            "https://cdn.discordapp.com/avatars/{}/{hash}.png?size=64",
            user.id
        ),
        None => {
            let index = user.discriminator.parse::<u32>().unwrap_or(0) % 5;
            format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
        }
    }
}

pub(super) fn guild_icon_url(guild_id: &str, hash: &str) -> String {
    if hash.starts_with("a_") {
        format!("https://cdn.discordapp.com/icons/{guild_id}/{hash}.gif?size=96")
    } else {
        format!("https://cdn.discordapp.com/icons/{guild_id}/{hash}.png?size=96")
    }
}

pub(super) fn is_image(content_type: &Option<String>, filename: &str) -> bool {
    if let Some(content_type) = content_type {
        return content_type.starts_with("image/");
    }
    let extension = filename.rsplit('.').next().unwrap_or_default();
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp"
    )
}

pub(super) fn presence_dot(status: &str) -> impl IntoElement {
    let color = match status {
        "online" => ONLINE,
        "idle" => IDLE,
        "dnd" => DND,
        _ => FAINT,
    };
    div()
        .size(px(11.))
        .rounded(px(6.))
        .bg(rgb(color))
        .border_2()
        .border_color(rgb(SIDEBAR))
}

pub(super) fn user_avatar(
    user: Option<&User>,
    fallback_name: &str,
    presence: &str,
) -> impl IntoElement {
    let visual = match user {
        Some(user) => gpui_kit::img(avatar_url(user))
            .size(px(32.))
            .into_any_element(),
        None => div()
            .size(px(32.))
            .rounded(px(16.))
            .bg(rgb(ACCENT))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0xffffff))
            .child(initials(fallback_name))
            .into_any_element(),
    };

    div()
        .relative()
        .flex_none()
        .child(
            div()
                .size(px(32.))
                .rounded(px(16.))
                .overflow_hidden()
                .child(visual),
        )
        .child(
            div()
                .absolute()
                .bottom(px(-1.))
                .right(px(-1.))
                .child(presence_dot(presence)),
        )
}

pub(super) fn dm_user(state: &State, channel: &Channel) -> Option<User> {
    channel.recipients.first().cloned().or_else(|| {
        channel
            .recipient_ids
            .iter()
            .find_map(|id| state.user(id).cloned())
    })
}

pub(super) fn snowflake_time(id: &str) -> Option<DateTime<Local>> {
    let millis = (snowflake(id) >> 22) as i64 + 1_420_070_400_000;
    Local.timestamp_millis_opt(millis).single()
}

pub(super) fn short_time(time: DateTime<Local>) -> String {
    let now = Local::now();
    if time.date_naive() == now.date_naive() {
        time.format("%H:%M").to_string()
    } else if time.date_naive().year() == now.date_naive().year() {
        time.format("%d %b").to_string()
    } else {
        time.format("%d.%m.%y").to_string()
    }
}

pub(super) fn empty_state(icon: IconName, title: &str, hint: &str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .p(px(24.))
        .child(
            gpui_kit::component::Icon::new(icon)
                .size(px(28.))
                .text_color(rgb(FAINT)),
        )
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(MUTED))
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(FAINT))
                .child(hint.to_string()),
        )
}

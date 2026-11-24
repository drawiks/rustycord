use super::*;

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

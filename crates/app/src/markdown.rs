use gpui_kit::{
    FontStyle, FontWeight, HighlightStyle, StrikethroughStyle, StyledText, TextStyle, px, rgb,
};
use rustycord_discord::State;

use crate::theme::*;

#[derive(Clone, Copy, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    dim: bool,
}

pub fn build(content: &str, state: &State) -> StyledText {
    let spans = spans(&resolve(content, state));
    let joined: String = spans.iter().map(|(_, text)| text.as_str()).collect();

    let base = TextStyle {
        color: rgb(TEXT).into(),
        font_size: px(14.).into(),
        line_height: px(20.).into(),
        ..TextStyle::default()
    };

    let runs = spans
        .iter()
        .map(|(style, text)| {
            let highlight = HighlightStyle {
                font_weight: style.bold.then_some(FontWeight::BOLD),
                font_style: style.italic.then_some(FontStyle::Italic),
                strikethrough: style.strike.then_some(StrikethroughStyle {
                    thickness: px(1.),
                    color: None,
                }),
                background_color: style.code.then(|| rgb(CODE_BG).into()),
                color: if style.code {
                    Some(rgb(CODE_TEXT).into())
                } else if style.dim {
                    Some(rgb(MUTED).into())
                } else {
                    None
                },
                ..Default::default()
            };
            base.clone().highlight(highlight).to_run(text.len())
        })
        .collect();

    StyledText::new(joined).with_runs(runs)
}

fn spans(resolved: &str) -> Vec<(Style, String)> {
    let mut spans = Vec::new();
    for (index, part) in resolved.split("```").enumerate() {
        if index % 2 == 0 {
            lines(part, &mut spans);
        } else {
            let body = part.split_once('\n').map(|(_, body)| body).unwrap_or(part);
            push(
                &mut spans,
                Style {
                    code: true,
                    ..Default::default()
                },
                body,
            );
        }
    }
    spans
}

fn lines(text: &str, spans: &mut Vec<(Style, String)>) {
    for line in text.split_inclusive('\n') {
        let (body, newline) = match line.strip_suffix('\n') {
            Some(body) => (body, true),
            None => (line, false),
        };
        if let Some(quote) = body.strip_prefix("> ") {
            push(
                spans,
                Style {
                    dim: true,
                    ..Default::default()
                },
                "▏ ",
            );
            push(
                spans,
                Style {
                    dim: true,
                    ..Default::default()
                },
                quote,
            );
        } else {
            inline(body, spans);
        }
        if newline {
            push(spans, Style::default(), "\n");
        }
    }
}

fn inline(text: &str, spans: &mut Vec<(Style, String)>) {
    let mut style = Style::default();
    let mut buffer = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    buffer.push(escaped);
                }
            }
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                flush(&mut buffer, &style, spans);
                style.bold = !style.bold;
            }
            '~' if chars.peek() == Some(&'~') => {
                chars.next();
                flush(&mut buffer, &style, spans);
                style.strike = !style.strike;
            }
            '_' if chars.peek() == Some(&'_') => {
                chars.next();
                flush(&mut buffer, &style, spans);
            }
            '*' | '_' => {
                flush(&mut buffer, &style, spans);
                style.italic = !style.italic;
            }
            '`' => {
                flush(&mut buffer, &style, spans);
                style.code = !style.code;
            }
            _ => buffer.push(c),
        }
    }
    flush(&mut buffer, &style, spans);
}

fn flush(buffer: &mut String, style: &Style, spans: &mut Vec<(Style, String)>) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    push(spans, *style, &text);
}

fn push(spans: &mut Vec<(Style, String)>, style: Style, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some((last_style, last_text)) = spans.last_mut()
        && *last_style == style
    {
        last_text.push_str(text);
        return;
    }
    spans.push((style, text.to_string()));
}

fn resolve(text: &str, state: &State) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        output.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after.find('>') else {
            output.push_str(after);
            return output;
        };
        let token = &after[1..end];
        let raw = &after[..=end];

        let replacement = if token.starts_with("@&") {
            "@role".to_string()
        } else if let Some(id) = token.strip_prefix("@!").or_else(|| token.strip_prefix('@')) {
            state
                .user(id)
                .map(|user| format!("@{}", user.display_name()))
                .unwrap_or_else(|| format!("@{id}"))
        } else if let Some(id) = token.strip_prefix('#') {
            state
                .channel(id)
                .map(|channel| format!("#{}", channel.display_name()))
                .unwrap_or_else(|| format!("#{id}"))
        } else if let Some(rest) = token.strip_prefix("a:").or_else(|| token.strip_prefix(':')) {
            match rest.split(':').next() {
                Some(name) if !name.is_empty() => format!(":{name}:"),
                _ => raw.to_string(),
            }
        } else {
            raw.to_string()
        };

        output.push_str(&replacement);
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustycord_discord::User;

    fn text(spans: &[(Style, String)]) -> String {
        spans.iter().map(|(_, text)| text.as_str()).collect()
    }

    fn style_of(spans: &[(Style, String)], needle: &str) -> Style {
        spans
            .iter()
            .find(|(_, text)| text.contains(needle))
            .map(|(style, _)| *style)
            .expect("span")
    }

    #[test]
    fn strips_inline_markers() {
        let out = spans("**bold** and *italic* and ~~gone~~");
        assert_eq!(text(&out), "bold and italic and gone");
        assert!(style_of(&out, "bold").bold);
        assert!(style_of(&out, "italic").italic);
        assert!(style_of(&out, "gone").strike);
    }

    #[test]
    fn handles_code_blocks_and_inline_code() {
        let out = spans("run `cargo test`\n```rust\nfn main() {}\n```\n");
        assert_eq!(text(&out), "run cargo test\nfn main() {}\n\n");
        assert!(style_of(&out, "cargo test").code);
        assert!(style_of(&out, "fn main").code);
    }

    #[test]
    fn resolves_mentions_and_emoji() {
        let mut state = State::default();
        state.users.insert(
            "42".to_string(),
            User {
                id: "42".to_string(),
                username: "alice".to_string(),
                ..Default::default()
            },
        );
        let resolved = resolve("hi <@42> <@!42> <:wave:123> <@999>", &state);
        assert_eq!(resolved, "hi @alice @alice :wave: @999");
    }

    #[test]
    fn keeps_unknown_tags() {
        let state = State::default();
        assert_eq!(resolve("a <b> c", &state), "a <b> c");
    }
}

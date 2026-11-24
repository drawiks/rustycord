use std::ops::Range;

use gpui_kit::{
    FontStyle, FontWeight, HighlightStyle, SharedString, StrikethroughStyle, StyledText, TextRun,
    TextStyle, px, rgb,
};
use rustycord_discord::{Id, State};

use crate::theme::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Url(String),
    User(Id),
    Channel(Id),
}

#[derive(Clone)]
pub struct Layout {
    pub text: SharedString,
    pub runs: Vec<TextRun>,
    pub targets: Vec<(Range<usize>, Target)>,
}

impl Layout {
    pub fn styled(&self) -> StyledText {
        StyledText::new(self.text.clone()).with_runs(self.runs.clone())
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    dim: bool,
    mention: bool,
    link: bool,
}

struct Span {
    style: Style,
    text: String,
    target: Option<Target>,
}

pub fn layout(content: &str, state: &State) -> Layout {
    let spans = spans(content, state);
    let base = TextStyle {
        color: rgb(TEXT).into(),
        font_family: "Inter Variable".into(),
        font_size: px(14.).into(),
        line_height: px(20.).into(),
        ..TextStyle::default()
    };

    let mut joined = String::new();
    let mut targets = Vec::new();
    let mut runs = Vec::new();
    for span in spans {
        let start = joined.len();
        joined.push_str(&span.text);
        let end = joined.len();
        if let Some(target) = &span.target {
            targets.push((start..end, target.clone()));
        }

        let highlight = HighlightStyle {
            font_weight: span.style.bold.then_some(FontWeight::BOLD),
            font_style: span.style.italic.then_some(FontStyle::Italic),
            strikethrough: span.style.strike.then_some(StrikethroughStyle {
                thickness: px(1.),
                color: None,
            }),
            background_color: if span.style.code {
                Some(rgb(CODE_BG).into())
            } else if span.style.mention {
                Some(rgb(0x3c4270).into())
            } else {
                None
            },
            color: if span.style.code {
                Some(rgb(CODE_TEXT).into())
            } else if span.style.link || span.style.mention {
                Some(rgb(LINK).into())
            } else if span.style.dim {
                Some(rgb(MUTED).into())
            } else {
                None
            },
            ..Default::default()
        };
        let mut run_style = base.clone().highlight(highlight);
        if span.style.code {
            run_style.font_family = "JetBrains Mono".into();
        }
        runs.push(run_style.to_run(span.text.len()));
    }

    Layout {
        text: joined.into(),
        runs,
        targets,
    }
}

fn spans(content: &str, state: &State) -> Vec<Span> {
    let mut spans = Vec::new();
    for (index, part) in content.split("```").enumerate() {
        if index % 2 == 0 {
            lines(part, state, &mut spans);
        } else {
            let body = part.split_once('\n').map(|(_, body)| body).unwrap_or(part);
            push(
                &mut spans,
                Style {
                    code: true,
                    ..Default::default()
                },
                body,
                None,
            );
        }
    }
    spans
}

fn lines(text: &str, state: &State, spans: &mut Vec<Span>) {
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
                None,
            );
            push(
                spans,
                Style {
                    dim: true,
                    ..Default::default()
                },
                quote,
                None,
            );
        } else {
            inline(body, state, spans);
        }
        if newline {
            push(spans, Style::default(), "\n", None);
        }
    }
}

fn inline(text: &str, state: &State, spans: &mut Vec<Span>) {
    let chars: Vec<char> = text.chars().collect();
    let mut style = Style::default();
    let mut buffer = String::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                if let Some(escaped) = chars.get(i + 1) {
                    buffer.push(*escaped);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '*' if chars.get(i + 1) == Some(&'*') => {
                flush(&mut buffer, style, spans);
                style.bold = !style.bold;
                i += 2;
            }
            '~' if chars.get(i + 1) == Some(&'~') => {
                flush(&mut buffer, style, spans);
                style.strike = !style.strike;
                i += 2;
            }
            '_' if chars.get(i + 1) == Some(&'_') => {
                flush(&mut buffer, style, spans);
                i += 2;
            }
            '*' | '_' => {
                flush(&mut buffer, style, spans);
                style.italic = !style.italic;
                i += 1;
            }
            '`' => {
                flush(&mut buffer, style, spans);
                style.code = !style.code;
                i += 1;
            }
            '<' if !style.code => {
                if let Some((display, target, next)) = token(&chars, i, state) {
                    flush(&mut buffer, style, spans);
                    let mention = Style {
                        mention: true,
                        ..style
                    };
                    push(spans, mention, &display, target);
                    i = next;
                } else {
                    buffer.push(c);
                    i += 1;
                }
            }
            'h' if !style.code && is_url_start(&chars[i..]) => {
                flush(&mut buffer, style, spans);
                let mut j = i;
                while j < chars.len()
                    && !chars[j].is_whitespace()
                    && !matches!(chars[j], '>' | '"' | '<')
                {
                    j += 1;
                }
                let raw: String = chars[i..j].iter().collect();
                let url = raw
                    .trim_end_matches([',', '.', ';', ':', '!', '?', ')'])
                    .to_string();
                let consumed = url.chars().count();
                let link = Style {
                    link: true,
                    ..style
                };
                push(spans, link, &url, Some(Target::Url(url.clone())));
                i += consumed;
            }
            _ => {
                buffer.push(c);
                i += 1;
            }
        }
    }
    flush(&mut buffer, style, spans);
}

fn token(chars: &[char], start: usize, state: &State) -> Option<(String, Option<Target>, usize)> {
    let end = start + chars[start..].iter().position(|c| *c == '>')?;
    if end - start > 96 {
        return None;
    }
    let inner: String = chars[start + 1..end].iter().collect();

    if let Some(id) = inner.strip_prefix("@&") {
        let name = state
            .role(id)
            .map(|role| format!("@{}", role.name))
            .unwrap_or_else(|| "@role".to_string());
        return Some((name, None, end + 1));
    }

    if let Some(id) = inner.strip_prefix("@!").or_else(|| inner.strip_prefix('@')) {
        let name = state
            .user(id)
            .map(|user| format!("@{}", user.display_name()))
            .unwrap_or_else(|| format!("@{id}"));
        return Some((name, Some(Target::User(id.to_string())), end + 1));
    }

    if let Some(id) = inner.strip_prefix('#') {
        let name = state
            .channel(id)
            .map(|channel| format!("#{}", state.channel_display_name(channel)))
            .unwrap_or_else(|| format!("#{id}"));
        return Some((name, Some(Target::Channel(id.to_string())), end + 1));
    }

    let rest = inner
        .strip_prefix("a:")
        .or_else(|| inner.strip_prefix(':'))?;
    let name = rest.split(':').next().filter(|name| !name.is_empty())?;
    Some((format!(":{name}:"), None, end + 1))
}

fn is_url_start(chars: &[char]) -> bool {
    starts_with(chars, "https://") || starts_with(chars, "http://")
}

fn starts_with(chars: &[char], pattern: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    chars.len() >= pattern.len() && chars[..pattern.len()] == pattern[..]
}

fn flush(buffer: &mut String, style: Style, spans: &mut Vec<Span>) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    push(spans, style, &text, None);
}

fn push(spans: &mut Vec<Span>, style: Style, text: &str, target: Option<Target>) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut()
        && last.style == style
        && last.target == target
    {
        last.text.push_str(text);
        return;
    }
    spans.push(Span {
        style,
        text: text.to_string(),
        target,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustycord_discord::User;

    fn joined(spans: &[Span]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    fn style_of<'a>(spans: &'a [Span], needle: &str) -> &'a Style {
        &spans
            .iter()
            .find(|span| span.text.contains(needle))
            .expect("span")
            .style
    }

    #[test]
    fn strips_inline_markers() {
        let state = State::default();
        let spans = spans("**bold** and *italic* and ~~gone~~", &state);
        assert_eq!(joined(&spans), "bold and italic and gone");
        assert!(style_of(&spans, "bold").bold);
        assert!(style_of(&spans, "italic").italic);
        assert!(style_of(&spans, "gone").strike);
    }

    #[test]
    fn handles_code_blocks_and_inline_code() {
        let state = State::default();
        let spans = spans("run `cargo test`\n```rust\nfn main() {}\n```\n", &state);
        assert_eq!(joined(&spans), "run cargo test\nfn main() {}\n\n");
        assert!(style_of(&spans, "cargo test").code);
        assert!(style_of(&spans, "fn main").code);
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
        let spans = spans("hi <@42> <@!42> <:wave:123> <@999>", &state);
        assert_eq!(joined(&spans), "hi @alice @alice :wave: @999");
        let mention = spans
            .iter()
            .find(|span| span.text == "@alice")
            .expect("mention span");
        assert_eq!(mention.target, Some(Target::User("42".to_string())));
    }

    #[test]
    fn detects_urls() {
        let state = State::default();
        let spans = spans("see https://example.com/a?b=1).", &state);
        assert_eq!(joined(&spans), "see https://example.com/a?b=1).");
        let link = spans
            .iter()
            .find(|span| span.style.link)
            .expect("link span");
        assert_eq!(link.text, "https://example.com/a?b=1");
        assert_eq!(
            link.target,
            Some(Target::Url("https://example.com/a?b=1".to_string()))
        );
    }

    #[test]
    fn keeps_unknown_tags() {
        let state = State::default();
        assert_eq!(joined(&spans("a <b> c", &state)), "a <b> c");
    }
}

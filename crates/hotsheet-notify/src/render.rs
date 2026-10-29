//! Degrade a [`Message`] to what one notifier can carry (`docs/32-notifier-plugins.md`
//! §32.4): pick the richest accepted body format, fold or cap the title, drop attachments
//! the channel cannot carry (saying so in the body), and truncate on a UTF-8 boundary.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use crate::capability::{BodyFormat, Capabilities};
use crate::message::{Attachment, EventKind, Message, Severity, TicketFacts};

/// Appended when a title or body was shortened.
pub const TRUNCATION_MARKER: &str = "… (truncated)";

/// The message as one notifier will send it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rendered {
    pub event: EventKind,
    pub severity: Severity,
    /// `None` when the notifier has no title field; the title is then the body's first line.
    pub title: Option<String>,
    pub body: String,
    pub format: BodyFormat,
    /// A plain-text alternative of an HTML body (for multipart email); `None` otherwise.
    pub plain_alternative: Option<String>,
    /// Present only when the notifier supports a click URL; otherwise it is in the body.
    pub url: Option<String>,
    pub attachments: Vec<Attachment>,
    pub project: Option<String>,
    pub project_name: Option<String>,
    pub ticket: Option<TicketFacts>,
    pub truncated: bool,
    pub omitted_attachments: usize,
}

pub fn render(message: &Message, caps: &Capabilities) -> Rendered {
    let format = choose_format(caps);
    let (attachments, omitted) = fit_attachments(&message.attachments, caps);
    let mut markdown = message.body.trim().to_string();
    if !omitted.is_empty() && !caps.audio {
        let names = omitted.join(", ");
        let noun = if omitted.len() == 1 {
            "attachment"
        } else {
            "attachments"
        };
        push_paragraph(
            &mut markdown,
            &format!("_{} {noun} omitted: {}_", omitted.len(), escape_md(&names)),
        );
    }
    let url = match &message.url {
        Some(url) if caps.click_url => Some(url.clone()),
        Some(url) if !caps.audio => {
            push_paragraph(&mut markdown, &format!("[Open in Hot Sheet](<{url}>)"));
            None
        }
        _ => None,
    };
    let mut truncated = false;
    let title = if caps.max_title_bytes > 0 {
        let (title, cut) = truncate_utf8(message.title.trim(), caps.max_title_bytes);
        truncated |= cut;
        Some(title)
    } else {
        let title = message.title.trim();
        if caps.audio {
            markdown = if markdown.is_empty() {
                title.to_string()
            } else {
                format!("{}. {markdown}", title.trim_end_matches('.'))
            };
        } else if !title.is_empty() {
            markdown = if markdown.is_empty() {
                format!("**{}**", escape_md(title))
            } else {
                format!("**{}**\n\n{markdown}", escape_md(title))
            };
        }
        None
    };
    let (body, cut) = render_body(&markdown, format, caps.audio, caps.max_body_bytes);
    truncated |= cut;
    let plain_alternative = (format == BodyFormat::Html)
        .then(|| render_body(&markdown, BodyFormat::Plain, false, caps.max_body_bytes).0);
    Rendered {
        event: message.event,
        severity: message.severity,
        title,
        body,
        format,
        plain_alternative,
        url,
        attachments,
        project: message.project.clone(),
        project_name: message.project_name.clone(),
        ticket: message.ticket.clone(),
        truncated,
        omitted_attachments: omitted.len(),
    }
}

fn choose_format(caps: &Capabilities) -> BodyFormat {
    if caps.audio {
        return BodyFormat::Plain;
    }
    let mut formats = caps.formats.clone();
    formats.sort();
    formats.first().copied().unwrap_or(BodyFormat::Plain)
}

fn fit_attachments(all: &[Attachment], caps: &Capabilities) -> (Vec<Attachment>, Vec<String>) {
    let limits = &caps.attachments;
    let mut kept = Vec::new();
    let mut omitted = Vec::new();
    let mut total = 0usize;
    for attachment in all {
        let size = attachment.bytes.len();
        let fits = kept.len() < limits.max_count
            && (!limits.images_only || attachment.is_image())
            && (limits.max_bytes == 0 || size <= limits.max_bytes)
            && (limits.max_total_bytes == 0 || total + size <= limits.max_total_bytes);
        if fits {
            total += size;
            kept.push(attachment.clone());
        } else {
            omitted.push(attachment.name.clone());
        }
    }
    (kept, omitted)
}

fn render_body(markdown: &str, format: BodyFormat, audio: bool, max: usize) -> (String, bool) {
    match format {
        BodyFormat::Markdown => truncate_utf8(markdown, max),
        BodyFormat::Plain => truncate_utf8(&markdown_to_plain(markdown, audio), max),
        BodyFormat::Html => {
            let html = markdown_to_html(markdown);
            if max == 0 || html.len() <= max {
                return (html, false);
            }
            // Cut the source rather than the HTML so tags stay balanced; shrink until the
            // rendering fits, then fall back to escaped text.
            let mut budget = markdown.len().min(max);
            for _ in 0..8 {
                let (cut, _) = truncate_utf8(markdown, budget);
                let html = markdown_to_html(&cut);
                if html.len() <= max {
                    return (html, true);
                }
                budget = budget.saturating_sub(html.len() - max + TRUNCATION_MARKER.len());
            }
            let (text, _) = truncate_utf8(&markdown_to_plain(markdown, false), max / 2);
            (format!("<p>{}</p>", escape_html(&text)), true)
        }
    }
}

/// Shorten to at most `max` bytes on a char boundary, ending with [`TRUNCATION_MARKER`]
/// when cut. `max == 0` means unlimited.
pub fn truncate_utf8(text: &str, max: usize) -> (String, bool) {
    if max == 0 || text.len() <= max {
        return (text.to_string(), false);
    }
    if max <= TRUNCATION_MARKER.len() {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        return (text[..end].to_string(), true);
    }
    let mut end = max - TRUNCATION_MARKER.len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (
        format!("{}{TRUNCATION_MARKER}", text[..end].trim_end()),
        true,
    )
}

/// CommonMark to HTML with raw HTML neutralized: notification text may quote ticket
/// content, which must never inject markup into an email.
pub fn markdown_to_html(markdown: &str) -> String {
    let parser =
        Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH).map(|event| match event {
            Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
            other => other,
        });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    html
}

/// CommonMark to readable plain text. Links keep their URL in parentheses unless
/// `audio`, which also drops code fences' decoration.
pub fn markdown_to_plain(markdown: &str, audio: bool) -> String {
    let mut out = String::new();
    let mut link: Option<String> = None;
    let mut list_depth = 0usize;
    for event in Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Text(text) | Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) => {
                out.push_str(&text)
            }
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => out.push('\n'),
            Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => {
                if let Some(url) = link.take() {
                    if !audio && !out.ends_with(url.as_str()) {
                        out.push_str(&format!(" ({url})"));
                    }
                }
            }
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                end_block(&mut out);
            }
            Event::Start(Tag::Item) => {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&"  ".repeat(list_depth.saturating_sub(1)));
                out.push_str("- ");
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock) => {
                if list_depth == 0 {
                    end_block(&mut out);
                }
            }
            Event::End(TagEnd::Item) => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Event::Rule => end_block(&mut out),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn end_block(out: &mut String) {
    let trimmed = out.trim_end_matches('\n').len();
    out.truncate(trimmed);
    if !out.is_empty() {
        out.push_str("\n\n");
    }
}

fn push_paragraph(markdown: &mut String, paragraph: &str) {
    if !markdown.is_empty() {
        markdown.push_str("\n\n");
    }
    markdown.push_str(paragraph);
}

fn escape_md(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '_' | '`' | '[' | ']' | '<' | '>' | '\\' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::AttachmentLimits;

    fn caps(formats: Vec<BodyFormat>) -> Capabilities {
        Capabilities {
            formats,
            max_title_bytes: 100,
            max_body_bytes: 4096,
            attachments: AttachmentLimits::NONE,
            priority: false,
            click_url: false,
            audio: false,
        }
    }

    fn message() -> Message {
        let mut m = Message::new(EventKind::TicketBlocked, "HS2-1 is blocked");
        m.body = "Waiting on **review** of [the PR](https://example.test/pr).".into();
        m.url = Some("https://hotsheet.test/t/HS2-1".into());
        m
    }

    #[test]
    fn picks_richest_format_markdown_then_html_then_plain() {
        let m = message();
        let r = render(&m, &caps(vec![BodyFormat::Plain, BodyFormat::Markdown]));
        assert_eq!(r.format, BodyFormat::Markdown);
        assert!(r.body.contains("**review**"));
        let r = render(&m, &caps(vec![BodyFormat::Plain, BodyFormat::Html]));
        assert_eq!(r.format, BodyFormat::Html);
        assert!(r.body.contains("<strong>review</strong>"), "{}", r.body);
        let r = render(&m, &caps(vec![BodyFormat::Plain]));
        assert_eq!(r.format, BodyFormat::Plain);
        assert!(
            r.body
                .starts_with("Waiting on review of the PR (https://example.test/pr).")
        );
    }

    #[test]
    fn url_goes_to_click_field_or_into_the_body() {
        let m = message();
        let mut c = caps(vec![BodyFormat::Plain]);
        let r = render(&m, &c);
        assert_eq!(r.url, None);
        assert!(
            r.body
                .ends_with("Open in Hot Sheet (https://hotsheet.test/t/HS2-1)")
        );
        c.click_url = true;
        let r = render(&m, &c);
        assert_eq!(r.url.as_deref(), Some("https://hotsheet.test/t/HS2-1"));
        assert!(!r.body.contains("hotsheet.test"));
    }

    #[test]
    fn titleless_channels_fold_the_title_into_the_body() {
        let mut c = caps(vec![BodyFormat::Plain]);
        c.max_title_bytes = 0;
        let r = render(&message(), &c);
        assert_eq!(r.title, None);
        assert!(
            r.body.starts_with("HS2-1 is blocked\n\nWaiting on"),
            "{}",
            r.body
        );
    }

    #[test]
    fn audio_speaks_title_and_text_without_urls() {
        let mut c = caps(vec![BodyFormat::Markdown]);
        c.max_title_bytes = 0;
        c.audio = true;
        let r = render(&message(), &c);
        assert_eq!(r.format, BodyFormat::Plain);
        assert_eq!(r.body, "HS2-1 is blocked. Waiting on review of the PR.");
    }

    #[test]
    fn truncates_on_char_boundaries_with_marker() {
        let (s, cut) = truncate_utf8("ééééééééééééééééééééé", 20);
        assert!(
            cut && s.len() <= 20 && s.ends_with(TRUNCATION_MARKER),
            "{s}"
        );
        assert_eq!(truncate_utf8("short", 20), ("short".into(), false));
        assert_eq!(truncate_utf8("long text", 0), ("long text".into(), false));
        let (s, cut) = truncate_utf8("éé", 3);
        assert!(cut && s == "é");
        let mut c = caps(vec![BodyFormat::Plain]);
        c.max_body_bytes = 30;
        c.max_title_bytes = 5;
        let r = render(&message(), &c);
        assert!(r.truncated && r.body.len() <= 30 && r.title.as_deref() == Some("HS2-1"));
    }

    #[test]
    fn html_truncation_keeps_markup_balanced() {
        let mut m = message();
        m.body = (0..200).map(|i| format!("**item {i}** ")).collect();
        let mut c = caps(vec![BodyFormat::Html]);
        c.max_body_bytes = 300;
        let r = render(&m, &c);
        assert!(r.truncated && r.body.len() <= 300, "{}", r.body.len());
        assert_eq!(
            r.body.matches("<strong>").count(),
            r.body.matches("</strong>").count()
        );
    }

    #[test]
    fn html_neutralizes_raw_markup() {
        let html = markdown_to_html("hi <script>alert(1)</script>");
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn attachments_respect_count_size_total_and_image_limits() {
        let mut m = message();
        let a = |name: &str, ty: &str, n: usize| Attachment {
            name: name.into(),
            content_type: ty.into(),
            bytes: vec![0; n],
        };
        m.attachments = vec![
            a("big.png", "image/png", 50),
            a("log.txt", "text/plain", 5),
            a("ok.png", "image/png", 10),
            a("ok2.png", "image/png", 10),
            a("over-total.png", "image/png", 10),
        ];
        let mut c = caps(vec![BodyFormat::Markdown]);
        c.attachments = AttachmentLimits {
            max_count: 3,
            max_bytes: 20,
            max_total_bytes: 25,
            images_only: true,
        };
        let r = render(&m, &c);
        let kept: Vec<_> = r.attachments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(kept, ["ok.png", "ok2.png"]);
        assert_eq!(r.omitted_attachments, 3);
        assert!(
            r.body
                .contains("3 attachments omitted: big.png, log.txt, over-total.png"),
            "{}",
            r.body
        );
        let r = render(&m, &caps(vec![BodyFormat::Plain]));
        assert!(r.attachments.is_empty() && r.omitted_attachments == 5);
    }

    #[test]
    fn plain_text_keeps_list_structure() {
        let text = markdown_to_plain("# Digest\n\n- one\n- two [x](https://x.test)\n\nend", false);
        assert_eq!(text, "Digest\n\n- one\n- two x (https://x.test)\n\nend");
    }
}

//! Markdown to HTML for the `markdown` template filter.
//!
//! The text may come from the data, which a template does not control, so raw HTML in it is shown as text, and links
//! and images keep only addresses that are safe to follow. Pure string work: the browser build uses this file too.

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};

/// Longest Markdown text converted, in bytes; a longer one is cut at a character boundary.
pub const MAX_MARKDOWN_BYTES: usize = 2 * 1024 * 1024;

/// CommonMark plus tables, strikethrough and task lists, as HTML. Raw HTML is escaped; `javascript:` and other
/// unknown schemes are removed from links and images.
pub fn to_html(markdown: &str) -> String {
    let mut end = markdown.len().min(MAX_MARKDOWN_BYTES);
    while !markdown.is_char_boundary(end) {
        end -= 1;
    }
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events = Parser::new_ext(&markdown[..end], options).map(|event| match event {
        // `<script>` or `<img src=...>` typed into the text is text, not markup.
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        // A task list box as a small CSS square (an `<input>` is drawn as an empty box whether or not it is checked).
        Event::TaskListMarker(checked) => Event::Html(CowStr::Borrowed(if checked {
            "<span style=\"display:inline-block;width:0.7em;height:0.7em;margin-right:0.4em;border:1px solid #333;background:#333\"></span>"
        } else {
            "<span style=\"display:inline-block;width:0.7em;height:0.7em;margin-right:0.4em;border:1px solid #333\"></span>"
        })),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe_url(dest_url, false),
            title,
            id,
        }),
        Event::Start(Tag::Image { link_type, dest_url, title, id }) => Event::Start(Tag::Image {
            link_type,
            dest_url: safe_url(dest_url, true),
            title,
            id,
        }),
        other => other,
    });
    let mut out = String::with_capacity(markdown.len() * 3 / 2 + 16);
    html::push_html(&mut out, events);
    out
}

/// The address if its scheme is one a document should follow, else an empty one.
fn safe_url(url: CowStr<'_>, image: bool) -> CowStr<'_> {
    let lower = url.trim_start().to_ascii_lowercase();
    let scheme = lower
        .split([':', '/', '?', '#'])
        .next()
        .filter(|s| lower[s.len()..].starts_with(':'));
    let ok = match scheme {
        None => true, // relative, `#anchor`
        Some("http" | "https") => true,
        Some("mailto" | "tel") => !image,
        Some("data") => image && lower.starts_with("data:image/"),
        Some(_) => false,
    };
    if ok { url } else { CowStr::Borrowed("") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        let html = to_html("# Title\n\nSome **bold**, *italic* and `code`.\n\n- one\n- two\n");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(
            html.contains("<strong>bold</strong>")
                && html.contains("<em>italic</em>")
                && html.contains("<code>code</code>")
        );
        assert!(html.contains("<ul>") && html.contains("<li>one</li>"));
    }

    #[test]
    fn tables_strikethrough_and_task_lists() {
        let html =
            to_html("| a | b |\n|---|---|\n| 1 | 2 |\n\n~~gone~~\n\n- [x] done\n- [ ] todo\n");
        assert!(
            html.contains("<table>") && html.contains("<th>a</th>") && html.contains("<td>2</td>")
        );
        assert!(html.contains("<del>gone</del>"));
        assert_eq!(html.matches("display:inline-block").count(), 2);
        assert!(!html.contains("<input"));
    }

    #[test]
    fn raw_html_is_text() {
        let html = to_html(
            "hello <script>alert(1)</script> <img src=\"http://169.254.169.254/\">\n\n<div onclick=\"x\">y</div>",
        );
        assert!(!html.contains("<script") && !html.contains("<img") && !html.contains("<div"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn unsafe_link_schemes_are_dropped() {
        let html = to_html(
            "[a](javascript:alert(1)) [b](https://example.com/x) [c](mailto:a@b.co) [d](#top) [e](page.html) [f](vbscript:x)",
        );
        assert!(!html.contains("javascript") && !html.contains("vbscript"));
        assert!(
            html.contains("href=\"https://example.com/x\"")
                && html.contains("href=\"mailto:a@b.co\"")
        );
        assert!(html.contains("href=\"#top\"") && html.contains("href=\"page.html\""));
    }

    #[test]
    fn images_allow_data_images_but_not_other_schemes() {
        let html = to_html(
            "![x](data:image/png;base64,AAAA) ![y](data:text/html,<b>) ![z](file:///etc/passwd) ![w](mailto:a@b.co)",
        );
        assert!(html.contains("src=\"data:image/png;base64,AAAA\""));
        assert!(!html.contains("text/html") && !html.contains("file:") && !html.contains("mailto"));
    }

    #[test]
    fn a_long_text_is_cut_not_refused() {
        let big = "word ".repeat(MAX_MARKDOWN_BYTES);
        assert!(to_html(&big).len() < MAX_MARKDOWN_BYTES * 3);
    }
}

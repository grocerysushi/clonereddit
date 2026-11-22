use pulldown_cmark::{html, Event, Parser, Tag, TagEnd};

use crate::model::inline_image_key;

pub(super) fn render(source: &str, resolve_image: impl Fn(&str) -> Option<String>) -> String {
    let mut visible_links = Vec::new();
    let mut visible_images = Vec::new();
    let events = Parser::new(source).filter_map(|event| match event {
        Event::Html(text) | Event::InlineHtml(text) => Some(Event::Text(text)),
        Event::SoftBreak => Some(Event::HardBreak),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let url = inline_image_key(&dest_url).and_then(&resolve_image);
            visible_images.push(url.is_some());
            url.map(|url| {
                Event::Start(Tag::Image {
                    link_type,
                    dest_url: url.into(),
                    title,
                    id,
                })
            })
        }
        Event::End(TagEnd::Image) => visible_images.pop().unwrap_or(false).then_some(event),
        Event::Start(Tag::Link { ref dest_url, .. }) => {
            let visible = is_web_link(dest_url);
            visible_links.push(visible);
            visible.then_some(event)
        }
        Event::End(TagEnd::Link) => visible_links.pop().unwrap_or(false).then_some(event),
        _ => Some(event),
    });

    let mut output = String::new();
    html::push_html(&mut output, events);
    output
}

fn is_web_link(url: &str) -> bool {
    let Some((scheme, address)) = url.split_once("://") else {
        return false;
    };
    let host = address.split(['/', '?', '#']).next().unwrap_or_default();
    (scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
        && !host.is_empty()
        && !host.contains('@')
        && !url.contains('\\')
        && !url.chars().any(|ch| ch.is_whitespace() || ch.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_text(source: &str) -> String {
        render(source, |_| None)
    }

    #[test]
    fn renders_inline_formatting_and_web_links() {
        let source = r#"
**bold with *italics***, *italic*, and [a **bold** link](https://example.com/?x=1&y=2 "A \"note\"").
"#;
        let output = render_text(source.trim());
        assert!(output.contains("<strong>bold with <em>italics</em></strong>"));
        assert!(output.contains("<em>italic</em>"));
        assert!(output.contains(
            r#"<a href="https://example.com/?x=1&amp;y=2" title="A &quot;note&quot;">a <strong>bold</strong> link</a>"#
        ));
    }

    #[test]
    fn renders_lists_and_quotations_as_blocks() {
        let output = render_text("- one\n- *two*\n\n3. three\n4. four\n\n> quoted\n> second line");
        assert!(output.contains("<ul>\n<li>one</li>\n<li><em>two</em></li>\n</ul>"));
        assert!(output.contains("<ol start=\"3\">\n<li>three</li>\n<li>four</li>\n</ol>"));
        assert!(output.contains("<blockquote>\n<p>quoted<br />\nsecond line</p>\n</blockquote>"));
    }

    #[test]
    fn preserves_plain_line_breaks_and_inline_code() {
        let output = render_text("One line & another\n**A new line**\n\n`<b>plain</b>`");
        assert_eq!(
            output,
            "<p>One line &amp; another<br />\n<strong>A new line</strong></p>\n<p><code>&lt;b&gt;plain&lt;/b&gt;</code></p>\n"
        );
    }

    #[test]
    fn escapes_html_and_preserves_labels_without_loading_untrusted_destinations() {
        let source = r#"
<script>alert(1)</script>

<img src="https://example.test/x" onerror="alert(1)"> and **ok**.

[script](javascript:alert%281%29) [entity](javascript&#58;alert%281%29) [data](data:text/html,body) [relative](/settings) [relative protocol](//example.org) [missing host](https:///x) [userinfo](https://user@example.org) [space](<https://example.org/a b>) [encoded quote](https://example.org/path&quot;more) ![image text](https://example.org/pixel.png)
"#;
        let output = render_text(source.trim());
        assert!(output.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(output.contains("&lt;img"));
        assert!(output.contains("<strong>ok</strong>"));
        assert!(!output.contains("<script") && !output.contains("<img"));
        assert!(output
            .contains("script entity data relative relative protocol missing host userinfo space"));
        assert!(output.contains(r#"<a href="https://example.org/path%22more">encoded quote</a>"#));
        assert!(output.contains("image text"));
        assert_eq!(output.matches("<a ").count(), 1);
    }

    #[test]
    fn authorized_images_keep_their_position_and_escaped_alt_text() {
        let key = "1234567890abcdef".repeat(3);
        let source = format!("Before\n\n![Trail & path](inline-image:{key})\n\nAfter");
        let output = render(&source, |candidate| {
            (candidate == key).then(|| format!("/posts/inline-image?id=42&key={key}"))
        });
        let image = format!(
            "<img src=\"/posts/inline-image?id=42&amp;key={key}\" alt=\"Trail &amp; path\" />"
        );
        assert!(output.starts_with("<p>Before</p>\n"));
        assert!(output.contains(&image));
        assert!(output.ends_with("<p>After</p>\n"));
        assert!(!render_text(&source).contains("<img "));
    }
}

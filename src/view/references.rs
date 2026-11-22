use std::fmt::Write;

use super::escape;

pub(super) enum Segment<'a> {
    Text(&'a str),
    Reference { label: &'a str, href: String },
}

pub(super) fn segments(text: &str) -> impl Iterator<Item = Segment<'_>> {
    let mut segments = Vec::new();
    let mut offset = 0;
    let mut plain_start = 0;
    for part in text.split_inclusive(char::is_whitespace) {
        let token = part.trim_end_matches(char::is_whitespace);
        let start = token.trim_start_matches(['(', '[', '{', '<', '"', '\'', '“', '‘', '«', '‹']);
        let label = start.trim_end_matches([
            ')', ']', '}', '>', '"', '\'', '”', '’', '»', '›', ',', '.', '!', '?', ';', ':', '…',
        ]);
        if let Some(href) = destination(label) {
            let reference_start = offset + token.len() - start.len();
            if plain_start < reference_start {
                segments.push(Segment::Text(&text[plain_start..reference_start]));
            }
            segments.push(Segment::Reference { label, href });
            plain_start = reference_start + label.len();
        }
        offset += part.len();
    }
    if plain_start < text.len() {
        segments.push(Segment::Text(&text[plain_start..]));
    }
    segments.into_iter()
}

fn destination(label: &str) -> Option<String> {
    let path = label.strip_prefix('/').unwrap_or(label);
    let (kind, name) = path.split_once('/')?;
    let valid = match kind {
        "c" => {
            (2..=32).contains(&name.len())
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !name.starts_with('-')
                && !name.ends_with('-')
        }
        "u" => {
            (2..=24).contains(&name.len())
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        }
        _ => false,
    };
    valid.then(|| format!("/{}", path.to_ascii_lowercase()))
}

pub(super) fn render_text(text: &str) -> String {
    let mut html = String::with_capacity(text.len());
    for segment in segments(text) {
        match segment {
            Segment::Text(text) => html.push_str(&escape(text)),
            Segment::Reference { label, href } => {
                write!(
                    html,
                    "<a href='{href}' class='reference-link'>{}</a>",
                    escape(label)
                )
                .unwrap();
            }
        }
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_text_and_punctuation_around_canonical_references() {
        let text = "Read (‘c/Signal-Lab’),\u{a0}\"/u/NAME_42\" or /c/Rust…";
        let mut found = segments(text);
        assert!(matches!(found.next(), Some(Segment::Text("Read (‘"))));
        assert!(
            matches!(found.next(), Some(Segment::Reference { label: "c/Signal-Lab", href }) if href == "/c/signal-lab")
        );
        assert!(matches!(found.next(), Some(Segment::Text("’),\u{a0}\""))));
        assert!(
            matches!(found.next(), Some(Segment::Reference { label: "/u/NAME_42", href }) if href == "/u/name_42")
        );
        assert!(matches!(found.next(), Some(Segment::Text("\" or "))));
        assert!(
            matches!(found.next(), Some(Segment::Reference { label: "/c/Rust", href }) if href == "/c/rust")
        );
        assert!(matches!(found.next(), Some(Segment::Text("…"))));
        assert!(found.next().is_none());
    }

    #[test]
    fn leaves_urls_paths_emails_and_invalid_references_intact() {
        let text = format!(
            "https://example.test/u/name www.example.test/c/room https://example.test/?go=(u/name) bob@u/name.test u/name@example.test /docs/u/name //c/room /c/room/topics c/room?sort=top u/élan u/name-extra c/wrong_name c/-edge c/edge- u/name.rs u/q r/room u/{} c/{}",
            "u".repeat(25),
            "c".repeat(33),
        );
        let mut found = segments(&text);
        assert!(matches!(found.next(), Some(Segment::Text(plain)) if plain == text));
        assert!(found.next().is_none());
    }

    #[test]
    fn comment_markup_escapes_surrounding_text() {
        assert_eq!(
            render_text("<script> & \"u/Foo\"."),
            "&lt;script&gt; &amp; &quot;<a href='/u/foo' class='reference-link'>u/Foo</a>&quot;."
        );
    }
}

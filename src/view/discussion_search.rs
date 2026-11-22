use std::fmt::Write;

use super::{account_byline, discussion, discussion::CommentSort, escape, hidden};
use crate::model::Comment;

const MAX_QUERY_LENGTH: usize = 120;

pub(super) fn normalize_query(query: &str) -> String {
    query.trim().chars().take(MAX_QUERY_LENGTH).collect()
}

pub(super) fn render<'a>(
    post_id: u64,
    back: &str,
    sort: CommentSort,
    query: &str,
    comments: impl Iterator<Item = &'a Comment>,
) -> String {
    let (sort_field, sort_value) = sort.form_field();
    let mut html = format!(
        "<form class='discussion-search' role='search' aria-label='Discussion comments' method='get' action='/thread#comments'>{}{}<label for='discussion-comment-query'>Search comments</label><div class='discussion-search-controls'><input id='discussion-comment-query' type='search' name='comment_q' maxlength='{MAX_QUERY_LENGTH}' value='{}'><button class='button light' type='submit'>Search</button>",
        hidden("id", &post_id.to_string()),
        hidden(sort_field, sort_value),
        escape(query),
    );
    if !query.is_empty() {
        write!(
            html,
            "<a class='button discussion-search-clear' href='{}#comments'>Clear</a>",
            escape(back)
        )
        .unwrap();
    }
    html.push_str("</div></form>");
    if query.is_empty() {
        return html;
    }

    let needle = lowercase(query);
    let mut results = String::new();
    let mut count = 0;
    for comment in comments {
        if let Some(excerpt) = matching_excerpt(&discussion::body_text(comment), &needle) {
            count += 1;
            write!(
                results,
                "<li data-comment-result-id='{id}'><a href='#comment-{id}'><strong>{}</strong><span class='discussion-search-excerpt'>{excerpt}</span></a></li>",
                account_byline(&comment.author),
                id = comment.id,
            )
            .unwrap();
        }
    }
    html.push_str(
        "<section class='discussion-search-results' aria-labelledby='discussion-search-summary'>",
    );
    match count {
        0 => html.push_str("<h3 id='discussion-search-summary'>No matching comments</h3>"),
        _ => {
            let label = if count == 1 { "comment" } else { "comments" };
            write!(
                html,
                "<h3 id='discussion-search-summary'>{count} matching {label}</h3><ul>{results}</ul>"
            )
            .unwrap();
        }
    }
    html.push_str("</section>");
    html
}

fn lowercase(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

fn matching_excerpt(body: &str, needle: &str) -> Option<String> {
    let match_start = lowercase(body).find(needle)?;
    let match_end = match_start + needle.len();
    // Map the match back to whole source characters even when lowercase expands a character.
    let mut source = body.char_indices().scan(0, |lowercase_end, (start, ch)| {
        *lowercase_end += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
        Some((start, start + ch.len_utf8(), *lowercase_end))
    });
    let (start, first_end, first_lowercase_end) =
        source.find(|&(_, _, lowercase_end)| lowercase_end > match_start)?;
    let end = if first_lowercase_end >= match_end {
        first_end
    } else {
        source
            .find(|&(_, _, lowercase_end)| lowercase_end >= match_end)?
            .1
    };

    let prefix = &body[..start];
    let suffix = &body[end..];
    let excerpt_start = prefix.char_indices().rev().nth(29).map_or(0, |(i, _)| i);
    let excerpt_end = suffix
        .char_indices()
        .nth(56)
        .map_or(suffix.len(), |(i, _)| i);
    Some(format!(
        "{}{}<mark>{}</mark>{}{}",
        if excerpt_start > 0 { "…" } else { "" },
        escape(&prefix[excerpt_start..]),
        escape(&body[start..end]),
        escape(&suffix[..excerpt_end]),
        if excerpt_end < suffix.len() {
            "…"
        } else {
            ""
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::{lowercase, matching_excerpt, normalize_query};

    #[test]
    fn query_is_trimmed_and_limited_without_splitting_characters() {
        assert_eq!(normalize_query("  two words \t"), "two words");
        assert_eq!(normalize_query(&"🌱".repeat(122)), "🌱".repeat(120));
    }

    #[test]
    fn excerpt_highlights_case_insensitive_matches_with_escaped_context() {
        let body = format!("{} 🌱 İdea & <pond> {}", "a".repeat(60), "b".repeat(90));
        let excerpt = matching_excerpt(&body, &lowercase("İDEA & <POND>")).unwrap();
        assert!(excerpt.starts_with('…') && excerpt.ends_with('…'));
        assert!(excerpt.contains("🌱 <mark>İdea &amp; &lt;pond&gt;</mark>"));
        assert!(!excerpt.contains(&"a".repeat(60)));
        assert!(matching_excerpt(&body, "idea missing").is_none());
    }
}

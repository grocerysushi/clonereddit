use std::collections::BTreeMap;
use std::fmt::Write;

use super::{escape, field, hidden, search::Kind};

const FILTERS: &[(&str, &str, &str)] = &[
    ("author", "Author", "u/username"),
    ("community", "Community", "c/community"),
    ("post_flair", "Post flair", "Field notes"),
];

pub(super) fn render(
    active: &str,
    query: &BTreeMap<String, String>,
    kind: Kind,
    clear_url: &str,
    error: Option<&str>,
) -> String {
    let mut hidden_fields = hidden("view", active);
    hidden_fields.push_str(&hidden("search", "1"));
    for name in ["sort", "t"] {
        let value = field(query, name);
        if !value.is_empty() {
            hidden_fields.push_str(&hidden(name, value));
        }
    }
    if kind == Kind::Media {
        hidden_fields.push_str(&hidden("type", kind.code()));
    }

    let terms = escape(field(query, "q"));
    let (error_notice, error_attributes) = if let Some(error) = error {
        (
            format!(
                "<p class='notice setup-error' id='post-search-error' role='alert'>{}</p>",
                escape(error)
            ),
            " aria-invalid='true' aria-describedby='post-search-error post-search-help'",
        )
    } else {
        (String::new(), " aria-describedby='post-search-help'")
    };

    let mut filters = String::new();
    for &(name, label, placeholder) in FILTERS {
        let value = escape(field(query, name));
        write!(
            filters,
            "<label>{label}<input name='{name}' value='{value}' placeholder='{placeholder}'></label>"
        )
        .unwrap();
    }
    let clear = if FILTERS
        .iter()
        .any(|(name, _, _)| !field(query, name).trim().is_empty())
    {
        format!(
            "<a class='button light clear-post-search-filters' href='{}'>Clear fields</a>",
            escape(clear_url)
        )
    } else {
        String::new()
    };

    format!(
        r#"<form class='panel stack-form post-search-filters' method='get' action='/' aria-label='Post search filters'>
{hidden_fields}{error_notice}
<label>Search terms<input type='search' name='q' value='{terms}' maxlength='120' placeholder='Try "night sky"'{error_attributes}></label>
<div class='post-search-fields'>{filters}</div>
<p class='post-search-hint' id='post-search-help'>Use quotes to match an exact phrase. You can also use <code>author:</code>, <code>community:</code>, and <code>flair:</code> in search terms.</p>
<div class='form-actions'><button class='button dark' type='submit'>Apply filters</button>{clear}</div>
</form>"#
    )
}

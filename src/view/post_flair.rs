use std::fmt::Write;

use super::escape;
use crate::model::{Community, State};

pub(super) fn composer(
    state: &State,
    destination: &str,
    selection: Option<u64>,
    custom_text: Option<&str>,
) -> String {
    let mut eligible: Vec<_> = state
        .communities
        .iter()
        .filter(|item| state.can_contribute(item))
        .collect();
    if let [item] = eligible.as_slice() {
        if !item.post_flairs().is_empty() {
            let selected = selection.filter(|_| item.slug == destination);
            return community_select(item, selected);
        }
    }

    eligible.sort_by_key(|item| item.slug != destination);
    let selected_community = eligible.first().filter(|item| item.slug == destination);
    let mut groups = String::new();
    for item in &eligible {
        if item.post_flairs().is_empty() {
            continue;
        }
        let required = if item.requires_post_flair() {
            " (required)"
        } else {
            ""
        };
        let selected = selection.filter(|_| item.slug == destination);
        write!(
            groups,
            "<optgroup label='{}{required}'>{}</optgroup>",
            escape(&item.name),
            options(item, selected)
        )
        .unwrap();
    }

    let custom = custom_text
        .filter(|_| eligible.iter().any(|item| item.post_flairs().is_empty()))
        .map_or_else(String::new, custom_field);
    if groups.is_empty() {
        return custom;
    }

    let hint = selected_community
        .filter(|item| item.requires_post_flair())
        .map(|item| {
            format!(
                "{} requires post flair. If you change communities, choose flair for the new destination.",
                escape(&item.name)
            )
        })
        .unwrap_or_else(|| {
            "Choose flair from the community you’re posting to. Communities that require flair are marked."
                .to_owned()
        });
    let mut html = format!(
        "<div class='composer-flair'><label>Post flair<select name='flair_id' aria-describedby='post-flair-help'><option value=''>No flair selected</option>{groups}</select></label><small id='post-flair-help'>{hint}</small></div>"
    );
    if !custom.is_empty() {
        let open = if eligible
            .iter()
            .any(|item| item.slug == destination && item.post_flairs().is_empty())
        {
            " open"
        } else {
            ""
        };
        write!(
            html,
            "<details class='composer-flair-custom'{open}><summary>Custom flair for communities without a flair list</summary>{custom}</details>"
        )
        .unwrap();
    }
    html
}

pub(super) fn scheduled(item: &Community, selection: Option<u64>, custom_text: &str) -> String {
    if item.post_flairs().is_empty() {
        custom_field(custom_text)
    } else {
        community_select(item, selection)
    }
}

fn community_select(item: &Community, selection: Option<u64>) -> String {
    let (prompt, required) = if item.requires_post_flair() {
        ("Choose flair", " required")
    } else {
        ("No flair", "")
    };
    format!(
        "<label>Post flair<select name='flair_id'{required}><option value=''>{prompt}</option>{}</select></label>",
        options(item, selection)
    )
}

fn custom_field(value: &str) -> String {
    format!(
        "<label>Custom post flair <small>Optional</small><input name='flair' maxlength='32' value='{}' placeholder='Discussion, question, or link'></label>",
        escape(value)
    )
}

fn options(item: &Community, selection: Option<u64>) -> String {
    item.post_flairs()
        .iter()
        .map(|flair| {
            let selected = if selection == Some(flair.id) {
                " selected"
            } else {
                ""
            };
            format!(
                "<option value='{}'{selected}>{}</option>",
                flair.id,
                escape(&flair.label)
            )
        })
        .collect()
}

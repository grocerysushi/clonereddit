use std::{collections::BTreeMap, fmt::Write};

use crate::model::{Community, CommunityPostFlair, COMMUNITY_POST_FLAIR_LABEL_MAX};

use super::super::{action_fields, escape, field};
use super::tools_url;

pub(super) fn render(
    item: &Community,
    edit: &str,
    delete: &str,
    rejected: Option<(&BTreeMap<String, String>, &str)>,
) -> String {
    let base = tools_url(item, "post-flair");
    let options = item.post_flairs();
    let find_flair = |value: &str| {
        value
            .parse::<u64>()
            .ok()
            .and_then(|id| options.iter().find(|flair| flair.id == id))
    };
    let edit = if delete.is_empty() { edit } else { "" };
    let selected = find_flair(edit);
    let deleting = find_flair(delete);
    let submitted = rejected
        .filter(|(form, _)| {
            field(form, "action") == "save-post-flair" && (edit.is_empty() || selected.is_some())
        })
        .map(|(form, _)| field(form, "label"));
    let error = rejected.map_or_else(
        || {
            if (!edit.is_empty() && selected.is_none())
                || (!delete.is_empty() && deleting.is_none())
            {
                "That flair is unavailable in this community."
            } else {
                ""
            }
        },
        |(_, error)| error,
    );

    let mut html = String::new();
    if !error.is_empty() {
        write!(
            html,
            "<p class='notice setup-error' role='alert'>{}</p>",
            escape(error),
        )
        .unwrap();
    }

    let deletion_blocked = item.requires_post_flair() && options.len() == 1;
    if let Some(flair) = deleting {
        html.push_str(&confirm_delete(item, &base, flair, deletion_blocked));
    } else {
        html.push_str(&editor(item, &base, selected, submitted));
    }

    html.push_str("<section class='panel mod-section' aria-labelledby='post-flair-list-heading'><h2 id='post-flair-list-heading'>Available flair</h2>");
    if options.is_empty() {
        html.push_str("<p class='post-flair-empty muted'>There are no post flair options yet.</p>");
    } else {
        if deletion_blocked {
            html.push_str("<p class='post-flair-empty muted'>Turn off the flair requirement to delete the last option.</p>");
        }
        html.push_str("<ul class='post-flair-list' role='list'>");
        for flair in options {
            let id = flair.id;
            let label = escape(&flair.label);
            let edit_url = escape(&format!("{base}&edit={id}#post-flair-editor"));
            let delete = if deletion_blocked {
                "<button class='button light' type='button' disabled>Delete flair</button>"
                    .to_owned()
            } else {
                let url = escape(&format!("{base}&delete={id}#post-flair-delete"));
                format!("<a class='button light' href='{url}' aria-label='Delete flair {label}'>Delete flair</a>")
            };
            write!(html, "<li id='post-flair-{id}' data-flair-id='{id}'><span class='flair'>{label}</span><div class='post-flair-actions'><a class='button light' href='{edit_url}' aria-label='Edit flair {label}'>Edit flair</a>{delete}</div></li>").unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html.push_str(&policy(item, &base));
    html
}

fn policy(item: &Community, base: &str) -> String {
    let fields = action_fields("set-post-flair-policy", base, &[("community", &item.slug)]);
    let checked = if item.requires_post_flair() {
        " checked"
    } else {
        ""
    };
    let (disabled, detail) = if item.post_flairs().is_empty() {
        (" disabled", "Add a flair option to enable the requirement.")
    } else {
        (
            "",
            "When enabled, authors must choose a flair before publishing a new post.",
        )
    };
    format!("<section id='post-flair-policy' class='panel mod-section' aria-labelledby='post-flair-policy-heading'><h2 id='post-flair-policy-heading'>Publishing requirement</h2><form class='stack-form post-flair-policy' method='post' action='/action'>{fields}<div><label class='checkbox'><input type='checkbox' name='required' value='on' aria-describedby='post-flair-policy-detail'{checked}{disabled}>Require post flair</label><p id='post-flair-policy-detail' class='muted'>{detail}</p></div><button class='button dark' type='submit'{disabled}>Save requirement</button></form></section>")
}

fn editor(
    item: &Community,
    base: &str,
    flair: Option<&CommunityPostFlair>,
    submitted: Option<&str>,
) -> String {
    let label = submitted
        .or(flair.map(|flair| flair.label.as_str()))
        .unwrap_or_default();
    let (id, heading, button, cancel) = if let Some(flair) = flair {
        (
            flair.id.to_string(),
            "Edit flair",
            "Save flair",
            format!("<a class='button light' href='{}'>Cancel</a>", escape(base)),
        )
    } else {
        (String::new(), "Add flair", "Create flair", String::new())
    };
    let fields = action_fields(
        "save-post-flair",
        base,
        &[("community", &item.slug), ("id", &id)],
    );
    format!("<section id='post-flair-editor' class='panel mod-section' aria-labelledby='post-flair-editor-heading'><h2 id='post-flair-editor-heading'>{heading}</h2><form class='stack-form post-flair-editor' method='post' action='/action'>{fields}<label>Flair label<input name='label' value='{}' required maxlength='{COMMUNITY_POST_FLAIR_LABEL_MAX}'></label><div class='form-actions'><button class='button dark' type='submit'>{button}</button>{cancel}</div></form></section>", escape(label))
}

fn confirm_delete(
    item: &Community,
    base: &str,
    flair: &CommunityPostFlair,
    blocked: bool,
) -> String {
    let id = flair.id;
    let fields = action_fields(
        "delete-post-flair",
        base,
        &[("community", &item.slug), ("id", &id.to_string())],
    );
    let label = escape(&flair.label);
    let cancel = escape(&format!("{base}#post-flair-{id}"));
    let disabled = if blocked { " disabled" } else { "" };
    format!("<section id='post-flair-delete' class='panel mod-section' tabindex='-1' aria-labelledby='post-flair-delete-heading'><h2 id='post-flair-delete-heading'>Delete this flair?</h2><p class='mod-record-detail'><strong>{label}</strong></p><p class='muted'>This option will disappear from the composer. Existing posts keep their label.</p><form class='form-actions post-flair-delete-confirm' method='post' action='/action'>{fields}<button class='button dark' type='submit'{disabled}>Delete flair</button><a class='button light' href='{cancel}'>Cancel</a></form></section>")
}

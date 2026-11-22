use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{action_fields, action_form, community_owner, encode, escape, field};
use crate::model::{Community, State};
use crate::schedule_time::{display, to_local};

pub(super) fn render(state: &State, item: &Community) -> String {
    workspace(state, item, None, "")
}

pub(super) fn render_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    workspace(state, item, Some(form), error)
}

fn workspace(
    state: &State,
    item: &Community,
    form: Option<&BTreeMap<String, String>>,
    error: &str,
) -> String {
    if !community_owner(state, item) {
        return String::new();
    }

    let back = format!(
        "/moderation/tools?community={}&section=restricted-users",
        encode(&item.slug)
    );
    let current = current_restrictions(state, item, &back);
    let editor = restriction_form(state, item, &back, form, error);
    if error.is_empty() {
        format!("<div class='restricted-workspace'>{current}{editor}</div>")
    } else {
        format!("<div class='restricted-workspace'>{editor}{current}</div>")
    }
}

fn current_restrictions(state: &State, item: &Community, back: &str) -> String {
    let mut records: Vec<_> = state
        .restrictions
        .iter()
        .filter(|record| record.community == item.slug)
        .collect();
    records.sort_by_key(|record| Reverse((record.created_at, record.id)));
    let mut html = String::from(
        "<section class='panel mod-section restricted-current'><h2>Current restrictions</h2>",
    );
    if records.is_empty() {
        html.push_str(
            "<p class='muted restricted-empty'>No users are restricted in this community.</p>",
        );
    } else {
        html.push_str("<ul class='restricted-list'>");
        for record in records {
            let (kind, lift_label) = if record.kind == "mute" {
                ("Mute", "Lift mute")
            } else {
                ("Ban", "Lift ban")
            };
            let lift = action_form(
                "lift-restriction",
                back,
                &[("community", &item.slug), ("id", &record.id.to_string())],
                lift_label,
                "",
                "button light",
            );
            let reason = if record.reason.is_empty() {
                String::new()
            } else {
                format!(
                    "<p class='restricted-reason'>{}</p>",
                    escape(&record.reason)
                )
            };
            write!(
                html,
                "<li class='restricted-row'><div class='restricted-row-copy'><h3>u/{}</h3><div class='restricted-meta'><span class='flair restricted-kind' data-kind='{}'>{kind}</span><time datetime='{}Z'>{}</time></div>{reason}</div>{lift}</li>",
                escape(&record.username),
                if record.kind == "mute" { "mute" } else { "ban" },
                to_local(record.created_at, "UTC"),
                escape(&display(record.created_at, "America/Chicago"))
            )
            .unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

fn restriction_form(
    state: &State,
    item: &Community,
    back: &str,
    form: Option<&BTreeMap<String, String>>,
    error: &str,
) -> String {
    let entered = |key| form.map_or("", |values| field(values, key));
    let (ban, mute) = if entered("kind") == "mute" {
        ("", " selected")
    } else {
        (" selected", "")
    };
    let alert = if error.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='notice restricted-error' role='alert'>{}</p>",
            escape(error)
        )
    };
    let suggestions = contributor_suggestions(state, item);
    let list = if suggestions.is_empty() {
        ""
    } else {
        " list='restriction-user-options'"
    };
    format!(
        r#"<section class="panel mod-section restricted-form-panel"><h2>Restrict a user</h2>{alert}
<p id="restriction-kind-help" class="muted">A ban blocks posting, editing, and replying in this community. A mute blocks messages to its moderation inbox.</p>
<form method="post" action="/action" class="stack-form restricted-form">{fields}
<label>Username<input name="username" minlength="2" maxlength="26" value="{username}" autocomplete="off" autocapitalize="none" spellcheck="false"{list} required><small>Use a Commonline handle. The u/ prefix is optional.</small></label>{suggestions}
<label>Restriction<select name="kind" aria-describedby="restriction-kind-help"><option value="ban"{ban}>Ban</option><option value="mute"{mute}>Mute</option></select></label>
<label>Reason <small>Optional</small><textarea name="reason" rows="3" maxlength="280">{reason}</textarea></label>
<button class="button dark" type="submit">Apply restriction</button></form></section>"#,
        fields = action_fields("restrict-user", back, &[("community", &item.slug)]),
        username = escape(entered("username")),
        reason = escape(entered("reason"))
    )
}

fn contributor_suggestions(state: &State, item: &Community) -> String {
    let mut posts = BTreeSet::new();
    let mut contributors = BTreeSet::new();
    for post in state
        .posts
        .iter()
        .filter(|post| post.community == item.slug)
    {
        posts.insert(post.id);
        contributors.insert(post.author.as_str());
    }
    contributors.extend(
        state
            .comments
            .iter()
            .filter(|comment| posts.contains(&comment.post))
            .map(|comment| comment.author.as_str()),
    );
    contributors.remove(state.username.as_str());
    if contributors.is_empty() {
        return String::new();
    }
    let mut html = String::from("<datalist id='restriction-user-options'>");
    for username in contributors {
        write!(html, "<option value='{}'></option>", escape(username)).unwrap();
    }
    html.push_str("</datalist>");
    html
}

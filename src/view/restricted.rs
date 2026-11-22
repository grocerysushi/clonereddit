use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{action_fields, action_form, encode, escape, field};
use crate::model::{Community, ModPermission, State};
use crate::schedule_time::{display, now, to_local};

const DURATION_UNITS: [(&str, &str); 3] =
    [("minutes", "Minutes"), ("hours", "Hours"), ("days", "Days")];

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
    if !state.can_moderate(item, ModPermission::Restrictions) {
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
    let now = now();
    let mut records: Vec<_> = state
        .restrictions
        .iter()
        .filter(|record| record.community == item.slug && record.is_active_at(now))
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
            let expiry = restriction_expiry(record.expires_at);
            write!(
                html,
                "<li class='restricted-row'><div class='restricted-row-copy'><h3>u/{}</h3><div class='restricted-meta'><span class='flair restricted-kind' data-kind='{}'>{kind}</span><span>Applied <time datetime='{}Z'>{}</time></span></div><p class='restricted-meta'>{expiry}</p>{reason}</div>{lift}</li>",
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

pub(super) fn restriction_expiry(expires_at: Option<u64>) -> String {
    let Some(timestamp) = expires_at else {
        return "<span class='restricted-expiry'>Permanent</span>".to_owned();
    };
    let utc = to_local(timestamp, "UTC");
    let seconds = timestamp % 60;
    let readable = display(timestamp, "America/Chicago");
    let mut local = readable.rsplitn(3, ' ');
    let local = match (local.next(), local.next(), local.next()) {
        (Some(zone), Some(period), Some(clock)) => format!("{clock}:{seconds:02} {period} {zone}"),
        _ => readable,
    };
    format!(
        "<span class='restricted-expiry'>Expires <time datetime='{utc}:{seconds:02}Z'>{}</time></span>",
        escape(&local)
    )
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
    let submitted_unit = entered("duration_unit");
    let selected_unit = match submitted_unit {
        "minutes" | "hours" => submitted_unit,
        _ => "days",
    };
    let mut duration_options = String::new();
    for (value, label) in DURATION_UNITS {
        let selected = if selected_unit == value {
            " selected"
        } else {
            ""
        };
        write!(
            duration_options,
            "<option value='{value}'{selected}>{label}</option>"
        )
        .unwrap();
    }
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
<div class="restricted-duration">
<label>Duration<input type="number" name="duration" min="1" step="1" inputmode="numeric" value="{duration}" aria-describedby="restriction-duration-help"></label>
<label>Duration unit<select name="duration_unit">{duration_options}</select></label>
<small id="restriction-duration-help">Enter a whole number or leave the duration blank for a permanent restriction.</small></div>
<label>Reason <small>Optional</small><textarea name="reason" rows="3" maxlength="280">{reason}</textarea></label>
<button class="button dark" type="submit">Apply restriction</button></form></section>"#,
        fields = action_fields("restrict-user", back, &[("community", &item.slug)]),
        username = escape(entered("username")),
        duration = escape(entered("duration")),
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
            .filter(|comment| !comment.deleted && posts.contains(&comment.post))
            .map(|comment| comment.author.as_str()),
    );
    contributors.remove(state.username.as_str());
    contributors.remove(crate::model::DELETED_ACCOUNT);
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

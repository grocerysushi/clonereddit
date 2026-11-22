use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use super::{
    action_fields, action_form, content_tool_url, encode, escape, field, icon, post_flair,
};
use crate::model::{Community, ContentKind, ModPermission, ScheduledItem, State};
use crate::schedule_time::{display, to_local};

const CHICAGO: &str = "America/Chicago";
const ZONES: &[(&str, &str)] = &[
    (CHICAGO, "America/Chicago"),
    ("UTC", "UTC"),
    ("central-standard", "Central Standard Time (UTC−06:00)"),
    ("central-daylight", "Central Daylight Time (UTC−05:00)"),
];

pub(super) fn render(state: &State, item: &Community, query: &BTreeMap<String, String>) -> String {
    if !state.can_moderate(item, ModPermission::Schedule) {
        return String::new();
    }
    let requested = field(query, "edit");
    let editing = pending_item(state, item, requested);
    let editor = editing.map_or_else(|| Editor::new(field(query, "kind")), Editor::from_item);
    let error = if !requested.is_empty() && editing.is_none() {
        "This item is no longer available to edit."
    } else {
        ""
    };
    workspace(state, item, &editor, error)
}

pub(super) fn render_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    if !state.can_moderate(item, ModPermission::Schedule) {
        return String::new();
    }
    let editor = if field(form, "action") == "edit-scheduled-item" {
        match pending_item(state, item, field(form, "id")) {
            Some(entry) => Editor::from_form(form, Some(entry.id)),
            None => Editor::new("post"),
        }
    } else {
        Editor::from_form(form, None)
    };
    workspace(state, item, &editor, error)
}

fn schedule_url(item: &Community) -> String {
    format!(
        "/moderation/tools?community={}&section=scheduled",
        encode(&item.slug)
    )
}

fn pending_item<'a>(
    state: &'a State,
    item: &Community,
    reference: &str,
) -> Option<&'a ScheduledItem> {
    let id: u64 = reference.parse().ok()?;
    state
        .scheduled
        .iter()
        .find(|entry| entry.id == id && entry.community == item.slug && entry.status == "pending")
}

fn workspace(state: &State, item: &Community, editor: &Editor<'_>, error: &str) -> String {
    let back = schedule_url(item);
    let (mut upcoming, mut history): (Vec<_>, Vec<_>) = state
        .scheduled
        .iter()
        .filter(|entry| entry.community == item.slug)
        .partition(|entry| entry.status == "pending");
    upcoming.sort_by_key(|entry| (entry.publish_at, entry.id));
    history.sort_by_key(|entry| Reverse((entry.publish_at, entry.id)));

    let form = editor.render(item, &back, error);
    let upcoming = schedule_list(state, item, &back, &upcoming, true, editor.id.is_some());
    let history = schedule_list(state, item, &back, &history, false, editor.id.is_some());
    if editor.id.is_some() || !error.is_empty() {
        format!("<div class='scheduled-workspace'>{form}{upcoming}{history}</div>")
    } else {
        format!("<div class='scheduled-workspace'>{upcoming}{form}{history}</div>")
    }
}

fn schedule_list(
    state: &State,
    item: &Community,
    back: &str,
    entries: &[&ScheduledItem],
    upcoming: bool,
    editing: bool,
) -> String {
    let (class, title, empty) = if upcoming {
        (
            "scheduled-upcoming",
            "Upcoming",
            "No posts or events scheduled.",
        )
    } else {
        (
            "scheduled-history",
            "History",
            "Published and canceled items will appear here.",
        )
    };
    let create = if upcoming {
        let destination = if editing {
            format!("{back}#scheduled-editor")
        } else {
            "#scheduled-editor".into()
        };
        format!(
            "<a class='button light' href='{}'>{}<span>Schedule an item</span></a>",
            escape(&destination),
            icon("plus")
        )
    } else {
        String::new()
    };
    let mut html = format!(
        "<section class='panel mod-section {class}'><header class='scheduled-list-heading'><h2>{title}</h2>{create}</header>"
    );
    if entries.is_empty() {
        write!(html, "<p class='muted scheduled-empty'>{empty}</p>").unwrap();
    } else {
        html.push_str("<ul class='scheduled-list'>");
        for entry in entries {
            html.push_str(&schedule_row(state, item, back, entry, upcoming));
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

fn schedule_row(
    state: &State,
    item: &Community,
    back: &str,
    entry: &ScheduledItem,
    upcoming: bool,
) -> String {
    let kind = if entry.kind == "event" {
        "Event"
    } else {
        "Post"
    };
    let published = entry
        .post_id
        .filter(|id| entry.status == "published" && state.posts.iter().any(|post| post.id == *id));
    let removal = published.and_then(|id| state.content_removal(ContentKind::Post, id));
    let needs_flair = upcoming && state.scheduled_flair_blocked(entry);
    let status = match (entry.status.as_str(), removal) {
        ("published", Some(removal)) if removal.automated => {
            "<span class='flair scheduled-status'>Removed by AutoMod</span>"
        }
        ("published", Some(_)) => "<span class='flair scheduled-status'>Removed</span>",
        ("published", None) => "<span class='flair scheduled-status'>Published</span>",
        ("canceled", _) => "<span class='flair scheduled-status'>Canceled</span>",
        _ if needs_flair => "<span class='flair scheduled-status'>Needs post flair</span>",
        _ => "",
    };
    let publish_label = if upcoming && !needs_flair {
        "Publishes"
    } else {
        "Scheduled for"
    };
    let flair_notice = if needs_flair {
        "<p class='muted scheduled-flair-notice'>Edit this schedule and choose a current post flair to publish it.</p>"
    } else {
        ""
    };
    let mut times = time_row(publish_label, entry.publish_at);
    if let Some(start) = entry.event_start {
        times.push_str(&time_row("Event starts", start));
    }
    if let Some(end) = entry.event_end {
        times.push_str(&time_row("Event ends", end));
    }
    let actions = if upcoming {
        let edit = format!("{back}&edit={}#scheduled-editor", entry.id);
        let cancel = action_form(
            "cancel-scheduled-item",
            back,
            &[("community", &item.slug), ("id", &entry.id.to_string())],
            "Cancel schedule",
            "",
            "button light",
        );
        format!(
            "<div class='scheduled-actions'><a class='button light' href='{}'>Edit</a>{cancel}</div>",
            escape(&edit)
        )
    } else if removal.is_some() {
        published.and_then(|post| content_tool_url(state, &item.slug, ContentKind::Post, post))
            .map_or_else(String::new, |url| format!("<div class='scheduled-actions'><a class='subtle-link' href='{}'>Review removal</a></div>", escape(&url)))
    } else if let Some(post) = published {
        format!("<div class='scheduled-actions'><a class='subtle-link' href='/thread?id={post}'>View published post</a></div>")
    } else {
        String::new()
    };
    format!(
        "<li class='scheduled-row'><header class='scheduled-row-heading'><h3>{}</h3><div class='scheduled-labels'><span class='flair scheduled-kind'>{kind}</span>{status}</div></header><dl class='scheduled-meta'>{times}</dl>{flair_notice}{actions}</li>",
        escape(&entry.title)
    )
}

fn time_row(label: &str, timestamp: u64) -> String {
    format!(
        "<div><dt>{label}</dt><dd><time datetime='{}Z'>{}</time></dd></div>",
        to_local(timestamp, "UTC"),
        escape(&display(timestamp, CHICAGO))
    )
}

struct Editor<'a> {
    id: Option<u64>,
    kind: &'a str,
    title: &'a str,
    body: &'a str,
    url: &'a str,
    flair: &'a str,
    flair_id: Option<u64>,
    zone: &'a str,
    publish_at: String,
    event_start: String,
    event_end: String,
    sensitive: bool,
    spoiler: bool,
    brand_affiliate: bool,
}

impl<'a> Editor<'a> {
    fn new(kind: &'a str) -> Self {
        Self {
            id: None,
            kind,
            title: "",
            body: "",
            url: "",
            flair: "",
            flair_id: None,
            zone: CHICAGO,
            publish_at: String::new(),
            event_start: String::new(),
            event_end: String::new(),
            sensitive: false,
            spoiler: false,
            brand_affiliate: false,
        }
    }

    fn from_item(entry: &'a ScheduledItem) -> Self {
        let zone = known_zone(&entry.zone);
        let convert = |timestamp| to_local(timestamp, zone);
        Self {
            id: Some(entry.id),
            kind: &entry.kind,
            title: &entry.title,
            body: &entry.body,
            url: &entry.url,
            flair: &entry.flair,
            flair_id: entry.flair_id,
            zone,
            publish_at: convert(entry.publish_at),
            event_start: entry.event_start.map_or_else(String::new, convert),
            event_end: entry.event_end.map_or_else(String::new, convert),
            sensitive: entry.sensitive,
            spoiler: entry.spoiler,
            brand_affiliate: entry.brand_affiliate,
        }
    }

    fn from_form(form: &'a BTreeMap<String, String>, id: Option<u64>) -> Self {
        Self {
            id,
            kind: field(form, "kind"),
            title: field(form, "title"),
            body: field(form, "body"),
            url: field(form, "url"),
            flair: field(form, "flair"),
            flair_id: field(form, "flair_id").trim().parse().ok(),
            zone: known_zone(field(form, "zone")),
            publish_at: field(form, "publish_at").to_owned(),
            event_start: field(form, "event_start").to_owned(),
            event_end: field(form, "event_end").to_owned(),
            sensitive: field(form, "sensitive") == "on",
            spoiler: field(form, "spoiler") == "on",
            brand_affiliate: field(form, "brand_affiliate") == "on",
        }
    }

    fn render(&self, item: &Community, back: &str, error: &str) -> String {
        let (heading, action, submit) = if self.id.is_some() {
            (
                "Edit scheduled item",
                "edit-scheduled-item",
                "Save schedule",
            )
        } else {
            ("Schedule a post or event", "schedule-item", "Schedule")
        };
        let id = self.id.map(|value| value.to_string());
        let mut fields = vec![("community", item.slug.as_str())];
        if let Some(id) = &id {
            fields.push(("id", id));
        }
        let zone_options: String = ZONES
            .iter()
            .map(|(zone, label)| {
                format!(
                    "<option value='{zone}'{}>{label}</option>",
                    if *zone == self.zone { " selected" } else { "" }
                )
            })
            .collect();
        let alert = if error.is_empty() {
            String::new()
        } else {
            format!(
                "<p class='notice scheduled-error' role='alert'>{}</p>",
                escape(error)
            )
        };
        let abandon = if self.id.is_some() {
            format!(
                "<a class='button light' href='{}'>Back to schedule</a>",
                escape(back)
            )
        } else {
            String::new()
        };
        format!(
            r#"<section class="panel mod-section scheduled-form-panel" id="scheduled-editor"><h2>{heading}</h2>{alert}
<form class="stack-form scheduled-form" method="post" action="/action">{fields}
<label>Item type<select name="kind"><option value="post"{post}>Post</option><option value="event"{event}>Event</option></select></label>
<label>Title<input name="title" required maxlength="160" value="{title}"></label>
{body}
<label>Link <small>Optional</small><input type="url" name="url" maxlength="600" value="{url}"></label>
{flair}
<fieldset class="scheduled-dates"><legend>Date and time</legend>
<label>Time zone<select name="zone" aria-describedby="schedule-zone-help">{zone_options}</select></label><p id="schedule-zone-help" class="muted">All dates use the selected time zone. Choose an explicit Central offset if a clock change repeats the hour.</p>
<label>Publish at<input type="datetime-local" name="publish_at" value="{publish_at}" required></label>
<label>Event starts <small>Required for events</small><input type="datetime-local" name="event_start" value="{event_start}"></label>
<label>Event ends <small>Optional for events</small><input type="datetime-local" name="event_end" value="{event_end}"></label>
</fieldset>
<fieldset class="scheduled-flags"><legend>Post tags</legend>
<label class="checkbox"><input type="checkbox" name="sensitive"{sensitive}> NSFW (18+)</label>
<label class="checkbox"><input type="checkbox" name="spoiler"{spoiler}> Spoiler</label>
<label class="checkbox"><input type="checkbox" name="brand_affiliate"{brand_affiliate}> Brand affiliate</label>
</fieldset>
<div class="scheduled-actions form-actions"><button class="button dark" type="submit">{submit}</button>{abandon}</div></form></section>"#,
            fields = action_fields(action, back, &fields),
            post = selection(self.kind != "event"),
            event = selection(self.kind == "event"),
            title = escape(self.title),
            body = super::post_body_editor::editor(self.body, 4),
            url = escape(self.url),
            flair = post_flair::scheduled(item, self.flair_id, self.flair),
            publish_at = escape(&self.publish_at),
            event_start = escape(&self.event_start),
            event_end = escape(&self.event_end),
            sensitive = checked(self.sensitive),
            spoiler = checked(self.spoiler),
            brand_affiliate = checked(self.brand_affiliate),
        )
    }
}

fn known_zone(value: &str) -> &str {
    ZONES
        .iter()
        .find(|(zone, _)| *zone == value)
        .map_or(CHICAGO, |(zone, _)| zone)
}

fn selection(selected: bool) -> &'static str {
    if selected {
        " selected"
    } else {
        ""
    }
}

fn checked(enabled: bool) -> &'static str {
    if enabled {
        " checked"
    } else {
        ""
    }
}

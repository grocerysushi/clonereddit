use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use super::moderation_workspace::{report_record, QueueKind};
use super::modmail;
use super::{
    can_view_post, community_owner, community_url, encode, escape, field, hidden, post_card,
    PostView,
};
use crate::model::{Community, ModPermission, State};

const QUEUE_URL: &str = "/moderation?section=queue";
const MOD_FEED_URL: &str = "/moderation?section=mod-feed";

pub(super) fn render(state: &State, section: &str, query: &BTreeMap<String, String>) -> String {
    if section == "mail" {
        return modmail::render(state, None, query);
    }
    let assigned: BTreeMap<&str, &Community> = state
        .communities
        .iter()
        .filter(|item| match section {
            "manage" => state.can_open_mod_tools(item),
            _ => state.can_moderate(item, ModPermission::Reports),
        })
        .map(|item| (item.slug.as_str(), item))
        .collect();

    match section {
        "mod-feed" => mod_feed(state, &assigned),
        "manage" => manage(state, &assigned),
        _ => queue(state, &assigned, query),
    }
}

fn queue(
    state: &State,
    owned: &BTreeMap<&str, &Community>,
    query: &BTreeMap<String, String>,
) -> String {
    let kind = QueueKind::parse(field(query, "kind"));
    let (status, reviewed) = match field(query, "status") {
        "needs-review" => ("needs-review", Some(false)),
        "reviewed" => ("reviewed", Some(true)),
        _ => ("reported", None),
    };
    let posts: BTreeMap<_, _> = state
        .posts
        .iter()
        .filter(|post| owned.contains_key(post.community.as_str()))
        .map(|post| (post.id, post))
        .collect();
    let mut entries: Vec<_> = state
        .reports
        .iter()
        .filter(|report| {
            kind.allows(report.kind()) && reviewed.is_none_or(|value| report.reviewed == value)
        })
        .filter_map(|report| {
            let community = state.report_community(report)?;
            let item = *owned.get(community.slug.as_str())?;
            let post = posts
                .get(&report.post)
                .copied()
                .filter(|post| post.community == item.slug);
            Some((report, post, item))
        })
        .collect();
    entries.sort_by_key(|(report, _, _)| (report.reviewed, Reverse(report.id)));

    let back = format!("{QUEUE_URL}&status={status}&kind={}", kind.as_str());
    let mut status_options = String::new();
    for (value, label) in [
        ("reported", "All reports"),
        ("needs-review", "Needs review"),
        ("reviewed", "Reviewed"),
    ] {
        let selected = if status == value { " selected" } else { "" };
        write!(
            status_options,
            "<option value='{value}'{selected}>{label}</option>"
        )
        .unwrap();
    }
    let mut html = format!(
        "<section class='panel standalone moderation-tool'><h1>Moderation queue</h1><p>Mark reports reviewed or reopen them for follow-up.</p><form class='mod-queue-sort' method='get' action='/moderation'>{}<label>Status<select name='status'>{status_options}</select></label>{}<button class='button light' type='submit'>Apply</button></form>",
        hidden("section", "queue"),
        kind.select(),
    );
    if entries.is_empty() {
        html.push_str(
            "<p class='muted log-empty'>No reports in your communities match these filters.</p>",
        );
    }
    for (report, post, item) in entries {
        html.push_str(&report_record(state, item, post, report, &back, true));
    }
    html.push_str("</section>");
    html
}

fn mod_feed(state: &State, owned: &BTreeMap<&str, &Community>) -> String {
    let mut html = String::from(
        "<section class='panel standalone moderation-tool'><h1>Mod feed</h1><p>Discussions from communities you moderate.</p>",
    );
    let mut posts: Vec<_> = state
        .posts
        .iter()
        .filter(|post| can_view_post(state, post))
        .filter_map(|post| {
            owned
                .get(post.community.as_str())
                .map(|item| (post, item.is_highlighted(post.id)))
        })
        .collect();
    posts.sort_by_key(|(post, _)| Reverse(post.id));
    if posts.is_empty() {
        html.push_str("<p class='muted log-empty'>No discussions from your communities.</p>");
    }
    html.push_str("</section>");
    for (post, pinned) in posts {
        html.push_str(&post_card(
            state,
            post,
            MOD_FEED_URL,
            PostView::Card,
            pinned,
        ));
    }
    html
}

fn manage(state: &State, assigned: &BTreeMap<&str, &Community>) -> String {
    let mut html = String::from(
        "<section class='panel standalone moderation-tool'><header class='creator-header'><h1>Manage communities</h1><a class='button dark' href='/communities/new'>Create community</a></header>",
    );
    if assigned.is_empty() {
        html.push_str(
            "<p class='muted log-empty'>Communities you create or moderate will appear here.</p>",
        );
    } else {
        html.push_str("<ul class='creator-list'>");
        for item in assigned.values() {
            let is_owner = community_owner(state, item);
            let slug = encode(&item.slug);
            let name = escape(&item.name);
            let title = if state.can_view_community(item) {
                format!(
                    "<a class='subtle-link' href='{}'>{name}</a>",
                    community_url(&item.slug)
                )
            } else {
                format!("<strong>{name}</strong>")
            };
            let role = state
                .moderator_role(item, &state.username)
                .map_or("Owner", |role| role.label());
            let settings = if is_owner {
                format!("<a class='button light' aria-label='Community settings for {name}' href='/communities/settings?community={slug}'>Community settings</a>")
            } else {
                String::new()
            };
            write!(
                html,
                "<li class='creator-row'><div class='creator-row-main'>{title}<p class='muted'>{role}</p></div><div class='creator-row-actions'><a class='button light' href='/moderation/tools?community={slug}'>Mod tools</a>{settings}</div></li>"
            )
            .unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

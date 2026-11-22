use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use super::{
    action_fields, action_form, community_owner, community_url, encode, escape, post_card,
};
use crate::model::{Community, ModMail, ModPermission, State};

const QUEUE_URL: &str = "/moderation?section=queue";
const MAIL_URL: &str = "/moderation?section=mail";
const MOD_FEED_URL: &str = "/moderation?section=mod-feed";

pub(super) fn render(state: &State, section: &str) -> String {
    let assigned: BTreeMap<&str, &Community> = state
        .communities
        .iter()
        .filter(|item| match section {
            "manage" => state.can_open_mod_tools(item),
            "mail" => state.can_moderate(item, ModPermission::Mail),
            _ => state.can_moderate(item, ModPermission::Reports),
        })
        .map(|item| (item.slug.as_str(), item))
        .collect();

    match section {
        "mail" => mail(state, &assigned),
        "mod-feed" => mod_feed(state, &assigned),
        "manage" => manage(state, &assigned),
        _ => queue(state, &assigned),
    }
}

fn queue(state: &State, owned: &BTreeMap<&str, &Community>) -> String {
    let posts: BTreeMap<_, _> = state
        .posts
        .iter()
        .filter_map(|post| {
            owned
                .get(post.community.as_str())
                .map(|&item| (post.id, (post, item)))
        })
        .collect();
    let mut entries: Vec<_> = state
        .reports
        .iter()
        .filter_map(|report| {
            posts
                .get(&report.post)
                .map(|(post, item)| (report, *post, *item))
        })
        .collect();
    entries.sort_by_key(|(report, _, _)| (report.reviewed, Reverse(report.id)));

    let mut html = String::from(
        "<section class='panel standalone moderation-tool'><h1>Moderation queue</h1><p>Review legal reports in communities you moderate. Changing a report’s status leaves the discussion visible.</p>",
    );
    if entries.is_empty() {
        html.push_str("<p class='muted log-empty'>No reports from your communities.</p>");
    }
    for (report, post, item) in entries {
        let (status, reviewed, action) = if report.reviewed {
            ("Reviewed", "no", "Reopen")
        } else {
            ("Pending review", "yes", "Mark reviewed")
        };
        let update = action_form(
            "review-report",
            QUEUE_URL,
            &[("id", &report.id.to_string()), ("reviewed", reviewed)],
            action,
            "",
            "button light",
        );
        write!(
            html,
            "<article class='log-item moderation-record'><span class='flair'>{status}</span><h2><a href='/thread?id={}'>{}</a></h2><p><a class='subtle-link' href='{}'>{}</a> · <strong>Region</strong> {}</p><p>{}</p><div class='form-actions'>{update}</div></article>",
            post.id,
            escape(&post.title),
            community_url(&item.slug),
            escape(&item.name),
            escape(&report.region),
            escape(&report.details)
        )
        .unwrap();
    }
    html.push_str("</section>");
    html
}

fn mail(state: &State, owned: &BTreeMap<&str, &Community>) -> String {
    let mut html = String::from(
        "<section class='panel standalone moderation-tool'><h1>Mod mail</h1><section class='preference-list'><h2>Inbox</h2>",
    );
    let mut inbox = state
        .mod_mail
        .iter()
        .rev()
        .filter_map(|message| {
            owned
                .get(message.community.as_str())
                .map(|item| (message, *item))
        })
        .peekable();
    if inbox.peek().is_none() {
        html.push_str("<p class='muted log-empty'>No messages to your communities.</p>");
    }
    for (message, item) in inbox {
        mail_record(&mut html, message, Some(&item.name), true);
    }

    html.push_str("</section><section class='preference-list'><h2>Sent</h2>");
    let mut sent = state
        .mod_mail
        .iter()
        .rev()
        .filter(|message| message.sender == state.username)
        .peekable();
    if sent.peek().is_none() {
        html.push_str("<p class='muted log-empty'>No messages sent.</p>");
    }
    for message in sent {
        let name = state
            .communities
            .iter()
            .find(|item| item.slug == message.community && state.can_view_community(item))
            .map(|item| item.name.as_str());
        mail_record(&mut html, message, name, false);
    }
    html.push_str("</section></section>");
    html
}

fn mail_record(html: &mut String, message: &ModMail, community_name: Option<&str>, inbox: bool) {
    let from = if inbox {
        format!("From u/{} · ", escape(&message.sender))
    } else {
        String::from("To ")
    };
    let destination = community_name.map_or_else(
        || "Community unavailable".to_owned(),
        |name| {
            format!(
                "<a class='subtle-link' href='{}'>{}</a>",
                community_url(&message.community),
                escape(name)
            )
        },
    );
    write!(
        html,
        "<article class='log-item moderation-record'><h3>{}</h3><p class='muted'>{from}{destination}</p><p>{}</p>",
        escape(&message.subject),
        escape(&message.body)
    )
    .unwrap();
    if !message.reply.is_empty() {
        write!(
            html,
            "<blockquote class='moderation-reply'><strong>Reply from {}</strong><p>{}</p></blockquote>",
            escape(community_name.unwrap_or("the moderation team")),
            escape(&message.reply)
        )
        .unwrap();
    } else if inbox {
        write!(
            html,
            "<form class='stack-form moderation-reply-form' method='post' action='/action'>{}<label>Reply<textarea name='reply' rows='3' maxlength='4000' required></textarea></label><button class='button dark' type='submit'>Send reply</button></form>",
            action_fields("reply-mod-mail", MAIL_URL, &[("id", &message.id.to_string())])
        )
        .unwrap();
    } else {
        html.push_str("<p class='muted'>Awaiting reply</p>");
    }
    html.push_str("</article>");
}

fn mod_feed(state: &State, owned: &BTreeMap<&str, &Community>) -> String {
    let mut html = String::from(
        "<section class='panel standalone moderation-tool'><h1>Mod feed</h1><p>Discussions from communities you moderate.</p>",
    );
    let mut posts: Vec<_> = state
        .posts
        .iter()
        .filter_map(|post| {
            owned
                .get(post.community.as_str())
                .map(|item| (post, item.pinned_post == Some(post.id)))
        })
        .collect();
    posts.sort_by_key(|(post, _)| Reverse(post.id));
    if posts.is_empty() {
        html.push_str("<p class='muted log-empty'>No discussions from your communities.</p>");
    }
    html.push_str("</section>");
    for (post, pinned) in posts {
        html.push_str(&post_card(state, post, MOD_FEED_URL, false, pinned));
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

pub(super) fn contact(item: &Community) -> String {
    let fields = action_fields("send-mod-mail", MAIL_URL, &[("community", &item.slug)]);
    let community = escape(&item.name);
    let cancel = community_url(&item.slug);
    format!(
        "<section class='panel standalone moderation-tool'><h1>Contact moderators</h1><p>Send a message to the moderators of <strong>{community}</strong>.</p><form class='stack-form' action='/action' method='post'>{fields}<label>Subject<input name='subject' maxlength='120' required></label><label>Message<textarea name='body' rows='7' maxlength='4000' required></textarea></label><div class='form-actions'><button class='button dark' type='submit'>Send message</button><a class='button light' href='{cancel}'>Cancel</a></div></form></section>"
    )
}

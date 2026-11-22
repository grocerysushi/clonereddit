use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use crate::model::{Comment, Community, ContentKind, ModPermission, Post, State};
use crate::schedule_time;

use super::super::{
    account_byline, action_fields, action_form, display_post_title, encode, escape, field, hidden,
};
use super::{empty, tools_url};

const PAGE_SIZE: usize = 25;

#[derive(Clone, Copy)]
enum Entry<'a> {
    Post(&'a Post),
    Comment(&'a Comment, &'a Post),
}

impl<'a> Entry<'a> {
    fn id(self) -> u64 {
        match self {
            Self::Post(post) => post.id,
            Self::Comment(comment, _) => comment.id,
        }
    }

    fn post(self) -> &'a Post {
        match self {
            Self::Post(post) | Self::Comment(_, post) => post,
        }
    }

    fn author(self) -> &'a str {
        match self {
            Self::Post(post) => &post.author,
            Self::Comment(comment, _) => &comment.author,
        }
    }

    fn body(self) -> &'a str {
        match self {
            Self::Post(post) => &post.body,
            Self::Comment(comment, _) => &comment.body,
        }
    }

    fn kind(self) -> ContentKind {
        match self {
            Self::Post(_) => ContentKind::Post,
            Self::Comment(..) => ContentKind::Comment,
        }
    }

    fn parent_removed(self, state: &State) -> bool {
        matches!(self, Self::Comment(..))
            && state
                .content_removal(ContentKind::Post, self.post().id)
                .is_some()
    }
}

pub(super) fn detail_url(item: &Community, kind: ContentKind, id: u64) -> String {
    format!(
        "{}&kind={}&id={id}",
        tools_url(item, "content"),
        kind.as_str()
    )
}

pub(super) fn render(
    state: &State,
    item: &Community,
    posts: &BTreeMap<u64, &Post>,
    query: &BTreeMap<String, String>,
    error: Option<&str>,
) -> String {
    if !state.can_moderate(item, ModPermission::Content) {
        return empty(
            "Content unavailable",
            "Your account needs content moderation access.",
            "shield",
        );
    }

    let kind = match field(query, "kind") {
        "" | "post" => Some(ContentKind::Post),
        "comment" => Some(ContentKind::Comment),
        _ => None,
    };
    let mut html = error.map_or_else(String::new, |error| {
        format!(
            "<p class='notice setup-error' role='alert'>{}</p>",
            escape(error)
        )
    });
    if query.contains_key("id") {
        let entry = kind
            .zip(field(query, "id").parse::<u64>().ok())
            .and_then(|(kind, id)| match kind {
                ContentKind::Post => posts.get(&id).map(|post| Entry::Post(post)),
                ContentKind::Comment => state
                    .comments
                    .iter()
                    .find(|comment| comment.id == id && !comment.deleted)
                    .and_then(|comment| {
                        posts
                            .get(&comment.post)
                            .map(|post| Entry::Comment(comment, post))
                    }),
            });
        if let Some(entry) = entry {
            html.push_str(&detail(state, item, entry, field(query, "reason")));
        } else {
            write!(
                html,
                "<a class='back-link' href='{}'>← Posts &amp; comments</a>",
                escape(&tools_url(item, "content"))
            )
            .unwrap();
            html.push_str(&empty(
                "Content unavailable",
                "Choose a post or comment from this community’s list.",
                "shield",
            ));
        }
        return html;
    }

    html.push_str(&list(
        state,
        item,
        posts,
        kind.unwrap_or(ContentKind::Post),
        query,
    ));
    html
}

fn list(
    state: &State,
    item: &Community,
    posts: &BTreeMap<u64, &Post>,
    kind: ContentKind,
    query: &BTreeMap<String, String>,
) -> String {
    let removed = field(query, "status") == "removed";
    let search: String = field(query, "content_q").trim().chars().take(120).collect();
    let base = tools_url(item, "content");
    let tab = |label, kind, removed, active| {
        let url = escape(&list_url(&base, kind, removed, &search, 1));
        let aria = if active { " aria-current='page'" } else { "" };
        format!("<a href='{url}'{aria}>{label}</a>")
    };
    let post_list = tab(
        "Posts",
        ContentKind::Post,
        removed,
        matches!(kind, ContentKind::Post),
    );
    let comment_list = tab(
        "Comments",
        ContentKind::Comment,
        removed,
        matches!(kind, ContentKind::Comment),
    );
    let available = tab("Available", kind, false, !removed);
    let removed_list = tab("Removed", kind, true, removed);
    let fields = hidden("community", &item.slug)
        + &hidden("section", "content")
        + &hidden("kind", kind.as_str())
        + &hidden("status", status(removed));
    let mut html = format!(
        "<div class='mod-content-controls'><nav class='mod-queue-tabs' aria-label='Content type'>{post_list}{comment_list}</nav><nav class='mod-queue-tabs' aria-label='Content status'>{available}{removed_list}</nav></div><form class='mod-tool-search mod-content-search' action='/moderation/tools' method='get' role='search'>{fields}<label class='sr-only' for='content-query'>Search content</label><input id='content-query' type='search' name='content_q' maxlength='120' value='{}' placeholder='Search content'><button class='button light' type='submit'>Search content</button></form>",
        escape(&search)
    );

    let needle = search.to_lowercase();
    let mut entries: Vec<_> = match kind {
        ContentKind::Post => posts.values().map(|post| Entry::Post(post)).collect(),
        ContentKind::Comment => state
            .comments
            .iter()
            .filter(|comment| !comment.deleted)
            .filter_map(|comment| {
                posts
                    .get(&comment.post)
                    .map(|post| Entry::Comment(comment, post))
            })
            .collect(),
    };
    entries.retain(|entry| {
        let removal = state.content_removal(kind, entry.id());
        removal.is_some() == removed
            && (needle.is_empty()
                || [
                    display_post_title(state, entry.post()),
                    entry.author(),
                    entry.body(),
                ]
                .iter()
                .any(|text| text.to_lowercase().contains(&needle)))
    });
    if entries.is_empty() {
        let message = if search.is_empty() {
            "There is no content in this view."
        } else {
            "Try a different title, username, or phrase."
        };
        html.push_str(&empty("No matches", message, "chat"));
        return html;
    }
    entries.sort_by_key(|entry| Reverse(entry.id()));
    let last = entries.len().div_ceil(PAGE_SIZE);
    let page = field(query, "page")
        .parse::<usize>()
        .unwrap_or(1)
        .clamp(1, last);
    let start = (page - 1) * PAGE_SIZE;
    for entry in entries.into_iter().skip(start).take(PAGE_SIZE) {
        let context = if entry.parent_removed(state) {
            " <span class='flair'>Parent post removed</span>"
        } else {
            ""
        };
        let preview = excerpt(entry.body());
        write!(html, "<article class='panel mod-record mod-content-list-item'><header><span class='flair'>{}</span><span class='muted'>#{} · {}</span></header><h2><a href='{}'>{}</a></h2>{context}<p class='mod-record-detail'>{}</p></article>",
            kind.label(), entry.id(), account_byline(entry.author()), escape(&detail_url(item, kind, entry.id())), escape(display_post_title(state, entry.post())), escape(&preview)).unwrap();
    }
    if last > 1 {
        let page_link = |label, index| {
            format!(
                "<a class='button light' href='{}'>{label}</a>",
                escape(&list_url(&base, kind, removed, &search, index))
            )
        };
        let previous = if page > 1 {
            page_link("Previous", page - 1)
        } else {
            String::new()
        };
        let next = if page < last {
            page_link("Next", page + 1)
        } else {
            String::new()
        };
        write!(html, "<nav class='mod-content-pagination' aria-label='Content pages'>{previous}<span class='muted'>Page {page} of {last}</span>{next}</nav>").unwrap();
    }
    html
}

fn detail(state: &State, item: &Community, entry: Entry<'_>, requested_reason: &str) -> String {
    let kind = entry.kind();
    let id = entry.id();
    let target = detail_url(item, kind, id);
    let removal = state.content_removal(kind, id);
    let removed = removal.is_some();
    let list = list_url(&tools_url(item, "content"), kind, removed, "", 1);
    let label = kind.label();
    let display_status = if removed { "Removed" } else { "Available" };
    let context = if entry.parent_removed(state) {
        " <span class='flair'>Parent post removed</span>"
    } else {
        ""
    };
    let mut html = format!("<a class='back-link' href='{}'>← Posts &amp; comments</a><article class='panel mod-record mod-content-detail'><header><span class='flair'>{display_status}</span><span class='muted'>{label} #{id} · {}</span></header><h2>{}</h2>{context}", escape(&list), account_byline(entry.author()), escape(display_post_title(state, entry.post())));
    if !entry.body().is_empty() {
        write!(
            html,
            "<p class='mod-record-detail'>{}</p>",
            escape(entry.body())
        )
        .unwrap();
    }
    if let Entry::Post(post) = entry {
        if !post.url.is_empty() {
            write!(
                html,
                "<p class='mod-record-detail'><strong>Link</strong> <code>{}</code></p>",
                escape(&post.url)
            )
            .unwrap();
        }
    }
    let fields = [
        ("community", item.slug.as_str()),
        ("kind", kind.as_str()),
        ("id", &id.to_string()),
    ];
    if let Some(removal) = removal {
        let when = schedule_time::display(removal.removed_at, "America/Chicago");
        let timestamp = schedule_time::to_local(removal.removed_at, "UTC");
        let actor = if removal.automated {
            "AutoMod".to_owned()
        } else {
            format!("u/{}", escape(&removal.moderator))
        };
        let restore = action_form(
            "restore-content",
            &target,
            &fields,
            "Restore content",
            "",
            "button dark",
        );
        write!(html, "<blockquote class='moderation-reply'><strong>Removed by {actor}</strong> <time class='muted' datetime='{timestamp}Z'>{}</time><p class='mod-record-detail'>{}</p></blockquote><div class='form-actions'>{restore}</div>",
            escape(&when), escape(&removal.reason)).unwrap();
    } else {
        if !entry.parent_removed(state) {
            let anchor = if matches!(entry, Entry::Comment(..)) {
                format!("#comment-{id}")
            } else {
                String::new()
            };
            write!(html, "<p class='mod-content-thread'><a class='subtle-link' href='/thread?id={}{anchor}'>Open discussion</a></p>", entry.post().id).unwrap();
        }
        let remove = action_fields("remove-content", &target, &fields);
        write!(html, "<form class='stack-form mod-content-action' action='/action' method='post'>{remove}<label>Removal reason<textarea name='reason' rows='4' maxlength='1000' aria-describedby='content-reason-help' required>{}</textarea></label><small id='content-reason-help' class='muted'>This reason appears in the author’s notification.</small><button class='button dark' type='submit'>Remove content</button></form>", escape(requested_reason)).unwrap();
    }
    html.push_str("</article>");
    html
}

fn list_url(base: &str, kind: ContentKind, removed: bool, search: &str, page: usize) -> String {
    format!(
        "{base}&kind={}&status={}&content_q={}&page={page}",
        kind.as_str(),
        status(removed),
        encode(search)
    )
}

fn status(removed: bool) -> &'static str {
    if removed {
        "removed"
    } else {
        "available"
    }
}

fn excerpt(body: &str) -> String {
    let mut chars = body.chars();
    let mut text: String = chars.by_ref().take(180).collect();
    if chars.next().is_some() {
        text.push('…');
    }
    text
}

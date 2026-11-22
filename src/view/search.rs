use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{
    account_byline, can_view_post, community, community_icon, community_members, community_url,
    custom_feed, display_post_title, encode, escape, field, hidden, initials, is_sensitive,
    masthead,
};
use crate::auth::AuthStore;
use crate::model::{Comment, Community, CustomFeed, Post, State};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Kind {
    Posts,
    Comments,
    Communities,
    People,
    Media,
}

impl Kind {
    pub(super) fn from_query(query: &BTreeMap<String, String>) -> Self {
        match field(query, "type") {
            "comments" => Self::Comments,
            "communities" => Self::Communities,
            "people" => Self::People,
            "media" => Self::Media,
            _ => Self::Posts,
        }
    }

    pub(super) fn code(self) -> &'static str {
        match self {
            Self::Posts => "posts",
            Self::Comments => "comments",
            Self::Communities => "communities",
            Self::People => "people",
            Self::Media => "media",
        }
    }
}

const POST_FIELDS: &[&str] = &["author", "community", "post_flair"];

fn post_filters(query: &BTreeMap<String, String>) -> impl Iterator<Item = (&'static str, &str)> {
    POST_FIELDS.iter().copied().filter_map(|name| {
        let value = field(query, name).trim();
        (!value.is_empty()).then_some((name, value))
    })
}

pub(super) fn active(query: &BTreeMap<String, String>) -> bool {
    field(query, "search") == "1"
        || !field(query, "q").trim().is_empty()
        || post_filters(query).next().is_some()
}

pub(super) fn without_post_filters(mut url: String, kind: Kind) -> String {
    if kind != Kind::Posts {
        write!(url, "&type={}", kind.code()).unwrap();
    }
    url.push_str("&search=1");
    url
}

pub(super) fn with_filters(url: String, query: &BTreeMap<String, String>, kind: Kind) -> String {
    if !active(query) {
        return url;
    }
    let mut url = without_post_filters(url, kind);
    for (name, value) in post_filters(query) {
        write!(url, "&{name}={}", encode(value)).unwrap();
    }
    url
}

pub(super) fn header_fields(query: Option<&BTreeMap<String, String>>) -> String {
    let mut html = hidden("search", "1");
    if let Some(query) = query {
        let kind = Kind::from_query(query);
        if kind != Kind::Posts {
            html.push_str(&hidden("type", kind.code()));
        }
        for name in ["sort", "t"] {
            let value = field(query, name);
            if !value.is_empty() {
                html.push_str(&hidden(name, value));
            }
        }
        for (name, value) in post_filters(query) {
            html.push_str(&hidden(name, value));
        }
    }
    html
}

pub(super) fn has_media(state: &State, post: &Post) -> bool {
    post.image || !post.media.is_empty() || state.post_has_inline_images(post.id)
}

fn url(active: &str, query: &BTreeMap<String, String>, kind: Kind) -> String {
    let mut url = format!(
        "/?view={}&q={}",
        encode(active),
        encode(field(query, "q").trim())
    );
    for key in ["sort", "t"] {
        let value = field(query, key);
        if !value.is_empty() {
            write!(url, "&{key}={}", encode(value)).unwrap();
        }
    }
    with_filters(url, query, kind)
}

pub(super) fn tabs(active: &str, query: &BTreeMap<String, String>, kind: Kind) -> String {
    let mut html = String::from(
        "<nav class='panel profile-tabs search-tabs' aria-label='Search result types'>",
    );
    for (candidate, label) in [
        (Kind::Posts, "Posts"),
        (Kind::Comments, "Comments"),
        (Kind::Communities, "Communities"),
        (Kind::People, "People"),
        (Kind::Media, "Media"),
    ] {
        let selected = if candidate == kind {
            " aria-current='page'"
        } else {
            ""
        };
        let href = escape(&url(active, query, candidate));
        write!(html, "<a href='{href}'{selected}>{label}</a>").unwrap();
    }
    html.push_str("</nav>");
    html
}

pub(super) fn render(
    state: &State,
    auth: &AuthStore,
    active: &str,
    query: &BTreeMap<String, String>,
    kind: Kind,
) -> Option<String> {
    if !self::active(query) || matches!(kind, Kind::Posts | Kind::Media) {
        return None;
    }
    let search = field(query, "q").trim();
    if search.is_empty() {
        let mut html = masthead("Search", "", "DISCUSSIONS");
        html.push_str(&tabs(active, query, kind));
        html.push_str(&empty("Enter a search term"));
        return Some(html);
    }
    let scope = Scope {
        state,
        active,
        selected: community(state, active),
        custom: custom_feed(state, active),
    };
    let needle = search.to_lowercase();
    let results = match kind {
        Kind::Comments => comments(&scope, &needle, field(query, "sort")),
        Kind::Communities => communities(&scope, &needle),
        Kind::People => people(&scope, auth, &needle),
        Kind::Posts | Kind::Media => return None,
    };
    let mut html = masthead("Search", &format!("Results for “{search}”"), "DISCUSSIONS");
    html.push_str(&tabs(active, query, kind));
    let scope_name = scope
        .selected
        .map(|item| item.name.as_str())
        .or_else(|| scope.custom.map(|feed| feed.name.as_str()));
    if let Some(name) = scope_name {
        write!(html, "<div class='community-bar'><span>Searching {}</span><a class='button light' href='{}'>Search all of ThreadPorch</a></div>", escape(name), escape(&url("all", query, kind))).unwrap();
    }
    html.push_str(&results);
    Some(html)
}

struct Scope<'a> {
    state: &'a State,
    active: &'a str,
    selected: Option<&'a Community>,
    custom: Option<&'a CustomFeed>,
}

impl<'a> Scope<'a> {
    fn explicit(&self) -> bool {
        self.selected.is_some() || self.custom.is_some()
    }

    fn includes_community(&self, item: &Community) -> bool {
        if !self.state.can_view_community(item) {
            return false;
        }
        if let Some(selected) = self.selected {
            return selected.slug == item.slug;
        }
        if self.state.muted.contains(&item.slug) {
            return false;
        }
        if let Some(feed) = self.custom {
            return feed.communities.contains(&item.slug);
        }
        match self.active {
            "following" => self.state.joined.contains(&item.slug),
            "saved" => true,
            _ => item.visibility != "private",
        }
    }

    fn visible_posts(&self) -> impl Iterator<Item = &'a Post> + '_ {
        self.state.posts.iter().filter(move |post| {
            can_view_post(self.state, post)
                && community(self.state, &post.community)
                    .is_some_and(|item| self.includes_community(item))
                && (self.active != "saved" || self.state.saved.contains(&post.id))
                && (self.active != "news" || post.flair.eq_ignore_ascii_case("news"))
                && !self.state.hidden.contains(&post.id)
                && !self.state.blocked.contains(&post.author)
                && !(self.state.sensitive == "hide" && is_sensitive(self.state, post))
                && !self.state.keywords.iter().any(|keyword| {
                    let needle = keyword.to_lowercase();
                    matches_text(display_post_title(self.state, post), &needle)
                        || matches_text(&post.body, &needle)
                })
        })
    }

    fn visible_comment(&self, comment: &Comment) -> bool {
        !comment.deleted
            && !self.state.is_comment_removed(comment.id)
            && !self.state.blocked.contains(&comment.author)
            && !self
                .state
                .keywords
                .iter()
                .any(|keyword| matches_text(&comment.body, &keyword.to_lowercase()))
    }
}

fn comments(scope: &Scope<'_>, needle: &str, sort: &str) -> String {
    let parents: BTreeMap<_, _> = scope.visible_posts().map(|post| (post.id, post)).collect();
    let mut matches: Vec<_> = scope
        .state
        .comments
        .iter()
        .filter(|comment| scope.visible_comment(comment) && matches_text(&comment.body, needle))
        .filter_map(|comment| parents.get(&comment.post).map(|post| (comment, *post)))
        .collect();
    matches.sort_by_key(|(comment, _)| {
        let score = if sort == "top" {
            scope.state.comment_score(comment)
        } else {
            0
        };
        (Reverse(score), Reverse(comment.id))
    });

    if matches.is_empty() {
        return empty("No matching comments");
    }
    let state = scope.state;
    let mut html = String::from("<section class='search-comments' aria-label='Matching comments'>");
    for (comment, post) in matches {
        let title = escape(display_post_title(state, post));
        let location = format!("/thread?id={}#comment-{}", post.id, comment.id);
        let author = account_byline(&comment.author);
        let group = community(state, &post.community)
            .map_or(post.community.as_str(), |item| item.name.as_str());
        let excerpt = escape(&excerpt(&comment.body));
        write!(html, "<article class='panel profile-item search-comment'><div class='profile-comment-meta'><a class='subtle-link' href='{}'>{}</a><span class='muted'>{author} · {}</span></div><h2><a href='{location}'>Comment on {title}</a></h2>", community_url(&post.community), escape(group), escape(&comment.age)).unwrap();
        if post.spoiler || (state.sensitive != "show" && is_sensitive(state, post)) {
            let label = if post.spoiler {
                "Reveal spoiler"
            } else {
                "Show sensitive preview"
            };
            write!(html, "<details class='sensitive-preview'><summary>{label}</summary><p>{excerpt}</p></details>").unwrap();
        } else {
            write!(html, "<p>{excerpt}</p>").unwrap();
        }
        html.push_str("</article>");
    }
    html.push_str("</section>");
    html
}

fn communities(scope: &Scope<'_>, needle: &str) -> String {
    let state = scope.state;
    let mut matches: Vec<_> = state
        .communities
        .iter()
        .filter(|item| {
            let discoverable = if scope.explicit() {
                scope.includes_community(item)
            } else {
                item.visibility == "public" && !state.muted.contains(&item.slug)
            };
            discoverable
                && !(item.mature && state.sensitive == "hide")
                && [&item.name, &item.slug, &item.description, &item.topic]
                    .into_iter()
                    .any(|text| matches_text(text, needle))
        })
        .collect();
    matches.sort_by_cached_key(|item| (item.slug != needle, item.name.to_lowercase()));

    if matches.is_empty() {
        return empty("No matching communities");
    }
    let mut html = String::from(
        "<section class='directory search-communities' aria-label='Matching communities'>",
    );
    for item in matches {
        let handle = escape(&item.slug);
        let name = escape(&item.name);
        let href = community_url(&item.slug);
        let description = if item.mature && state.sensitive != "show" {
            ""
        } else {
            &item.description
        };
        write!(html, "<article class='panel directory-item'><a href='{href}'>{}<h2>{name}</h2></a><small>c/{handle} · {}</small>", community_icon(item), community_members(state, item)).unwrap();
        if !description.is_empty() {
            write!(html, "<p>{}</p>", escape(description)).unwrap();
        }
        html.push_str("</article>");
    }
    html.push_str("</section>");
    html
}

fn people(scope: &Scope<'_>, auth: &AuthStore, needle: &str) -> String {
    let state = scope.state;
    let eligible = scope.explicit().then(|| {
        let posts: BTreeMap<_, _> = scope
            .visible_posts()
            .map(|post| (post.id, post.author.as_str()))
            .collect();
        let mut authors: BTreeSet<_> = posts.values().copied().collect();
        authors.extend(
            state
                .comments
                .iter()
                .filter(|comment| {
                    posts.contains_key(&comment.post) && scope.visible_comment(comment)
                })
                .map(|comment| comment.author.as_str()),
        );
        authors
    });
    let handle_needle = needle
        .strip_prefix("u/")
        .filter(|name| !name.is_empty())
        .unwrap_or(needle);
    let mut matches: Vec<_> = auth
        .usernames()
        .filter(|handle| {
            !state.blocked.contains(*handle)
                && eligible
                    .as_ref()
                    .is_none_or(|authors| authors.contains(handle))
        })
        .filter_map(|handle| {
            let name = state.account_display_name(handle).unwrap_or(handle);
            (handle.contains(handle_needle)
                || (handle_needle == needle && matches_text(name, needle)))
            .then_some((handle, name))
        })
        .collect();
    matches.sort_by_cached_key(|(handle, name)| {
        (*handle != handle_needle, name.to_lowercase(), *handle)
    });

    if matches.is_empty() {
        return empty("No matching people");
    }
    let mut html =
        String::from("<section class='directory search-people' aria-label='Matching people'>");
    for (handle, name) in matches {
        let clean_handle = escape(handle);
        let avatar = escape(&initials(name));
        let label = if name == handle {
            format!("u/{clean_handle}")
        } else {
            escape(name)
        };
        write!(html, "<article class='panel directory-item'><a href='/u/{}'><span class='avatar' aria-hidden='true'>{avatar}</span><h2>{label}</h2></a>", encode(handle)).unwrap();
        if name != handle {
            write!(html, "<small>u/{clean_handle}</small>").unwrap();
        }
        html.push_str("</article>");
    }
    html.push_str("</section>");
    html
}

fn matches_text(text: &str, needle: &str) -> bool {
    text.to_lowercase().contains(needle)
}

fn excerpt(text: &str) -> String {
    let mut chars = text.chars();
    let mut short: String = chars.by_ref().take(240).collect();
    if chars.next().is_some() {
        short.push('…');
    }
    short
}

fn empty(label: &str) -> String {
    format!("<section class='panel empty'><h2>{label}</h2></section>")
}

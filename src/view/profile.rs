use std::cmp::Reverse;
use std::fmt::Write;

use super::{
    can_view_post, community, community_url, encode, escape, icon, initials, post_card,
    profile_name, profile_url, visible_post,
};
use crate::model::{Comment, Post, State};

const TABS: &[(&str, &str)] = &[
    ("overview", "Overview"),
    ("posts", "Posts"),
    ("comments", "Comments"),
    ("saved", "Saved"),
    ("history", "History"),
    ("hidden", "Hidden"),
    ("upvoted", "Upvoted"),
    ("downvoted", "Downvoted"),
];

pub(super) fn render(state: &State, requested: &str, public: bool, can_message: bool) -> String {
    let available = if public { &TABS[..3] } else { TABS };
    let tab = available
        .iter()
        .find(|(code, _)| *code == requested)
        .map(|(code, _)| *code)
        .unwrap_or("overview");
    let base = if public {
        profile_url(state)
    } else {
        "/profile".into()
    };
    let back = format!("{base}?tab={tab}");
    let controls = if can_message {
        format!("<div class='profile-header-actions'><a class='button light' href='/messages?peer={}'>{} Message</a></div>", encode(&state.username), icon("mail"))
    } else if public {
        String::new()
    } else {
        format!("<div class='profile-header-actions'><a class='button light' href='/profile/settings?section=identity'>{} Edit profile</a><form method='post' action='/account/logout'><button class='button light' type='submit'>Sign out</button></form></div>", icon("sliders"))
    };
    let show_avatar =
        state.avatar.is_some() && (!public || !state.profile_nsfw || state.sensitive == "show");
    let avatar = if show_avatar {
        format!(
            "<img class='profile-main-avatar' src='{base}/avatar' width='74' height='74' alt=''>"
        )
    } else {
        format!(
            "<span class='profile-main-avatar profile-main-initials' aria-hidden='true'>{}</span>",
            escape(&initials(profile_name(state)))
        )
    };
    let avatar_edit = if public {
        String::new()
    } else {
        format!("<a class='profile-avatar-edit' href='/profile/settings?section=images' aria-label='Change avatar'>{}</a>", icon("image"))
    };
    let tabs = available
        .iter()
        .map(|(code, label)| {
            let current = if *code == tab {
                " aria-current='page'"
            } else {
                ""
            };
            format!(
                "<a href='{}?tab={code}'{current}>{label}</a>",
                escape(&base)
            )
        })
        .collect::<String>();
    let badge = if state.profile_nsfw {
        super::nsfw_badge()
    } else {
        ""
    };
    let mut html = format!(
        r#"<section class="panel profile-hero"><header class="profile-heading"><div class="profile-avatar-wrap">{avatar}{avatar_edit}</div><div class="profile-heading-copy"><h1>{}</h1><div class="profile-heading-meta"><span>u/{}</span>{badge}</div></div>{controls}</header><nav class="profile-tabs" aria-label="Profile activity">{tabs}</nav></section>"#,
        escape(profile_name(state)),
        escape(&state.username)
    );

    if tab == "overview" {
        let recent = posts(state, "posts", public);
        let replies = comments(state, public);
        if recent.is_empty() && replies.is_empty() {
            html.push_str(
                "<section class='panel empty'><h2>No profile activity yet.</h2></section>",
            );
        }
        if !recent.is_empty() {
            write!(html, "<div class='profile-activity-heading'><h2>Recent posts</h2><a class='subtle-link' href='{}?tab=posts'>View posts</a></div>", escape(&base)).unwrap();
            add_posts(&mut html, state, recent.into_iter().take(3), &back, public);
        }
        if !replies.is_empty() {
            write!(html, "<div class='profile-activity-heading'><h2>Recent comments</h2><a class='subtle-link' href='{}?tab=comments'>View comments</a></div>", escape(&base)).unwrap();
            add_comments(&mut html, state, replies.into_iter().take(3));
        }
    } else if tab == "comments" {
        let items = comments(state, public);
        if items.is_empty() {
            html.push_str("<section class='panel empty'><h2>No comments to show.</h2></section>");
        }
        add_comments(&mut html, state, items.into_iter());
    } else {
        let items = posts(state, tab, public);
        if items.is_empty() {
            let status = match tab {
                "saved" => "No saved posts yet.",
                "history" => "Discussions you open will appear here.",
                "hidden" => "No hidden posts.",
                "upvoted" => "No upvoted posts.",
                "downvoted" => "No downvoted posts.",
                _ => "No posts to show.",
            };
            write!(
                html,
                "<section class='panel empty'><h2>{status}</h2></section>"
            )
            .unwrap();
        }
        add_posts(&mut html, state, items.into_iter(), &back, public);
    }
    html
}

fn public_post(state: &State, post: &Post) -> bool {
    community(state, &post.community).is_some_and(|item| item.visibility != "private")
        && !(super::is_sensitive(state, post) && state.sensitive == "hide")
}

fn posts<'a>(state: &'a State, tab: &str, public: bool) -> Vec<&'a Post> {
    if tab == "history" {
        return state
            .history
            .iter()
            .filter_map(|id| visible_post(state, *id))
            .collect();
    }
    let mut list: Vec<_> = state
        .posts
        .iter()
        .filter(|post| {
            let in_tab = match tab {
                "saved" => state.saved.contains(&post.id),
                "hidden" => state.hidden.contains(&post.id),
                "upvoted" => state.votes.get(&post.id) == Some(&1),
                "downvoted" => state.votes.get(&post.id) == Some(&-1),
                _ => {
                    post.author == state.username
                        && (!public
                            || state.public_posts
                                && !state.profile_hidden_posts.contains(&post.id)
                                && public_post(state, post))
                }
            };
            in_tab && can_view_post(state, post)
        })
        .collect();
    list.sort_by_key(|post| Reverse(post.id));
    list
}

fn comments(state: &State, public: bool) -> Vec<&Comment> {
    if public && !state.public_comments {
        return Vec::new();
    }
    let mut list: Vec<_> = state
        .comments
        .iter()
        .filter(|item| {
            !item.deleted
                && !state.is_comment_removed(item.id)
                && item.author == state.username
                && visible_post(state, item.post).is_some_and(|post| {
                    !public
                        || !state.profile_hidden_comments.contains(&item.id)
                            && public_post(state, post)
                })
        })
        .collect();
    list.sort_by_key(|item| Reverse(item.id));
    list
}

fn add_posts<'a>(
    html: &mut String,
    state: &State,
    posts: impl Iterator<Item = &'a Post>,
    back: &str,
    public: bool,
) {
    for post in posts {
        if public {
            let group = community(state, &post.community)
                .map(|item| item.name.as_str())
                .unwrap_or(&post.community);
            write!(html, "<article class='panel profile-item'><a class='subtle-link' href='{}'>{}</a><h2><a href='/thread?id={}'>{}</a></h2><p class='muted'>{} comments</p></article>", community_url(&post.community), escape(group), post.id, escape(&post.title), state.visible_reply_count(post)).unwrap();
        } else {
            html.push_str(&post_card(state, post, back, false, false));
        }
    }
}

fn add_comments<'a>(html: &mut String, state: &State, comments: impl Iterator<Item = &'a Comment>) {
    for item in comments {
        let Some(post) = visible_post(state, item.post) else {
            continue;
        };
        let content = if post.sensitive
            || post.spoiler
            || community(state, &post.community).is_some_and(|group| group.mature)
        {
            format!("<details class='sensitive-preview'><summary>Reveal comment</summary><p>{}</p></details>", escape(&item.body))
        } else {
            format!("<p>{}</p>", escape(&item.body))
        };
        write!(html, "<article class='panel profile-item'><div class='profile-comment-meta'><a class='subtle-link' href='/thread?id={}#comment-{}'>{}</a><span class='muted'>{}</span></div>{content}</article>", post.id, item.id, escape(&post.title), escape(&item.age)).unwrap();
    }
}

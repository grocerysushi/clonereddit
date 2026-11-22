use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use super::{can_view_post, encode, escape, icon, score, visible_post};
use crate::model::{Community, Post, State};

pub(super) fn render(state: &State, section: &str) -> String {
    let (title, body, can_publish) = if section == "trends" {
        ("Trends", trends(state), false)
    } else {
        let mut posts: Vec<&Post> = state
            .posts
            .iter()
            .filter(|post| post.author == state.username && can_view_post(state, post))
            .collect();
        posts.sort_by_key(|post| Reverse(post.id));
        match section {
            "performance" => ("Performance", performance(state, &posts), false),
            "content" => ("Content", content(state, &posts), true),
            _ => ("Dashboard", dashboard(state, &posts), true),
        }
    };
    let publish = if can_publish {
        format!(
            "<a class='button dark' href='/new'>{}<span>New post</span></a>",
            icon("plus")
        )
    } else {
        String::new()
    };

    format!(
        "<section class='panel standalone creator-tool'><header class='creator-header'><h1 class='creator-heading'>{title}</h1>{publish}</header>{body}</section>"
    )
}

fn dashboard(state: &State, posts: &[&Post]) -> String {
    let replies = state
        .comments
        .iter()
        .filter(|comment| {
            comment.author == state.username
                && !comment.deleted
                && !state.is_comment_removed(comment.id)
                && visible_post(state, comment.post).is_some()
        })
        .count();
    let joined = state
        .communities
        .iter()
        .filter(|item| state.joined.contains(&item.slug) && state.can_view_community(item))
        .count();
    let mut html = format!(
        "<dl class='creator-stats'><div><dt>Posts published</dt><dd>{}</dd></div><div><dt>Replies written</dt><dd>{replies}</dd></div><div><dt>Communities joined</dt><dd>{}</dd></div></dl><h2>Latest posts</h2>",
        posts.len(),
        joined
    );
    if posts.is_empty() {
        html.push_str("<p class='muted creator-muted'>Your latest posts will appear here.</p>");
    } else {
        html.push_str("<ul class='creator-list'>");
        for post in posts.iter().take(4) {
            write!(
                html,
                "<li class='creator-row'>{}</li>",
                post_details(state, post)
            )
            .unwrap();
        }
        html.push_str("</ul>");
    }
    html
}

fn performance(state: &State, posts: &[&Post]) -> String {
    if posts.is_empty() {
        return "<p class='muted creator-muted'>Publish a post to see its vote score and replies.</p>".into();
    }

    let mut html = String::from("<ul class='creator-list' aria-label='Performance by post'>");
    for post in posts {
        write!(
            html,
            "<li class='creator-row'>{}<dl class='creator-row-details'><div><dt>Vote score</dt><dd>{}</dd></div><div><dt>Replies</dt><dd>{}</dd></div></dl></li>",
            post_details(state, post),
            score(state, post),
            state.visible_reply_count(post)
        )
        .unwrap();
    }
    html.push_str("</ul>");
    html
}

fn trends(state: &State) -> String {
    let mut totals: BTreeMap<&str, (usize, i64)> = BTreeMap::new();
    for post in state.posts.iter().filter(|post| can_view_post(state, post)) {
        let (discussions, votes) = totals.entry(&post.community).or_default();
        *discussions += 1;
        *votes += i64::from(score(state, post));
    }
    let mut communities: Vec<(&Community, usize, i64)> = state
        .communities
        .iter()
        .filter(|item| state.can_view_community(item))
        .map(|community| {
            let (discussions, votes) = totals
                .get(community.slug.as_str())
                .copied()
                .unwrap_or_default();
            (community, discussions, votes)
        })
        .collect();
    communities.sort_by_key(|(community, discussions, votes)| {
        (
            Reverse(*discussions),
            Reverse(*votes),
            community.slug.as_str(),
        )
    });

    let mut html = String::from(
        "<p class='creator-summary muted'>Communities ranked by total discussions, then net votes.</p><ol class='creator-list' aria-label='Community rankings'>",
    );
    for (community, discussions, votes) in communities {
        write!(
            html,
            "<li class='creator-row'><div class='creator-row-main'><a class='subtle-link' href='/?view={}'>{}</a></div><dl class='creator-row-details'><div><dt>Discussions</dt><dd>{discussions}</dd></div><div><dt>Net votes</dt><dd>{votes}</dd></div></dl></li>",
            encode(&community.slug),
            escape(&community.name)
        )
        .unwrap();
    }
    html.push_str("</ol>");
    html
}

fn content(state: &State, posts: &[&Post]) -> String {
    if posts.is_empty() {
        return "<p class='muted creator-muted'>Your published posts will appear here.</p>".into();
    }

    let mut html = String::from("<ul class='creator-list' aria-label='Your published posts'>");
    for post in posts {
        let edit = if super::can_edit_post(state, post) {
            format!("<div class='creator-row-actions'><a class='button light' href='/edit?id={}' aria-label='Edit {}'>Edit</a></div>", post.id, escape(&post.title))
        } else {
            String::new()
        };
        write!(
            html,
            "<li class='creator-row'>{}{edit}</li>",
            post_details(state, post)
        )
        .unwrap();
    }
    html.push_str("</ul>");
    html
}

fn post_details(state: &State, post: &Post) -> String {
    let community = state
        .communities
        .iter()
        .find(|community| community.slug == post.community)
        .map(|community| community.name.as_str())
        .unwrap_or(&post.community);
    format!(
        "<div class='creator-row-main'><a class='subtle-link' href='/thread?id={}'>{}</a><p class='muted'>{}</p></div>",
        post.id,
        escape(&post.title),
        escape(community)
    )
}

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use super::{can_view_post, display_post_title, encode, escape, icon, score, visible_post};
use crate::model::{Community, Post, State};
use crate::schedule_time;

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

pub(super) fn insights(state: &State, post: &Post, range: &str) -> String {
    let days = if range == "30" { 30 } else { 7 };
    let history = state.post_insight_days(post, days, schedule_time::now());
    let mut html = format!(
        "<section class='panel standalone creator-tool creator-insights'><header class='creator-header'><h1 class='creator-heading'>Post insights</h1><a class='button light' href='/creator?section=performance'>Back to performance</a></header><p class='creator-insight-post'><a class='subtle-link' href='/thread?id={id}'>{title}</a></p><nav class='creator-insight-ranges' aria-label='Insights date range'>",
        id = post.id,
        title = escape(display_post_title(state, post))
    );
    for choice in [7, 30] {
        let selected = if choice == days {
            " aria-current='page'"
        } else {
            ""
        };
        write!(html, "<a class='button light' href='/creator?section=insights&amp;id={}&amp;range={choice}'{selected}>Last {choice} days</a>", post.id).unwrap();
    }
    html.push_str("</nav>");
    if history.is_empty() {
        html.push_str(
            "<p class='muted'>No insights have been recorded in this date range.</p></section>",
        );
        return html;
    }

    let (views, votes, comments) =
        history
            .iter()
            .fold((0_u64, 0_i64, 0_u64), |(views, votes, comments), day| {
                (
                    views.saturating_add(day.views),
                    votes.saturating_add(day.votes),
                    comments.saturating_add(day.comments),
                )
            });
    let formatted_votes = vote_change(votes);
    write!(
        html,
        "<dl class='creator-stats creator-insight-totals' aria-label='Totals for the selected date range'><div data-metric='views'><dt>Views</dt><dd>{views}</dd></div><div data-metric='votes'><dt>Net votes</dt><dd>{formatted_votes}</dd></div><div data-metric='comments'><dt>Comments added</dt><dd>{comments}</dd></div></dl>"
    ).unwrap();
    html.push_str("<p class='muted creator-insight-note' id='creator-insight-note'>Views count discussion opens by other readers, including guests. Net votes reflect new votes, changes, and clears. Comments added includes replies. Dates are in UTC, and totals cover tracked days.</p><table class='creator-insight-days' aria-describedby='creator-insight-note'><caption>Daily activity</caption><thead><tr><th scope='col'>Date</th><th scope='col'>Views</th><th scope='col'>Net votes</th><th scope='col'>Comments</th></tr></thead><tbody>");

    let (max_views, max_votes, max_comments) =
        history
            .iter()
            .fold((1, 1, 1), |(views, votes, comments), day| {
                (
                    views.max(day.views),
                    votes.max(day.votes.unsigned_abs()),
                    comments.max(day.comments),
                )
            });
    for day in history {
        let utc = schedule_time::to_local(day.day.saturating_mul(86_400), "UTC");
        let day_label = escape(utc.split('T').next().unwrap_or_default());
        write!(html, "<tr data-insight-day='{day_label}'><th scope='row'><time datetime='{day_label}'>{day_label}</time></th>").unwrap();
        insight_cell(
            &mut html,
            "views",
            &day.views.to_string(),
            day.views,
            max_views,
            false,
        );
        insight_cell(
            &mut html,
            "votes",
            &vote_change(day.votes),
            day.votes.unsigned_abs(),
            max_votes,
            day.votes < 0,
        );
        insight_cell(
            &mut html,
            "comments",
            &day.comments.to_string(),
            day.comments,
            max_comments,
            false,
        );
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></section>");
    html
}

fn vote_change(delta: i64) -> String {
    if delta > 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    }
}

fn insight_cell(html: &mut String, metric: &str, value: &str, bar: u64, max: u64, lost: bool) {
    let class = if lost { " class='is-negative'" } else { "" };
    write!(html, "<td data-metric='{metric}'><span class='creator-insight-value'>{value}</span><meter aria-hidden='true' min='0' max='{max}' value='{bar}'{class}></meter></td>").unwrap();
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
        "<div class='creator-row-main'><a class='subtle-link' href='/thread?id={id}'>{title}</a><p class='muted creator-row-context'>{community}<a class='subtle-link creator-insights-link' href='/creator?section=insights&amp;id={id}' aria-label='Insights for {title}'>Insights</a></p></div>",
        id = post.id,
        title = escape(display_post_title(state, post)),
        community = escape(community)
    )
}

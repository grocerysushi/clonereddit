use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use super::{action_fields, community, community_url, encode, escape, field, icon, is_sensitive};
use crate::model::{Post, State};

const TABS: &[(&str, &str)] = &[
    ("text", "Text and link"),
    ("media", "Media"),
    ("poll", "Poll"),
    ("repost", "Repost"),
];
const RECENT_SOURCES: usize = 80;

pub(super) fn tabs(selected: &str, community: &str) -> String {
    let selected = match selected {
        "media" | "poll" | "repost" => selected,
        _ => "text",
    };
    let mut html = String::from("<nav class='publishing-tabs' aria-label='Post type'>");
    for (kind, label) in TABS {
        let current = if *kind == selected {
            " aria-current='page'"
        } else {
            ""
        };
        write!(
            html,
            "<a href='{}'{current}>{label}</a>",
            escape(&new_url(kind, community))
        )
        .unwrap();
    }
    html.push_str("</nav>");
    html
}

fn new_url(kind: &str, community: &str) -> String {
    match (kind, community.is_empty()) {
        ("text", true) => "/new".into(),
        ("text", false) => format!("/new?community={}", encode(community)),
        (_, true) => format!("/new?type={kind}"),
        (_, false) => format!("/new?type={kind}&community={}", encode(community)),
    }
}

pub(super) fn editor(
    state: &State,
    query: &BTreeMap<String, String>,
    form: Option<&BTreeMap<String, String>>,
    error: &str,
) -> Option<String> {
    let entered = |name| form.map_or("", |data| field(data, name));
    let kind = match entered("action") {
        "create-poll" => "poll",
        "create-repost" => "repost",
        _ => field(query, "type"),
    };
    if !matches!(kind, "media" | "poll" | "repost") {
        return None;
    }
    let requested = form.map_or_else(
        || field(query, "community"),
        |data| field(data, "community"),
    );
    let selected = community(state, requested)
        .filter(|item| state.can_contribute(item))
        .map_or("", |item| item.slug.as_str());
    let cancel = if selected.is_empty() {
        "/".to_owned()
    } else {
        community_url(selected)
    };
    let back = new_url(kind, selected);
    let alert = if error.is_empty() {
        String::new()
    } else {
        let reselect = if kind == "media" {
            " Select your files again."
        } else {
            ""
        };
        format!(
            "<p class='notice publishing-error' role='alert'>{}{reselect}</p>",
            escape(error)
        )
    };
    let destination = community_selector(state, selected, kind == "repost");
    let title = title_field(entered("title"), kind == "repost");
    let body = format!(
        "<label>Body text <small>Optional</small><textarea name='body' rows='5' maxlength='8000' placeholder='Add context'>{}</textarea></label>",
        escape(entered("body"))
    );
    let (heading, submit, endpoint, enctype, fields, content) = match kind {
        "media" => {
            let media = "<div class='publishing-media-upload'><label>Images or video<input type='file' name='post_media' accept='image/png,image/jpeg,image/webp,image/gif,video/mp4,video/webm' multiple required aria-describedby='publishing-media-help'></label><small id='publishing-media-help'>Choose up to six PNG, JPEG, WebP, or GIF images of 2 MiB each, or one MP4 or WebM video of up to 16 MiB.</small></div>";
            (
                "New media post",
                "Publish media post",
                "/posts/media/new",
                " enctype='multipart/form-data'",
                String::new(),
                format!(
                    "{destination}{title}{media}{body}{}",
                    post_details(state, form)
                ),
            )
        }
        "poll" => (
            "New poll",
            "Publish poll",
            "/action",
            "",
            action_fields("create-poll", &back, &[]),
            format!(
                "{destination}{title}{body}{}{}",
                poll_fields(form),
                post_details(state, form)
            ),
        ),
        "repost" => {
            let source = form.map_or_else(|| field(query, "source"), |data| field(data, "source"));
            let sources = source_selector(state, source.trim().parse().ok());
            (
                "Repost a discussion",
                "Publish repost",
                "/action",
                "",
                action_fields("create-repost", &back, &[]),
                format!("{sources}{destination}{title}{body}"),
            )
        }
        _ => return None,
    };
    let tabs = tabs(kind, selected);
    Some(format!(
        "<section class='panel standalone composer publishing-composer'><h1>{heading}</h1>{tabs}{alert}<form class='stack-form publishing-form' method='post' action='{endpoint}'{enctype}>{fields}{content}<div class='form-actions'><button class='button dark' type='submit'>{submit}</button><a class='button light' href='{}'>Cancel</a></div></form></section>",
        escape(&cancel)
    ))
}

fn community_selector(state: &State, selected: &str, repost: bool) -> String {
    let (label, help) = if repost {
        (
            "Destination community",
            "<small>Choose a different community from the original.</small>",
        )
    } else {
        ("Community", "")
    };
    let mut options = String::new();
    for item in state
        .communities
        .iter()
        .filter(|item| state.can_contribute(item))
    {
        write!(
            options,
            "<option value='{}'{}>{}</option>",
            escape(&item.slug),
            selected_attribute(item.slug == selected),
            escape(&item.name)
        )
        .unwrap();
    }
    let placeholder = if options.is_empty() {
        "No communities available for posting"
    } else {
        "Choose a community"
    };
    format!(
        "<label>{label}<select name='community' required><option value=''>{placeholder}</option>{options}</select>{help}</label>"
    )
}

fn title_field(title: &str, optional: bool) -> String {
    let (hint, required, placeholder) = if optional {
        (" <small>Optional</small>", "", "Use the original title")
    } else {
        ("", " required", "What are we discussing?")
    };
    format!(
        "<label>Title{hint}<input name='title' maxlength='160' value='{}' placeholder='{placeholder}'{required}></label>",
        escape(title)
    )
}

fn post_details(state: &State, form: Option<&BTreeMap<String, String>>) -> String {
    let flair = form.map_or(state.profile_flair.as_str(), |data| field(data, "flair"));
    let tags = [
        ("sensitive", "NSFW (18+)"),
        ("spoiler", "Spoiler"),
        ("brand_affiliate", "Brand affiliate"),
    ];
    let open = if tags
        .iter()
        .any(|(name, _)| form.is_some_and(|data| field(data, name) == "on"))
    {
        " open"
    } else {
        ""
    };
    let inputs: String = tags
        .into_iter()
        .map(|(name, label)| {
            let checked = if form.is_some_and(|data| field(data, name) == "on") {
                " checked"
            } else {
                ""
            };
            format!(
                "<label class='composer-toggle'><strong>{label}</strong><input type='checkbox' name='{name}'{checked}></label>"
            )
        })
        .collect();
    format!(
        "<label>Post flair <small>Optional</small><input name='flair' maxlength='32' value='{}'></label><details class='composer-tags'{open}><summary>{} Add tags</summary><div class='composer-tag-options'>{inputs}</div></details>",
        escape(flair),
        icon("tag")
    )
}

fn poll_fields(form: Option<&BTreeMap<String, String>>) -> String {
    let entered = |name: &str| form.map_or("", |data| field(data, name));
    let choice = |number| {
        let key = format!("option_{number}");
        let required = if number <= 2 { " required" } else { "" };
        format!(
            "<label>Choice {number}<input name='{key}' maxlength='80' value='{}'{required}></label>",
            escape(entered(&key))
        )
    };
    let more_open = if (3..=6).any(|number| !entered(&format!("option_{number}")).is_empty()) {
        " open"
    } else {
        ""
    };
    let additional = (3..=6).map(choice).collect::<String>();
    let duration = match entered("duration") {
        "1" => "1",
        "7" => "7",
        _ => "3",
    };
    let durations: String = [("1", "1 day"), ("3", "3 days"), ("7", "7 days")]
        .into_iter()
        .map(|(value, label)| {
            format!(
                "<option value='{value}'{}>{label}</option>",
                selected_attribute(value == duration)
            )
        })
        .collect();
    format!(
        "<fieldset class='publishing-poll-choices'><legend>Poll choices</legend>{}{}<details class='publishing-extra-options'{more_open}><summary>Add more choices</summary>{additional}</details></fieldset><label>Voting duration<select name='duration'>{durations}</select></label>",
        choice(1),
        choice(2)
    )
}

fn source_selector(state: &State, selected: Option<u64>) -> String {
    let mut sources: Vec<&Post> = state
        .posts
        .iter()
        .filter(|post| {
            post.repost_of.is_none()
                && community(state, &post.community).is_some_and(|item| {
                    item.visibility != "private" && state.can_view_community(item)
                })
                && (selected == Some(post.id)
                    || !state.hidden.contains(&post.id)
                        && !state.blocked.contains(&post.author)
                        && !(state.sensitive == "hide" && is_sensitive(state, post)))
        })
        .collect();
    sources.sort_by_key(|post| (selected != Some(post.id), Reverse(post.id)));
    let mut options = String::new();
    for post in sources.into_iter().take(RECENT_SOURCES) {
        let title = if post.title.is_empty() {
            "Original discussion"
        } else {
            &post.title
        };
        write!(
            options,
            "<option value='{}'{}>{} · c/{}</option>",
            post.id,
            selected_attribute(selected == Some(post.id)),
            escape(title),
            escape(&post.community)
        )
        .unwrap();
    }
    let placeholder = if options.is_empty() {
        "No original posts available"
    } else {
        "Choose an original post"
    };
    format!(
        "<label class='publishing-source'>Original post<select name='source' required><option value=''>{placeholder}</option>{options}</select></label>"
    )
}

fn selected_attribute(selected: bool) -> &'static str {
    if selected {
        " selected"
    } else {
        ""
    }
}

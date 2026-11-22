use std::collections::BTreeMap;
use std::fmt::Write;

use super::{
    action_fields, community, escape, field, hidden, icon, post_body_editor, post_flair, publishing,
};
use crate::model::{Post, PostDraft, State};

const DRAFTS_URL: &str = "/new?drafts=1";

pub(super) fn drafts_link() -> &'static str {
    "<a class='button light' href='/new?drafts=1'>Drafts</a>"
}

pub(super) fn drafts(state: &State, confirm: Option<&PostDraft>) -> String {
    let mut html = String::from(
        "<section class='panel standalone composer-drafts'><header class='creator-header'><h1>Drafts</h1><a class='button light' href='/new'>New post</a></header>",
    );
    if let Some(draft) = confirm {
        let fields = action_fields(
            "delete-draft",
            DRAFTS_URL,
            &[("draft_id", &draft.id.to_string())],
        );
        write!(
            html,
            "<section class='draft-discard-confirmation notice publishing-error' aria-labelledby='discard-draft-heading'><h2 id='discard-draft-heading'>Discard draft?</h2><p>{}</p><form class='form-actions' method='post' action='/action'>{fields}<a class='button light' href='{DRAFTS_URL}'>Cancel</a><button class='button dark' type='submit'>Discard draft</button></form></section>",
            escape(draft_title(draft)),
        )
        .unwrap();
    }
    if state.drafts.is_empty() {
        html.push_str("<p>No drafts yet.</p>");
    } else {
        html.push_str("<ul class='draft-list'>");
        for draft in state.drafts.iter().rev() {
            let id = draft.id;
            let title = escape(draft_title(draft));
            let destination = if draft.community.is_empty() {
                "No community selected"
            } else {
                community(state, &draft.community)
                    .filter(|item| state.can_view_community(item))
                    .map_or("Community unavailable", |item| item.name.as_str())
            };
            let preview = if draft.title.is_empty() {
                let text = if draft.body.is_empty() {
                    &draft.url
                } else {
                    &draft.body
                };
                let excerpt: String = text.chars().take(100).collect();
                let suffix = if excerpt.len() < text.len() {
                    "…"
                } else {
                    ""
                };
                format!("<p class='draft-preview'>{}{suffix}</p>", escape(&excerpt))
            } else {
                String::new()
            };
            write!(html, "<li class='draft-entry' data-draft-id='{id}'><div class='draft-entry-summary'><h2>{title}</h2>{preview}<p class='muted'>{}</p></div><div class='draft-entry-actions'><a class='button light' href='/new?draft={id}' aria-label='Resume draft {title}'>Resume</a><a class='button light' href='/new?discard_draft={id}' aria-label='Discard draft {title}'>Discard</a></div></li>", escape(destination)).unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

fn draft_title(draft: &PostDraft) -> &str {
    if draft.title.is_empty() {
        "Untitled draft"
    } else {
        &draft.title
    }
}

pub(super) fn editor(
    state: &State,
    post: Option<&Post>,
    selected: &str,
    draft: Option<&PostDraft>,
    rejected: Option<(&BTreeMap<String, String>, &str)>,
) -> String {
    let (title, body, url, flair, tags) = match (rejected, post, draft) {
        (Some((form, _)), _, _) => (
            field(form, "title"),
            field(form, "body"),
            field(form, "url"),
            field(form, "flair"),
            ["sensitive", "spoiler", "brand_affiliate"].map(|key| field(form, key) == "on"),
        ),
        (_, Some(item), _) => (
            item.title.as_str(),
            item.body.as_str(),
            item.url.as_str(),
            item.flair.as_str(),
            [item.sensitive, item.spoiler, item.brand_affiliate],
        ),
        (_, _, Some(item)) => (
            item.title.as_str(),
            item.body.as_str(),
            item.url.as_str(),
            if item.flair_id.is_none() {
                item.flair.as_str()
            } else {
                ""
            },
            [item.sensitive, item.spoiler, item.brand_affiliate],
        ),
        _ => ("", "", "", state.profile_flair.as_str(), [false; 3]),
    };
    let selected = if let Some((form, _)) = rejected {
        field(form, "community")
    } else {
        draft.map_or(selected, |item| item.community.as_str())
    };
    let (heading, action, submit, back, cancel, draft_actions, tabs) = if let Some(post) = post {
        let thread = format!("/thread?id={}", post.id);
        (
            "Edit post",
            "edit",
            "Save changes",
            thread.clone(),
            thread,
            "",
            String::new(),
        )
    } else {
        let (heading, back, cancel) = if let Some(draft) = draft {
            (
                "Edit draft",
                format!("/new?draft={}", draft.id),
                DRAFTS_URL.to_owned(),
            )
        } else {
            ("New post", "/new".into(), "/".into())
        };
        (
            heading,
            "create",
            "Publish post",
            back,
            cancel,
            drafts_link(),
            publishing::tabs("text", selected),
        )
    };
    let id = post.map(|item| item.id.to_string()).unwrap_or_default();
    let mut fields = action_fields(action, &back, &[("id", &id), ("tags_present", "1")]);
    let discard = draft.map_or_else(String::new, |draft| {
        fields.push_str(&hidden("draft_id", &draft.id.to_string()));
        format!(
            "<a class='button light' href='/new?discard_draft={}'>Discard draft</a>",
            draft.id
        )
    });
    let (new_fields, save) = if post.is_none() {
        let selected_flair = if let Some((form, _)) = rejected {
            field(form, "flair_id").trim().parse().ok()
        } else {
            draft.and_then(|item| item.flair_id)
        };
        let mut options = String::new();
        let mut has_selected = selected.is_empty();
        for item in state
            .communities
            .iter()
            .filter(|item| state.can_contribute(item))
        {
            let current = if item.slug == selected {
                has_selected = true;
                " selected"
            } else {
                ""
            };
            write!(
                options,
                "<option value='{}'{current}>{}</option>",
                escape(&item.slug),
                escape(&item.name)
            )
            .unwrap();
        }
        let help = if has_selected {
            ""
        } else {
            "<small>Choose another community to publish this post.</small>"
        };
        let flair = post_flair::composer(state, selected, selected_flair, Some(flair));
        let stale_flair = if rejected.is_none()
            && selected_flair.is_some_and(|id| {
                community(state, selected).is_some_and(|item| {
                    state.can_contribute(item)
                        && !item.post_flairs().iter().any(|flair| flair.id == id)
                })
            }) {
            "<p class='notice' role='status'>Your saved post flair is no longer available.</p>"
        } else {
            ""
        };
        (
            format!("<label>Community<select name='community' required><option value=''>Choose a community</option>{options}</select>{help}</label>{stale_flair}{flair}"),
            "<button class='button light' type='submit' name='save_draft' value='1' formnovalidate>Save draft</button>",
        )
    } else {
        (String::new(), "")
    };
    let tag_fields = [
        ("sensitive", "NSFW (18+)", "Contains mature or adult content"),
        ("spoiler", "Spoiler", "Collapses the post body until someone reveals it"),
        ("brand_affiliate", "Brand affiliate", "Discloses that this post was made for a brand or business"),
    ].into_iter().zip(tags).map(|((name, label, help), checked)| {
        format!("<label class='composer-toggle'><span><strong>{label}</strong><small>{help}</small></span><input type='checkbox' name='{name}'{}></label>", if checked { " checked" } else { "" })
    }).collect::<String>();
    let alert = rejected.map_or_else(String::new, |(_, error)| {
        format!(
            "<p class='notice publishing-error' role='alert' tabindex='-1' autofocus>{}</p>",
            escape(error)
        )
    });
    let media_alternatives =
        post.filter(|post| !post.media.is_empty())
            .map_or_else(String::new, |post| {
                let saved_descriptions = post.media_alternatives.descriptions.join("\n");
                let descriptions = rejected.map_or(saved_descriptions.as_str(), |(form, _)| {
                    field(form, "media_descriptions")
                });
                let transcript = rejected
                    .map_or(post.media_alternatives.transcript.as_str(), |(form, _)| {
                        field(form, "media_transcript")
                    });
                publishing::media_alternative_fields(
                    descriptions,
                    transcript,
                    post.media[0].content_type.starts_with("video/"),
                )
            });
    format!(
        r#"<section class="panel standalone composer"><header class="creator-header"><h1>{heading}</h1>{draft_actions}</header>{tabs}{alert}<form class="stack-form" method="post" action="/action">{fields}{new_fields}
<label>Title<input name="title" required maxlength="160" value="{title}" placeholder="What are we discussing?"></label>
<details class="composer-tags"{tags_open}><summary>{tag_icon} Add tags</summary><div class="composer-tag-options">{tag_fields}</div></details>
{body}
{media_alternatives}
<label>Link <small>Optional</small><input name="url" type="url" maxlength="600" value="{url}" placeholder="https://"></label>
<div class="form-actions composer-actions"><a class="button light" href="{cancel}">Cancel</a><button class="button dark" type="submit">{submit}</button>{save}{discard}</div></form></section>"#,
        tag_icon = icon("tag"),
        tags_open = if tags.contains(&true) { " open" } else { "" },
        title = escape(title),
        body = post_body_editor::editor_with_inline_images(body, 7),
        url = escape(url),
        cancel = escape(&cancel),
    )
}

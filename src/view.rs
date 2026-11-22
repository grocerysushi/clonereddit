mod account;
mod community_appearance;
mod community_setup;
mod creator;
mod custom;
mod discussion;
mod moderation;
mod moderation_workspace;
mod profile;
mod profile_panel;
mod publishing;
mod restricted;
mod scheduled;
mod session;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

use crate::auth::AuthStore;
use crate::http::{Request, Response};
use crate::media;
use crate::model::{Comment, Community, CustomFeed, Post, PostDraft, ProfileAvatar, State};
use crate::schedule_time;

pub fn route(
    mut request: Request,
    state: &mut State,
    path: &Path,
    auth: &mut AuthStore,
) -> Response {
    if request.path.starts_with("/account/") {
        return session::route(&request, state, auth);
    }
    if state.username.is_empty()
        && (request.method == "POST" || session::requires_account(&request))
    {
        return Response::redirect("/account/login");
    }
    if let Some(profile_path) = request.path.strip_prefix("/u/") {
        return public_profile(state, auth, &request, profile_path);
    }
    if request.method == "POST" && request.path == "/posts/media/new" {
        return media_submission(request, state, path);
    }
    if request.method == "GET" && request.path == "/posts/media" {
        let asset = field(&request.query, "index")
            .parse::<usize>()
            .ok()
            .and_then(|index| {
                find_post(state, &request.query).and_then(|post| post.media.get(index))
            });
        return match asset.and_then(|asset| media::read(path, asset)) {
            Some((mime, bytes)) => Response::media(mime, bytes, request.range.as_deref()),
            None => Response::text(
                404,
                "text/plain; charset=utf-8",
                "Post attachment unavailable.",
            ),
        };
    }
    if request.method == "POST" && request.path == "/communities/appearance/settings" {
        return community_appearance::submit(request, state);
    }
    if request.method == "GET" && request.path == "/communities/appearance/review" {
        return community_appearance::review_page(state, &request.query);
    }
    if request.method == "POST" && request.path == "/communities/appearance" {
        let reference = field(&request.form, "artwork_preview");
        let original = reference.parse().ok();
        if !reference.is_empty() && original.is_none() {
            return wizard_page(state, &request.form, "");
        }
        let (icon_mime, icon_bytes) = request
            .community_uploads
            .remove("icon_image")
            .map(|asset| (asset.content_type, asset.bytes))
            .unzip();
        let (banner_mime, banner_bytes) = request
            .community_uploads
            .remove("banner_image")
            .map(|asset| (asset.content_type, asset.bytes))
            .unzip();
        let result = state.preview_artwork(
            original,
            icon_mime.as_deref().zip(icon_bytes),
            banner_mime.as_deref().zip(banner_bytes),
            field(&request.form, "remove_icon") == "on",
            field(&request.form, "remove_banner") == "on",
        );
        return match result {
            Ok(preview) => {
                if let Some(id) = preview {
                    request
                        .form
                        .insert("artwork_preview".into(), id.to_string());
                } else {
                    request.form.remove("artwork_preview");
                }
                wizard_page(state, &request.form, "")
            }
            Err(error) => {
                request.form.insert("setup_step".into(), "3".into());
                wizard_page(state, &request.form, &error)
            }
        };
    }
    if request.path == "/communities/preview-image" {
        let artwork = field(&request.query, "id")
            .parse()
            .ok()
            .and_then(|id| state.setup_artwork(id));
        return image_response(
            artwork.and_then(|item| match field(&request.query, "kind") {
                "icon" => item.icon.as_ref(),
                "banner" => item.banner.as_ref(),
                _ => None,
            }),
        );
    }
    if request.path == "/communities/image" {
        let selected = community(state, field(&request.query, "community"))
            .filter(|item| state.can_view_community(item));
        return image_response(
            selected.and_then(|item| match field(&request.query, "kind") {
                "icon" => item.icon_image.as_ref(),
                "banner" => item.banner_image.as_ref(),
                _ => None,
            }),
        );
    }
    if request.method == "POST" && request.path == "/profile/image" {
        let destination = "/profile/settings?section=images";
        let kind = field(&request.form, "kind");
        return match request
            .upload
            .take()
            .map(|upload| state.set_profile_image(kind, &upload.content_type, upload.bytes, path))
        {
            Some(Ok(notice)) => {
                Response::redirect(&format!("{destination}&notice={}", encode(notice)))
            }
            Some(Err(error)) => form_error(state, &error, destination),
            None => form_error(state, "Choose an image to upload.", destination),
        };
    }
    if matches!(request.path.as_str(), "/profile/avatar" | "/profile/cover") {
        let image = if request.path == "/profile/avatar" {
            &state.avatar
        } else {
            &state.cover
        };
        return match image {
            Some(asset) => Response::text(200, asset.content_type, asset.bytes.clone()),
            None => Response::text(
                404,
                "text/plain; charset=utf-8",
                "Profile image unavailable.",
            ),
        };
    }
    if request.method == "POST" && request.path == "/action" {
        let back = field(&request.form, "back");
        let back = if back.starts_with('/')
            && !back.starts_with("//")
            && !back.contains('\\')
            && !back.chars().any(char::is_control)
        {
            back
        } else {
            "/"
        };
        if field(&request.form, "review_community") == "1" {
            return wizard_page(state, &request.form, "");
        }
        let action = if field(&request.form, "save_draft") == "1" {
            "save-draft"
        } else {
            field(&request.form, "action")
        };
        if action == "profile" && field(&request.form, "username") != state.username {
            return form_error(state, "Your account username stays the same.", back);
        }
        if action == "send-direct-message" && !auth.has_account(field(&request.form, "recipient")) {
            return message_error(state, auth, &request.form, "Choose a registered recipient.");
        }
        if matches!(
            action,
            "schedule-item" | "edit-scheduled-item" | "cancel-scheduled-item"
        ) {
            return scheduled_submission(state, path, action, &request.form);
        }
        if matches!(action, "restrict-user" | "lift-restriction") {
            return restriction_submission(state, path, action, &request.form);
        }
        if matches!(action, "edit-comment" | "delete-comment") {
            return comment_submission(state, path, action, &request.form);
        }
        let deleted_media = if action == "delete" {
            find_post(state, &request.form)
                .map(|post| post.media.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        return match state.apply(action, &request.form, path) {
            Ok(notice) => {
                media::remove(path, &deleted_media);
                let target = match action {
                    "create" | "create-poll" | "create-repost" => state
                        .posts
                        .last()
                        .map(|post| format!("/thread?id={}", post.id)),
                    "create-community" => state
                        .communities
                        .last()
                        .map(|item| community_url(&item.slug)),
                    "create-feed" => state.custom_feeds.last().map(|item| custom_url(item.id)),
                    "save-draft" => Some("/new?draft=1".into()),
                    "send-direct-message" => Some(format!(
                        "/messages?peer={}",
                        encode(
                            &field(&request.form, "recipient")
                                .trim()
                                .to_ascii_lowercase()
                        )
                    )),
                    _ => None,
                }
                .unwrap_or_else(|| back.to_owned());
                let delimiter = if target.contains('?') { '&' } else { '?' };
                Response::redirect(&format!("{target}{delimiter}notice={}", encode(notice)))
            }
            Err(error) if action == "create-community" && back == "/communities/start" => {
                wizard_page(state, &request.form, &error)
            }
            Err(error) if action == "save-community-artwork" => community_appearance::error_page(
                state,
                field(&request.form, "community"),
                field(&request.form, "artwork_preview").parse().ok(),
                &error,
            ),
            Err(error) if matches!(action, "create-poll" | "create-repost") => {
                let kind = if action == "create-poll" {
                    "poll"
                } else {
                    "repost"
                };
                publishing_error(state, &request.form, kind, &error)
            }
            Err(error) if action == "send-direct-message" => {
                message_error(state, auth, &request.form, &error)
            }
            Err(error) => form_error(state, &error, back),
        };
    }

    let query = &request.query;
    let notice = field(query, "notice");
    let profile_tab = field(query, "tab");
    let (title, active, content) = match request.path.as_str() {
        "/" => {
            let current = field(query, "view");
            let active = if current.is_empty() { "home" } else { current };
            if active.starts_with("custom:") && custom_feed(state, active).is_none() {
                return missing(state);
            }
            if community(state, active).is_some_and(|item| !state.can_view_community(item)) {
                return missing(state);
            }
            if !state.username.is_empty()
                && field(query, "q").is_empty()
                && community(state, active).is_some()
            {
                state.record_visit(active, path);
            }
            let title = community(state, active)
                .map(|c| c.name.as_str())
                .or_else(|| custom_feed(state, active).map(|item| item.name.as_str()))
                .unwrap_or("The front page");
            (title, active, feed(state, query, active))
        }
        "/thread" => {
            if let Ok(id) = field(query, "id").parse() {
                if state
                    .posts
                    .iter()
                    .find(|post| post.id == id)
                    .and_then(|post| community(state, &post.community))
                    .is_some_and(|item| state.can_view_community(item))
                {
                    if !state.username.is_empty() {
                        state.record_post_visit(id, path);
                    }
                } else {
                    return missing(state);
                }
            }
            match find_post(state, query) {
                Some(post) => (
                    display_post_title(state, post),
                    post.community.as_str(),
                    thread(state, post, CommentAction::from_query(query)),
                ),
                None => return missing(state),
            }
        }
        "/new" => (
            "New post",
            "",
            publishing::editor(state, query, None, "").unwrap_or_else(|| {
                editor(
                    state,
                    None,
                    field(query, "community"),
                    (field(query, "draft") == "1")
                        .then_some(state.draft.as_ref())
                        .flatten(),
                )
            }),
        ),
        "/communities/start" => (
            "Start a community",
            "start-community",
            community_setup::render(query),
        ),
        "/communities/new" => (
            "Create community",
            "start-community",
            community_editor(state, None),
        ),
        "/communities/settings" => match community(state, field(query, "community")) {
            Some(item) if community_owner(state, item) => (
                "Community settings",
                item.slug.as_str(),
                community_appearance::settings_content(
                    state,
                    item,
                    field(query, "artwork_preview"),
                ),
            ),
            _ => return missing(state),
        },
        "/feeds/new" => ("Create custom feed", "feeds", custom::editor(state, None)),
        "/feeds/settings" => match field(query, "id")
            .parse::<u64>()
            .ok()
            .and_then(|id| state.custom_feeds.iter().find(|item| item.id == id))
        {
            Some(item) => (
                "Custom feed settings",
                "feeds",
                custom::editor(state, Some(item)),
            ),
            None => return missing(state),
        },
        "/creator" => {
            let section = match field(query, "section") {
                "performance" => "performance",
                "trends" => "trends",
                "content" => "content",
                _ => "dashboard",
            };
            ("Creator tools", section, creator::render(state, section))
        }
        "/moderation" => {
            let section = match field(query, "section") {
                "mail" => "mail",
                "mod-feed" => "mod-feed",
                "manage" => "manage",
                _ => "queue",
            };
            ("Moderation", section, moderation::render(state, section))
        }
        "/moderation/tools" => {
            let selected = field(query, "community");
            let item = if selected.is_empty() {
                state
                    .communities
                    .iter()
                    .find(|item| community_owner(state, item))
            } else {
                community(state, selected).filter(|item| community_owner(state, item))
            };
            match item {
                Some(item) => (
                    "Moderator workspace",
                    "mod-workspace",
                    moderation_workspace::render(state, item, query),
                ),
                None if selected.is_empty() => {
                    return Response::redirect("/moderation?section=manage")
                }
                None => return missing(state),
            }
        }
        "/moderation/contact" => {
            match community(state, field(query, "community")).filter(|item| item.owner.is_some() && state.can_view_community(item)) {
                Some(item) if state.is_mod_mail_muted(&item.slug, &state.username) => (
                    "Mod mail unavailable",
                    item.slug.as_str(),
                    format!("<section class='panel standalone'><h1>Mod mail unavailable</h1><p>Your account is muted from sending mod mail to this community.</p><a class='button light' href='{}'>Back to community</a></section>", community_url(&item.slug)),
                ),
                Some(item) => (
                    "Message moderators",
                    item.slug.as_str(),
                    moderation::contact(item),
                ),
                None => return missing(state),
            }
        }
        "/edit" => match find_post(state, query).filter(|post| can_edit_post(state, post)) {
            Some(post) => (
                "Edit post",
                "",
                editor(state, Some(post), &post.community, None),
            ),
            None => return missing(state),
        },
        "/settings" => ("Feed controls", "settings", preferences(state)),
        "/messages" => {
            let recipients = auth.usernames().map(str::to_owned).collect::<Vec<_>>();
            (
                "Messages",
                "messages",
                account::inbox(state, query, &recipients),
            )
        }
        "/notifications" => (
            "Notifications",
            "notifications",
            account::notifications(state),
        ),
        "/policy" => ("Moderation policy", "policy", policy()),
        "/reports" => ("Legal review log", "policy", reports(state)),
        "/report" => match find_post(state, query) {
            Some(post) => ("Report a legal concern", "policy", report_form(post)),
            None => return missing(state),
        },
        "/profile" => (
            "Your profile",
            "profile",
            profile::render(state, profile_tab, false, false),
        ),
        "/profile/settings" => (
            "Profile settings",
            "profile",
            profile_panel::settings(state, field(query, "section")),
        ),
        "/profile/achievements" => (
            "Trophy case",
            "profile",
            format!(
                "<section class='panel standalone'><h1>Trophy case</h1>{}</section>",
                profile_panel::trophies(state)
            ),
        ),
        "/feeds/shared" => match field(query, "id")
            .parse::<u64>()
            .ok()
            .and_then(|id| state.shared_custom_feed(id))
            .filter(|feed| !feed.private)
        {
            Some(item) => (item.name.as_str(), "public-feed", shared_feed(state, item)),
            None => return missing(state),
        },
        _ => return missing(state),
    };
    let right = (active == "profile")
        .then(|| profile_panel::render(state, &request.origin, false, profile_tab));
    Response::html(layout_with_rail(
        state,
        active,
        title,
        &content,
        field(query, "q"),
        notice,
        right,
    ))
}

fn message_error(
    state: &State,
    auth: &AuthStore,
    form: &BTreeMap<String, String>,
    error: &str,
) -> Response {
    let recipients = auth.usernames().map(str::to_owned).collect::<Vec<_>>();
    let query = BTreeMap::from([("peer".into(), field(form, "recipient").to_owned())]);
    let content = account::inbox_draft(state, &query, &recipients, field(form, "body"), error);
    Response::html(layout(state, "messages", "Messages", &content, "", ""))
}

fn public_profile(
    viewer: &State,
    auth: &AuthStore,
    request: &Request,
    profile_path: &str,
) -> Response {
    let (handle, image) = profile_path.split_once('/').unwrap_or((profile_path, ""));
    if handle.is_empty() || handle != handle.to_ascii_lowercase() {
        return missing(viewer);
    }
    let registered = auth.has_account(handle);
    if !registered && !viewer.has_profile(handle) {
        return missing(viewer);
    }
    let Some(mut profile) = viewer.profile_snapshot(handle) else {
        return missing(viewer);
    };
    profile.sensitive.clone_from(&viewer.sensitive);
    if !image.is_empty() {
        let asset = match image {
            "avatar" => profile.avatar.as_ref(),
            "cover" => profile.cover.as_ref(),
            _ => None,
        };
        return image_response(
            asset.filter(|_| !profile.profile_nsfw || viewer.sensitive == "show"),
        );
    }
    let tab = field(&request.query, "tab");
    let can_message = registered && !viewer.username.is_empty() && viewer.username != handle;
    let content = profile::render(&profile, tab, true, can_message);
    Response::html(layout_with_rail(
        viewer,
        "public-profile",
        profile_name(&profile),
        &content,
        "",
        "",
        Some(profile_panel::render(&profile, &request.origin, true, tab)),
    ))
}

fn comment_submission(
    state: &mut State,
    path: &Path,
    action: &str,
    form: &BTreeMap<String, String>,
) -> Response {
    let Ok(id) = field(form, "id").parse::<u64>() else {
        return missing(state);
    };
    let Some(comment) = state
        .comments
        .iter()
        .find(|comment| comment.id == id && !comment.deleted && comment.author == state.username)
    else {
        return missing(state);
    };
    let post_id = comment.post;
    let Some(post) = state.posts.iter().find(|post| post.id == post_id) else {
        return missing(state);
    };
    if !community(state, &post.community).is_some_and(|item| state.can_view_community(item)) {
        return missing(state);
    }
    let back = format!("/thread?id={post_id}");
    let editing = action == "edit-comment";
    let can_edit = can_write_here(state, &post.community);
    match state.apply(action, form, path) {
        Ok(notice) => {
            let anchor = if editing {
                format!("comment-{id}")
            } else {
                "comments".into()
            };
            Response::redirect(&format!("{back}&notice={}#{anchor}", encode(notice)))
        }
        Err(error) if editing && !can_edit => form_error(state, &error, &back),
        Err(error) => {
            let Some(post) = state.posts.iter().find(|post| post.id == post_id) else {
                return missing(state);
            };
            let interaction = if editing {
                CommentAction::Edit {
                    id,
                    draft: Some(field(form, "body")),
                    error: &error,
                }
            } else {
                CommentAction::Delete { id, error: &error }
            };
            let body = thread(state, post, interaction);
            Response::html(layout(
                state,
                &post.community,
                display_post_title(state, post),
                &body,
                "",
                "",
            ))
        }
    }
}

fn media_submission(request: Request, state: &mut State, path: &Path) -> Response {
    let form = &request.form;
    if !can_write_here(state, field(form, "community")) {
        return publishing_error(
            state,
            form,
            "media",
            "Choose a community where you can publish.",
        );
    }
    let attachments = match media::store(path, request.post_uploads) {
        Ok(attachments) => attachments,
        Err(error) => return publishing_error(state, form, "media", &error),
    };
    match state.create_media_post(form, attachments.clone(), path) {
        Ok(notice) => {
            let Some(post) = state.posts.last() else {
                return Response::redirect("/");
            };
            Response::redirect(&format!("/thread?id={}&notice={}", post.id, encode(notice)))
        }
        Err(error) => {
            media::remove(path, &attachments);
            publishing_error(state, form, "media", &error)
        }
    }
}

fn publishing_error(
    state: &State,
    form: &BTreeMap<String, String>,
    kind: &str,
    error: &str,
) -> Response {
    let query = BTreeMap::from([("type".into(), kind.into())]);
    let body = publishing::editor(state, &query, Some(form), error).unwrap_or_default();
    Response::html(layout(state, "", "Check your post", &body, "", ""))
}

fn restriction_submission(
    state: &mut State,
    path: &Path,
    action: &str,
    form: &BTreeMap<String, String>,
) -> Response {
    let slug = field(form, "community");
    if community(state, slug).is_none_or(|item| !community_owner(state, item)) {
        return missing(state);
    }
    match state.apply(action, form, path) {
        Ok(notice) => Response::redirect(&format!(
            "/moderation/tools?community={}&section=restricted-users&notice={}",
            encode(slug),
            encode(notice)
        )),
        Err(error) => {
            let Some(item) = community(state, slug) else {
                return missing(state);
            };
            let body = moderation_workspace::restriction_error(state, item, form, &error);
            Response::html(layout(
                state,
                "mod-workspace",
                "Restricted Users",
                &body,
                "",
                "",
            ))
        }
    }
}

fn scheduled_submission(
    state: &mut State,
    path: &Path,
    action: &str,
    form: &BTreeMap<String, String>,
) -> Response {
    let slug = field(form, "community");
    if community(state, slug).is_none_or(|item| !community_owner(state, item)) {
        return missing(state);
    }

    let back = format!(
        "/moderation/tools?community={}&section=scheduled",
        encode(slug)
    );
    let result = if action == "cancel-scheduled-item" {
        state.apply(action, form, path)
    } else {
        normalize_schedule_times(form).and_then(|normalized| state.apply(action, &normalized, path))
    };
    match result {
        Ok(notice) => Response::redirect(&format!("{back}&notice={}", encode(notice))),
        Err(error) if action == "cancel-scheduled-item" => form_error(state, &error, &back),
        Err(error) => {
            let Some(item) = community(state, slug) else {
                return missing(state);
            };
            let body = moderation_workspace::schedule_error(state, item, form, &error);
            Response::html(layout(
                state,
                "mod-workspace",
                "Scheduled posts and events",
                &body,
                "",
                "",
            ))
        }
    }
}

fn normalize_schedule_times(
    form: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut normalized = form.clone();
    let zone = field(form, "zone");
    let zone = if zone.is_empty() {
        "America/Chicago"
    } else {
        zone
    };
    let publish_at = schedule_time::parse_local(field(form, "publish_at"), zone)
        .map_err(|error| format!("Publish time. {error}"))?;
    normalized.insert("publish_at".into(), publish_at.to_string());
    normalized.insert("zone".into(), zone.into());
    for (key, label) in [("event_start", "Event start"), ("event_end", "Event end")] {
        let date = field(form, key);
        if date.is_empty() || field(form, "kind") != "event" {
            normalized.remove(key);
        } else {
            let time = schedule_time::parse_local(date, zone)
                .map_err(|error| format!("{label}. {error}"))?;
            normalized.insert(key.into(), time.to_string());
        }
    }
    Ok(normalized)
}

fn wizard_page(state: &State, form: &BTreeMap<String, String>, error: &str) -> Response {
    let reference = field(form, "artwork_preview");
    let artwork = reference
        .parse()
        .ok()
        .and_then(|id| state.setup_artwork(id));
    let mut reset = BTreeMap::new();
    let (form, error) = if !reference.is_empty() && artwork.is_none() {
        reset.clone_from(form);
        reset.remove("artwork_preview");
        reset.insert("setup_step".into(), "3".into());
        (
            &reset,
            "The image preview has expired. Select your images again.",
        )
    } else {
        (form, error)
    };
    let alert = if error.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='notice setup-error' role='alert'>{}</p>",
            escape(error)
        )
    };
    let body = alert + &community_setup::review(form, artwork);
    Response::html(layout(
        state,
        "start-community",
        "Community setup",
        &body,
        "",
        "",
    ))
}

fn image_response(image: Option<&ProfileAvatar>) -> Response {
    match image {
        Some(asset) => Response::text(200, asset.content_type, asset.bytes.clone()),
        None => Response::text(
            404,
            "text/plain; charset=utf-8",
            "Community image unavailable.",
        ),
    }
}

fn form_error(state: &State, error: &str, back: &str) -> Response {
    Response::html(layout(state, "", "Check your submission", &format!(
        "<section class='panel standalone'><h1>Check your submission</h1><p>{}</p><a class='button dark' href='{}'>Go back</a></section>", escape(error), escape(back)
    ), "", ""))
}

fn profile_name(state: &State) -> &str {
    if state.display_name.is_empty() {
        &state.username
    } else {
        &state.display_name
    }
}

fn profile_url(state: &State) -> String {
    format!("/u/{}", encode(&state.username))
}

fn nsfw_badge() -> &'static str {
    "<span class='content-tag nsfw-tag'><b>18+</b> NSFW</span>"
}

fn is_sensitive(state: &State, post: &Post) -> bool {
    let marked = |item: &Post| {
        item.sensitive || community(state, &item.community).is_some_and(|group| group.mature)
    };
    marked(post)
        || post
            .repost_of
            .and_then(|source| state.repost_source(source))
            .is_some_and(marked)
}

fn display_post_title<'a>(state: &'a State, post: &'a Post) -> &'a str {
    if post.repost_title_from_source {
        post.repost_of
            .and_then(|id| state.repost_source(id))
            .map_or(&post.title, |source| source.title.as_str())
    } else {
        &post.title
    }
}

fn shared_feed(state: &State, feed: &CustomFeed) -> String {
    let mut html = masthead(&feed.name, &feed.description, "SHARED FEED");
    let mut posts: Vec<_> = state
        .posts
        .iter()
        .filter(|post| {
            feed.communities.contains(&post.community)
                && community(state, &post.community)
                    .is_some_and(|item| item.visibility != "private")
                && !(is_sensitive(state, post) && state.sensitive == "hide")
        })
        .collect();
    posts.sort_by_key(|post| std::cmp::Reverse(post.id));
    if posts.is_empty() {
        html.push_str("<section class='panel empty'><h2>No public discussions in this feed yet.</h2></section>");
    }
    for post in posts {
        let name = community(state, &post.community)
            .map(|item| item.name.as_str())
            .unwrap_or(&post.community);
        write!(html, "<article class='panel profile-item'><a class='subtle-link' href='{}'>{}</a><h2><a href='/thread?id={}'>{}</a></h2></article>", community_url(&post.community), escape(name), post.id, escape(&post.title)).unwrap();
    }
    html
}

fn field<'a>(fields: &'a BTreeMap<String, String>, name: &str) -> &'a str {
    fields.get(name).map(String::as_str).unwrap_or_default()
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
    out
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            write!(out, "%{byte:02X}").unwrap();
        }
    }
    out
}

fn icon(name: &str) -> String {
    let shape = match name {
        "mark" => "<path d='M3 6h18M3 12h14l-4 6M7 18h14'/> <circle cx='3' cy='18' r='.9' fill='currentColor' stroke='none'/>",
        "home" => "<path d='M3 10 12 3l9 7v10H3zM9 20v-6h6v6'/>",
        "grid" => "<rect x='4' y='4' width='6' height='6' rx='1'/><rect x='14' y='4' width='6' height='6' rx='1'/><rect x='4' y='14' width='6' height='6' rx='1'/><rect x='14' y='14' width='6' height='6' rx='1'/>",
        "save" => "<path d='M6 4h12v16l-6-4-6 4z'/>",
        "search" => "<circle cx='10.5' cy='10.5' r='6'/><path d='m15 15 5 5'/>",
        "plus" => "<path d='M12 5v14M5 12h14'/>",
        "sliders" => "<path d='M4 7h7m4 0h5M4 17h3m4 0h9'/><circle cx='13' cy='7' r='2'/><circle cx='9' cy='17' r='2'/>",
        "up" => "<path d='m6 13 6-6 6 6M12 7v12'/>",
        "down" => "<path d='m6 11 6 6 6-6M12 17V5'/>",
        "chat" => "<path d='M20 11.3a7.5 7.5 0 0 1-8 7.2H5l1-3.4a7 7 0 0 1-1.5-4A7.5 7.5 0 0 1 12 4a7.5 7.5 0 0 1 8 7.3Z'/>",
        "more" => "<path d='M5 12h.01M12 12h.01M19 12h.01' stroke-width='3'/>",
        "shield" => "<path d='M12 3 4 6v5c0 5 3 8 8 10 5-2 8-5 8-10V6zM9 12l2 2 4-4'/>",
        "spark" => "<path d='M12 2v20M2 12h20M5 5l14 14m0-14L5 19'/>",
        "log" => "<rect x='5' y='3' width='14' height='18' rx='2'/><path d='M9 8h6m-6 4h6m-6 4h4'/>",
        "news" => "<rect x='3' y='5' width='18' height='14' rx='2'/><path d='M7 9h4v4H7zm7 0h3m-3 4h3m-10 3h10'/>",
        "trend" => "<path d='m3 17 6-6 4 3 7-8m-5 0h5v5'/>",
        "mail" => "<rect x='3' y='5' width='18' height='14' rx='2'/><path d='m4 7 8 6 8-6'/>",
        "bell" => "<path d='M12 4a5 5 0 0 0-5 5c0 5-2 6-2 7h14c0-1-2-2-2-7a5 5 0 0 0-5-5Zm-2 15a2 2 0 0 0 4 0'/>",
        "calendar" => "<rect x='3.5' y='5.5' width='17' height='15' rx='2'/><path d='M3.5 10h17M8 3.5v4m8-4v4m-8 7h3m3 0h3'/>",
        "image" => "<rect x='3' y='4' width='18' height='16' rx='2'/><circle cx='8' cy='9' r='1.4'/><path d='m4 18 5-5 4 4 3-3 4 4'/>",
        "tag" => "<path d='M3 12 12 3h7l2 2v7l-9 9-9-9Z'/><circle cx='16.5' cy='7.5' r='1'/>",
        "chevron" => "<path d='m6 9 6 6 6-6'/>",
        "pin" => "<path d='m8 4 7 1-1 5 3 4H6l3-4zm3 10v6'/>",
        "check" => "<path d='m5 12 4 4 10-10'/>",
        _ => "<circle cx='12' cy='12' r='7'/>",
    };
    format!("<svg viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='1.7' stroke-linecap='round' stroke-linejoin='round' aria-hidden='true'>{shape}</svg>")
}

fn community<'a>(state: &'a State, slug: &str) -> Option<&'a Community> {
    state.communities.iter().find(|item| item.slug == slug)
}

fn can_write_here(state: &State, slug: &str) -> bool {
    !state.username.is_empty()
        && community(state, slug).is_some_and(|item| state.can_contribute(item))
}

fn can_edit_post(state: &State, post: &Post) -> bool {
    post.author == state.username && !state.is_banned(&post.community, &state.username)
}

fn community_url(slug: &str) -> String {
    format!("/?view={}", encode(slug))
}

fn community_image_url(slug: &str, kind: &str) -> String {
    format!("/communities/image?community={}&kind={kind}", encode(slug))
}

fn community_preview_image_url(id: u64, kind: &str) -> String {
    format!("/communities/preview-image?id={id}&kind={kind}")
}

fn custom_url(id: u64) -> String {
    format!("/?view=custom%3A{id}")
}

fn custom_feed<'a>(state: &'a State, active: &str) -> Option<&'a CustomFeed> {
    let id: u64 = active.strip_prefix("custom:")?.parse().ok()?;
    state.custom_feeds.iter().find(|item| item.id == id)
}

fn community_owner(state: &State, item: &Community) -> bool {
    item.owner.as_deref() == Some(state.username.as_str())
}

fn community_members(state: &State, item: &Community) -> String {
    let total = state.community_member_count(item);
    if total == 1 {
        "1 member".into()
    } else {
        format!("{} members", metric(total))
    }
}

fn find_post<'a>(state: &'a State, query: &BTreeMap<String, String>) -> Option<&'a Post> {
    let id = field(query, "id").parse().ok()?;
    state.posts.iter().find(|post| {
        post.id == id
            && community(state, &post.community).is_some_and(|item| state.can_view_community(item))
    })
}

fn community_icon(item: &Community) -> String {
    if item.icon_image.is_some() {
        return format!("<span class='community-icon has-image' aria-hidden='true'><img src='{}' alt=''></span>", escape(&community_image_url(&item.slug, "icon")));
    }
    format!(
        "<span class='community-icon {}' aria-hidden='true'>{}</span>",
        escape(&item.color),
        escape(&item.icon)
    )
}

fn community_art(icon_url: Option<&str>, banner_url: Option<&str>, details: &str) -> String {
    let mark = icon_url.map_or_else(
        || format!("<span class='community-art-icon community-art-icon-default' aria-hidden='true'>{}</span>", icon("grid")),
        |url| format!("<img class='community-art-icon' src='{}' width='60' height='60' alt=''>", escape(url)),
    );
    let banner = banner_url.map_or_else(
        || "<div class='community-art-banner community-art-banner-default' aria-hidden='true'></div>".into(),
        |url| format!("<img class='community-art-banner' src='{}' alt=''>", escape(url)),
    );
    format!("<div class='panel community-art'>{banner}<div class='community-art-identity'>{mark}<div class='community-art-copy'>{details}</div></div></div>")
}

fn initials(handle: &str) -> String {
    handle
        .chars()
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

fn nav_link(url: &str, label: &str, glyph: &str, selected: bool) -> String {
    let selection = if selected {
        " class='selected' aria-current='page'"
    } else {
        ""
    };
    format!(
        "<a href='{url}'{selection}>{}<span>{label}</span></a>",
        icon(glyph)
    )
}

fn sidebar_group(label: &str, links: &str, open: bool) -> String {
    let open = if open { " open" } else { "" };
    format!("<details class='sidebar-group'{open}><summary>{label}{}</summary><div class='nav-group'>{links}</div></details>", icon("chevron"))
}

fn layout(
    state: &State,
    active: &str,
    title: &str,
    body: &str,
    query: &str,
    notice: &str,
) -> String {
    layout_with_rail(state, active, title, body, query, notice, None)
}

fn layout_with_rail(
    state: &State,
    active: &str,
    title: &str,
    body: &str,
    query: &str,
    notice: &str,
    right: Option<String>,
) -> String {
    let mut communities = String::new();
    for item in state
        .communities
        .iter()
        .filter(|item| state.joined.contains(&item.slug) && state.can_view_community(item))
    {
        let selection = if item.slug == active {
            " class='selected' aria-current='page'"
        } else {
            ""
        };
        write!(
            communities,
            "<a href='/?view={}'{selection}>{}<span>{}</span></a>",
            encode(&item.slug),
            community_icon(item),
            escape(&item.name)
        )
        .unwrap();
    }
    let signed_in = !state.username.is_empty();
    let main_nav = [
        ("/", "Home", "home", "home"),
        ("/?view=popular", "Popular", "trend", "popular"),
        ("/?view=news", "News", "news", "news"),
        ("/?view=communities", "Explore", "grid", "communities"),
        (
            "/communities/start",
            "Start a community",
            "plus",
            "start-community",
        ),
    ]
    .into_iter()
    .filter(|(_, _, _, key)| signed_in || *key != "start-community")
    .map(|(url, label, glyph, key)| {
        nav_link(
            url,
            label,
            glyph,
            active == key || key == "home" && active == "following",
        )
    })
    .collect::<String>();
    let utility_nav = [
        ("/?view=all", "All discussions", "grid", "all"),
        ("/?view=saved", "Saved", "save", "saved"),
        ("/settings", "Feed controls", "sliders", "settings"),
        ("/policy", "Moderation policy", "shield", "policy"),
    ]
    .into_iter()
    .map(|(url, label, glyph, key)| nav_link(url, label, glyph, active == key))
    .collect::<String>();
    let creator_links = [
        ("dashboard", "Dashboard", "grid"),
        ("performance", "Performance", "trend"),
        ("trends", "Trends", "chat"),
        ("content", "Content", "log"),
    ]
    .into_iter()
    .map(|(section, label, glyph)| {
        nav_link(
            &format!("/creator?section={section}"),
            label,
            glyph,
            active == section,
        )
    })
    .collect::<String>();
    let moderation_links = [
        ("workspace", "Moderator workspace", "shield"),
        ("queue", "Mod queue", "shield"),
        ("mail", "Mod mail", "mail"),
        ("mod-feed", "Mod feed", "grid"),
        ("manage", "Manage communities", "sliders"),
    ]
    .into_iter()
    .map(|(section, label, glyph)| {
        let url = if section == "workspace" {
            "/moderation/tools".into()
        } else {
            format!("/moderation?section={section}")
        };
        nav_link(&url, label, glyph, active == section)
    })
    .collect::<String>();
    let mut feeds = nav_link(
        "/feeds/new",
        "Create custom feed",
        "plus",
        active == "feeds",
    );
    for item in &state.custom_feeds {
        feeds.push_str(&nav_link(
            &custom_url(item.id),
            &escape(&item.name),
            "grid",
            active == format!("custom:{}", item.id),
        ));
    }
    let recent = state
        .recent
        .iter()
        .filter_map(|slug| community(state, slug))
        .filter(|item| state.can_view_community(item))
        .map(|item| {
            nav_link(
                &community_url(&item.slug),
                &escape(&item.name),
                "chat",
                false,
            )
        })
        .collect::<String>();
    let recent = if recent.is_empty() {
        "<p class='nav-empty'>No recent communities</p>"
    } else {
        &recent
    };
    let groups = sidebar_group(
        "Creator tools",
        &creator_links,
        matches!(active, "dashboard" | "performance" | "trends" | "content"),
    ) + &sidebar_group(
        "Moderation",
        &moderation_links,
        matches!(active, "queue" | "mail" | "mod-feed" | "manage"),
    ) + &sidebar_group(
        "Custom feeds",
        &feeds,
        active == "feeds" || active.starts_with("custom:"),
    ) + &sidebar_group("Recent", recent, false);
    let feedback = if notice.is_empty() {
        String::new()
    } else {
        format!(
            "<div class='notice' role='status'>{} {}</div>",
            icon("check"),
            escape(notice)
        )
    };
    let compose = if can_write_here(state, active) {
        format!("/new?community={}", encode(active))
    } else {
        "/new".into()
    };
    let search_community = community(state, active);
    let custom = custom_feed(state, active);
    let search_view = if search_community.is_some() || custom.is_some() {
        active
    } else {
        "all"
    };
    let search_label = search_community
        .map(|item| item.name.as_str())
        .or_else(|| custom.map(|item| item.name.as_str()))
        .map(|name| format!("Search {name}"))
        .unwrap_or_else(|| "Search discussions".into());
    let avatar = if state.avatar.is_some() {
        "<img src='/profile/avatar' width='36' height='36' alt=''>".into()
    } else {
        escape(&initials(profile_name(state)))
    };
    let public_preview = matches!(active, "public-profile" | "public-feed");
    let rail_class = if matches!(active, "profile" | "public-profile") {
        " profile-rail"
    } else {
        ""
    };
    let shell_class = if active == "mod-workspace" {
        " mod-shell"
    } else {
        ""
    };
    let owner_nav = if public_preview || !signed_in {
        String::new()
    } else {
        format!(
            r#"<div class="nav-group community-nav"><h2>Your communities</h2>{communities}</div><div class="sidebar-sections">{groups}</div><div class="nav-group utility-nav">{utility_nav}</div>"#
        )
    };
    let account_actions = if !signed_in {
        String::from("<a class='button light' href='/account/login'>Sign in</a><a class='button dark' href='/account/register'>Create account</a>")
    } else {
        let unread_messages = state
            .messages
            .iter()
            .filter(|item| item.to == state.username && !item.read)
            .count();
        let unread_notifications = state
            .notifications
            .iter()
            .filter(|item| item.recipient == state.username && !item.read)
            .count();
        let messages = account_shortcut(
            "/messages",
            "Messages",
            "mail",
            unread_messages,
            active == "messages",
        );
        let notifications = account_shortcut(
            "/notifications",
            "Notifications",
            "bell",
            unread_notifications,
            active == "notifications",
        );
        format!(
            r#"<a class="button light settings-top" href="/settings">{}<span>Feed controls</span></a><a class="button dark account-compose" href="{compose}" aria-label="New post">{}<span>New post</span></a>{messages}{notifications}<a class="avatar avatar-me" href="/profile" aria-label="Your profile">{avatar}</a>"#,
            icon("sliders"),
            icon("plus")
        )
    };
    format!(
        r##"<!doctype html><html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="theme-color" content="#f6f6f0"><title>{title} · Commonline</title><link rel="stylesheet" href="/style.css"></head>
<body><a class="skip" href="#content">Skip to content</a>
<header class="topbar"><div class="top-inner">
<a class="wordmark" href="/" aria-label="Commonline home"><span class="brand-mark">{brand}</span>commonline<span class="wordmark-period">.</span></a>
<form class="search" action="/" role="search"><input type="hidden" name="view" value="{search_view}"><input type="search" name="q" aria-label="{search_label}" placeholder="{search_label}" value="{query}" maxlength="120"><button type="submit" aria-label="Run search">{search_icon}</button></form>
<div class="header-actions">{account_actions}</div>
</div></header>
<div class="page-shell{shell_class}">
<aside class="left-rail"><nav aria-label="Main navigation"><div class="nav-group">{main_nav}</div>{owner_nav}</nav><div class="left-foot"><span></span> Interactive prototype</div></aside>
<main id="content" tabindex="-1">{feedback}{body}</main>
<aside class="right-rail{rail_class}" aria-label="Additional information">{right}</aside>
</div></body></html>"##,
        title = escape(title),
        brand = icon("mark"),
        search_icon = icon("search"),
        search_view = escape(search_view),
        search_label = escape(&search_label),
        query = escape(query),
        right = right.unwrap_or_else(|| right_rail(state)),
    )
}

fn account_shortcut(url: &str, label: &str, glyph: &str, unread: usize, selected: bool) -> String {
    let current = if selected { " aria-current='page'" } else { "" };
    let accessible = if unread == 0 {
        label.to_owned()
    } else {
        format!("{label}, {unread} unread")
    };
    let badge = if unread == 0 {
        String::new()
    } else {
        let count = if unread > 99 {
            "99+".to_owned()
        } else {
            unread.to_string()
        };
        format!("<span class='account-shortcut-count' aria-hidden='true'>{count}</span>")
    };
    format!("<a class='account-shortcut' href='{url}' aria-label='{accessible}' title='{accessible}'{current}>{}{badge}</a>", icon(glyph))
}

fn right_rail(state: &State) -> String {
    let mut items = String::new();
    let mut popular: Vec<&Community> = state
        .communities
        .iter()
        .filter(|item| item.visibility == "public")
        .collect();
    popular.sort_by_key(|item| std::cmp::Reverse(state.community_member_count(item)));
    for (index, item) in popular.into_iter().take(4).enumerate() {
        write!(items, "<li><span class='rank'>{:02}</span><a href='/?view={}'>{}<span><strong>{}</strong><small>{}</small></span></a></li>", index+1, encode(&item.slug), community_icon(item), escape(&item.name), community_members(state, item)).unwrap();
    }
    format!(
        r#"<section class="right-policy" aria-labelledby="policy-title"><div class="policy-label">{}<span>THE COMMONLINE POLICY</span></div><h2 id="policy-title">Lawful speech<br>stays up.</h2><p>You choose what shows up in your feed.</p><a class="button policy-button" href="/policy">Read the policy</a></section>
<section class="panel rising"><h2>Popular communities</h2><ol>{items}</ol><a class="subtle-link" href="/?view=communities">Explore communities</a></section>
<section class="panel review-card">{}<div><h2>Legal review log</h2><p>Track a report and see its status.</p><a class="subtle-link" href="/reports">View review log</a></div></section>"#,
        icon("shield"),
        icon("log")
    )
}

fn metric(count: u32) -> String {
    if count >= 1_000 {
        format!("{:.1}k", f64::from(count) / 1000.0)
    } else {
        count.to_string()
    }
}

fn masthead(title: &str, description: &str, label: &str) -> String {
    format!("<section class='masthead' aria-labelledby='page-title'><div class='mast-copy'><div class='overline'><span></span>{}</div><h1 id='page-title'>{}</h1><p>{}</p></div><div class='mast-art' aria-hidden='true'></div></section>", escape(label), escape(title), escape(description))
}

fn stream_url(active: &str, sort: &str, search: &str) -> String {
    format!(
        "/?view={}&sort={}&q={}",
        encode(active),
        encode(sort),
        encode(search)
    )
}

fn feed(state: &State, query: &BTreeMap<String, String>, active: &str) -> String {
    let selected = community(state, active);
    let custom = custom_feed(state, active);
    let search = field(query, "q").trim();
    let sort = match field(query, "sort") {
        "best" => "best",
        "hot" => "hot",
        "new" => "new",
        "top" => "top",
        _ if active == "popular" => "top",
        _ if active == "news" => "new",
        _ => "best",
    };
    let back = stream_url(active, sort, search);
    let mut html = match (active, selected, search.is_empty()) {
        (_, _, false) => masthead("Search", &format!("Results for “{search}”"), "DISCUSSIONS"),
        ("saved", _, true) => masthead(
            "Saved posts.",
            "Discussions you want to come back to.",
            "YOUR COLLECTION",
        ),
        ("all", _, true) => masthead(
            "All discussions.",
            "Browse conversations from across Commonline.",
            "DISCOVER",
        ),
        ("popular", _, true) => masthead(
            "Popular.",
            "Browse discussions across Commonline, ranked by votes.",
            "DISCOVER",
        ),
        ("news", _, true) => masthead(
            "News.",
            "News posts shared by Commonline communities.",
            "NEWS FEED",
        ),
        ("communities", _, true) => {
            return community_directory(
                state,
                field(query, "community_query"),
                field(query, "topic"),
            )
        }
        (_, Some(item), true) => community_masthead(item),
        (_, None, true) if custom.is_some() => {
            let item = custom.unwrap();
            masthead(
                &item.name,
                &item.description,
                if item.private {
                    "PRIVATE CUSTOM FEED"
                } else {
                    "CUSTOM FEED"
                },
            )
        }
        _ => masthead(
            "The front page.",
            "Independent communities. Open discussion.",
            "THE COMMONLINE FEED",
        ),
    };
    if matches!(active, "home" | "following") && search.is_empty() {
        let tabs = [("home", "For You"), ("following", "Following")]
            .into_iter()
            .map(|(key, label)| {
                let current = if key == active {
                    " aria-current='page'"
                } else {
                    ""
                };
                format!(
                    "<a href='{}'{current}>{label}</a>",
                    escape(&stream_url(key, sort, ""))
                )
            })
            .collect::<String>();
        write!(
            html,
            "<nav class='home-tabs' aria-label='Home feeds'>{tabs}</nav>"
        )
        .unwrap();
    }
    if let Some(item) = custom {
        let share = if item.private {
            String::new()
        } else {
            format!(
                "<a class='button light' href='/feeds/shared?id={}'>Open shared view</a>",
                item.id
            )
        };
        let count = item.communities.len();
        write!(html, "<div class='community-bar'><span>{count} {}</span>{share}<a class='button light' href='/feeds/settings?id={}'>{} Custom feed settings</a></div>", if count == 1 { "community" } else { "communities" }, item.id, icon("sliders")).unwrap();
    }
    if let Some(item) = selected.filter(|_| search.is_empty()) {
        let joined = state.joined.contains(&item.slug);
        let label = if joined {
            "Leave community"
        } else {
            "Join community"
        };
        let mute = if state.muted.contains(&item.slug) {
            "Unmute"
        } else {
            "Mute"
        };
        write!(
            html,
            "<div class='community-bar'><span>{}</span>{}{}",
            community_members(state, item),
            action_form(
                "join",
                &back,
                &[("community", &item.slug)],
                label,
                "plus",
                "button light"
            ),
            action_form(
                "mute",
                &back,
                &[("community", &item.slug)],
                mute,
                "",
                "button light"
            )
        )
        .unwrap();
        if community_owner(state, item) {
            write!(html, "<a class='button light' href='/communities/settings?community={}'>{} Community settings</a>", encode(&item.slug), icon("sliders")).unwrap();
            write!(
                html,
                "<a class='button light' href='/moderation/tools?community={}'>{} Mod tools</a>",
                encode(&item.slug),
                icon("shield")
            )
            .unwrap();
        }
        if item.owner.is_some() && !state.is_mod_mail_muted(&item.slug, &state.username) {
            write!(html, "<a class='button light' href='/moderation/contact?community={}'>{} Message moderators</a>", encode(&item.slug), icon("mail")).unwrap();
        }
        html.push_str("</div>");
        if state.is_banned(&item.slug, &state.username) {
            html.push_str("<p class='community-restriction-note muted'>Your account is banned from posting or replying in this community.</p>");
        }
        if !item.guidelines.is_empty() {
            write!(html, "<details class='panel community-guidelines'><summary>Topic guidelines</summary><p>{}</p></details>", escape(&item.guidelines)).unwrap();
        }
    }
    if let Some(item) = selected.filter(|_| !search.is_empty()) {
        write!(html, "<div class='community-bar'><span>Searching {}</span><a class='button light' href='{}'>Search all discussions</a></div>", escape(&item.name), escape(&stream_url("all", sort, search))).unwrap();
    }
    let sort_options = [
        ("best", "Best"),
        ("hot", "Hot"),
        ("new", "New"),
        ("top", "Top"),
    ];
    let sort_label = sort_options.iter().find(|(key, _)| *key == sort).unwrap().1;
    let sort_links = sort_options
        .into_iter()
        .map(|(key, label)| {
            let selection = if key == sort {
                " class='on' aria-current='page'"
            } else {
                ""
            };
            format!(
                "<a href='{}'{selection}>{label}</a>",
                escape(&stream_url(active, key, search))
            )
        })
        .collect::<String>();
    let needle = search.to_lowercase();
    let mut posts: Vec<&Post> = state
        .posts
        .iter()
        .filter(|post| {
            let group = community(state, &post.community);
            let viewable = group.is_some_and(|item| {
                state.can_view_community(item)
                    && (item.visibility != "private"
                        || selected.is_some()
                        || custom.is_some()
                        || matches!(active, "following" | "saved"))
            });
            let in_view = match active {
                "saved" => state.saved.contains(&post.id),
                "following" => state.joined.contains(&post.community),
                "news" => post.flair.eq_ignore_ascii_case("news"),
                _ => {
                    selected.is_none_or(|item| item.slug == post.community)
                        && custom.is_none_or(|item| item.communities.contains(&post.community))
                }
            };
            let matches = search.is_empty()
                || [
                    display_post_title(state, post),
                    post.body.as_str(),
                    post.author.as_str(),
                    post.community.as_str(),
                    post.flair.as_str(),
                ]
                .iter()
                .any(|text| text.to_lowercase().contains(&needle))
                || group.is_some_and(|item| item.name.to_lowercase().contains(&needle));
            let visible = !state.hidden.contains(&post.id)
                && !state.blocked.contains(&post.author)
                && (selected.is_some() || !state.muted.contains(&post.community))
                && !(is_sensitive(state, post) && state.sensitive == "hide")
                && !state.keywords.iter().any(|keyword| {
                    [display_post_title(state, post), post.body.as_str()]
                        .iter()
                        .any(|text| text.to_lowercase().contains(&keyword.to_lowercase()))
                });
            viewable && in_view && matches && visible
        })
        .collect();
    posts.sort_by_key(|post| {
        let votes = i64::from(score(state, post));
        let replies = i64::from(post.reply_count.min(100));
        let rank = match sort {
            "new" => 0,
            "top" => votes,
            "hot" => 24 * (votes + replies) / (4 + age_hours(&post.age)),
            _ => {
                votes
                    + 2 * replies
                    + i64::from(active == "home" && state.joined.contains(&post.community)) * 125
            }
        };
        (std::cmp::Reverse(rank), std::cmp::Reverse(post.id))
    });
    let pinned = if search.is_empty() {
        selected.and_then(|item| item.pinned_post)
    } else {
        None
    };
    if let Some(index) = posts.iter().position(|post| Some(post.id) == pinned) {
        posts[..=index].rotate_right(1);
    }
    write!(html, "<div class='feed-tools'><details class='sort-picker'><summary>Sort by <strong>{sort_label}</strong>{}</summary><nav class='sort' aria-label='Sort discussions'>{sort_links}</nav></details><span>{} discussion{}</span></div>", icon("chevron"), posts.len(), if posts.len() == 1 { "" } else { "s" }).unwrap();
    if posts.is_empty()
        && search.is_empty()
        && selected.is_some_and(|item| !state.posts.iter().any(|post| post.community == item.slug))
    {
        if can_write_here(state, active) {
            write!(html, "<section class='panel empty'><h2>Start the discussion.</h2><p>Be the first to post in this community.</p><a class='button dark' href='/new?community={}'>{} New post</a></section>", encode(active), icon("plus")).unwrap();
        } else {
            html.push_str("<section class='panel empty'><h2>No discussions yet.</h2></section>");
        }
    } else if posts.is_empty() && active == "following" {
        html.push_str("<section class='panel empty'><h2>No posts from your communities yet.</h2><a class='button light' href='/?view=communities'>Explore communities</a></section>");
    } else if posts.is_empty() {
        html.push_str("<section class='panel empty'><h2>No discussions here yet.</h2><p>Try a different search or check your feed controls.</p><a class='button light' href='/settings'>Feed controls</a></section>");
    }
    for post in posts {
        html.push_str(&post_card(
            state,
            post,
            &back,
            false,
            Some(post.id) == pinned,
        ));
    }
    html
}

fn age_hours(age: &str) -> i64 {
    if let Some(hours) = age.strip_suffix('h') {
        hours.parse::<i64>().unwrap_or(0).clamp(0, 8_760)
    } else if let Some(days) = age.strip_suffix('d') {
        days.parse::<i64>().unwrap_or(0).clamp(0, 365) * 24
    } else {
        0
    }
}

fn community_masthead(item: &Community) -> String {
    if item.owner.is_some() || item.icon_image.is_some() || item.banner_image.is_some() {
        let icon = item
            .icon_image
            .as_ref()
            .map(|_| community_image_url(&item.slug, "icon"));
        let banner = item
            .banner_image
            .as_ref()
            .map(|_| community_image_url(&item.slug, "banner"));
        return community_art(
            icon.as_deref(),
            banner.as_deref(),
            &community_art_details(item, true),
        );
    }
    format!("<section class='masthead' aria-labelledby='page-title'><div class='mast-copy'><div class='community-heading'>{}<span>c/{}</span></div><h1 id='page-title'>{}</h1><p>{}</p>{}</div><div class='mast-art' aria-hidden='true'></div></section>", community_icon(item), escape(&item.slug), escape(&item.name), escape(&item.description), community_badges(item))
}

fn community_art_details(item: &Community, published: bool) -> String {
    let owner = item
        .owner
        .as_ref()
        .map(|handle| format!(" <span>·</span> Started by u/{}", escape(handle)))
        .unwrap_or_default();
    let (heading, id) = if published {
        ("h1", " id='page-title'")
    } else {
        ("h2", "")
    };
    format!("<div class='community-art-address'>c/{}{owner}</div><{heading}{id} class='community-art-name'>{}</{heading}><p>{}</p>{}", escape(&item.slug), escape(&item.name), escape(&item.description), community_badges(item))
}

fn community_badges(item: &Community) -> String {
    let mode = match item.visibility.as_str() {
        "private" => "Private",
        "restricted" => "Restricted",
        _ => "Public",
    };
    let mature = if item.mature { nsfw_badge() } else { "" };
    let topic = escape(community_setup::topic_label(&item.topic));
    format!("<div class='community-badges'><span class='content-tag'>{topic}</span><span class='content-tag'>{mode}</span>{mature}</div>")
}

fn community_directory(state: &State, query: &str, topic: &str) -> String {
    let mut html = masthead(
        "Communities.",
        "Find people talking about the things you care about.",
        "DISCOVER",
    );
    let topics = topic_options(topic);
    write!(html, "<div class='directory-tools'><form class='search directory-filter' method='get' action='/' role='search'><input type='hidden' name='view' value='communities'><input type='search' name='community_query' aria-label='Find communities' placeholder='Find communities' maxlength='80' value='{}'><select name='topic' aria-label='Community topic'><option value=''>All topics</option>{topics}</select><button type='submit' aria-label='Search the directory'>{}</button></form><a class='button dark' href='/communities/start'>{} Create community</a></div>", escape(query), icon("search"), icon("plus")).unwrap();
    let needle = query.trim().to_lowercase();
    let mut items: Vec<_> = state
        .communities
        .iter()
        .filter(|item| {
            item.visibility == "public"
                && (topic.is_empty()
                    || (if item.topic.is_empty() {
                        "general"
                    } else {
                        &item.topic
                    }) == topic)
                && (item.name.to_lowercase().contains(&needle)
                    || item.slug.contains(&needle)
                    || item.description.to_lowercase().contains(&needle))
        })
        .collect();
    items.sort_by_cached_key(|item| item.name.to_lowercase());
    if items.is_empty() {
        html.push_str("<section class='panel empty'><h2>No matching communities.</h2><a class='button light' href='/?view=communities'>Clear search</a></section>");
        return html;
    }
    html.push_str("<div class='directory'>");
    for item in items {
        let joined = if state.joined.contains(&item.slug) {
            " · Joined"
        } else {
            ""
        };
        write!(html, "<article class='panel directory-item'><a href='{}'>{}<h2>{}</h2></a><p>{}</p><small>{}{joined}</small></article>", community_url(&item.slug), community_icon(item), escape(&item.name), escape(&item.description), community_members(state, item)).unwrap();
    }
    html.push_str("</div>");
    html
}

fn topic_options(selected: &str) -> String {
    community_setup::TOPICS
        .iter()
        .map(|(code, label)| {
            format!(
                "<option value='{code}'{}>{}</option>",
                if *code == selected { " selected" } else { "" },
                escape(label)
            )
        })
        .collect()
}

fn community_editor(state: &State, existing: Option<&Community>) -> String {
    let (heading, action, back, cancel, field_markup, name, description, guidelines) = match existing {
        Some(item) => (
            "Community settings", "edit-community", format!("/communities/settings?community={}", encode(&item.slug)), community_url(&item.slug),
            format!("{}<p class='muted'>Community address <strong>c/{}</strong></p>", hidden("community", &item.slug), escape(&item.slug)),
            item.name.as_str(), item.description.as_str(), item.guidelines.as_str(),
        ),
        None => (
            "Create community", "create-community", "/communities/new".into(), "/?view=communities".into(),
            r"<label>Community address<input name='slug' required minlength='2' maxlength='32' pattern='[A-Za-z0-9][A-Za-z0-9\-]{0,30}[A-Za-z0-9]' placeholder='city-gardening' aria-describedby='slug-help'><small id='slug-help'>Use 2–32 letters, numbers, or hyphens. The address stays the same after creation.</small></label>".to_owned(),
            "", "", "",
        ),
    };
    let submit = if existing.is_some() {
        "Save community settings"
    } else {
        "Create community"
    };
    let topic = existing
        .map(|item| item.topic.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("general");
    let visibility = existing
        .map(|item| item.visibility.as_str())
        .unwrap_or("public");
    let modes = [
        ("public", "Public"),
        ("restricted", "Restricted"),
        ("private", "Private"),
    ]
    .into_iter()
    .map(|(code, label)| {
        format!(
            "<option value='{code}'{}>{label}</option>",
            if code == visibility { " selected" } else { "" }
        )
    })
    .collect::<String>();
    let mature = if existing.is_some_and(|item| item.mature) {
        " checked"
    } else {
        ""
    };
    let jump = if existing.is_some() {
        "<a class='button light' href='#community-appearance'>Appearance</a>"
    } else {
        ""
    };
    let mut html = format!(
        r#"<section class="panel standalone"><header class="creator-header"><h1>{heading}</h1>{jump}</header>
<form class="stack-form" method="post" action="/action">{fields}{field_markup}
<label>Community name<input name="name" required maxlength="60" value="{name}" placeholder="City Gardening"></label>
<label>Description <small>Optional</small><textarea name="description" rows="3" maxlength="500" placeholder="What do people discuss here?">{description}</textarea></label>
<label>Topic<select name="topic">{topics}</select></label><label>Community access<select name="visibility">{modes}</select></label><label class="checkbox"><input type="checkbox" name="mature"{mature}> Mature (18+)</label>
<label>Topic guidelines <small>Optional</small><textarea name="guidelines" rows="5" maxlength="2000" placeholder="Describe the kinds of conversations that fit the community.">{guidelines}</textarea></label>
<div class="form-actions"><a class="button light" href="{cancel}">Back to community browsing</a><button class="button dark" type="submit">{submit}</button></div></form>"#,
        fields = action_fields(action, &back, &[]),
        name = escape(name),
        description = escape(description),
        guidelines = escape(guidelines),
        topics = topic_options(topic),
    );
    if let Some(item) = existing {
        let mut options = String::from("<option value=''>No pinned discussion</option>");
        for post in state
            .posts
            .iter()
            .rev()
            .filter(|post| post.community == item.slug)
        {
            write!(
                options,
                "<option value='{}'{}>{}</option>",
                post.id,
                if item.pinned_post == Some(post.id) {
                    " selected"
                } else {
                    ""
                },
                escape(&post.title)
            )
            .unwrap();
        }
        write!(html, "<section class='preference-list'><h2>Pinned discussion</h2><p class='muted'>Choose a discussion to keep at the top of the community feed.</p><form class='stack-form community-pin-form' method='post' action='/action'>{}<label>Discussion<select name='post'>{options}</select></label><button class='button dark' type='submit'>Save pin</button></form></section>", action_fields("pin-community", &back, &[("community", &item.slug)])).unwrap();
    }
    html.push_str("</section>");
    html
}

fn hidden(name: &str, value: &str) -> String {
    format!(
        "<input type='hidden' name='{}' value='{}'>",
        escape(name),
        escape(value)
    )
}

fn action_fields(action: &str, back: &str, fields: &[(&str, &str)]) -> String {
    hidden("action", action)
        + &hidden("back", back)
        + &fields
            .iter()
            .map(|(name, value)| hidden(name, value))
            .collect::<String>()
}

fn action_form(
    action: &str,
    back: &str,
    fields: &[(&str, &str)],
    label: &str,
    glyph: &str,
    class: &str,
) -> String {
    let graphic = if glyph.is_empty() {
        String::new()
    } else {
        icon(glyph)
    };
    format!("<form method='post' action='/action' class='inline'>{}<button class='{class}' type='submit'>{graphic}<span>{}</span></button></form>", action_fields(action, back, fields), escape(label))
}

fn score(state: &State, post: &Post) -> i32 {
    state.post_score(post)
}

fn votes(action: &str, id: u64, score: i32, vote: i8, back: &str) -> String {
    let id = id.to_string();
    let button = |direction, label, glyph| {
        format!(
        "<form method='post' action='/action'>{}<button type='submit' aria-label='{label}' aria-pressed='{}'>{}</button></form>",
        action_fields(action, back, &[("id", &id), ("direction", if direction == 1 { "1" } else { "-1" })]), vote == direction, icon(glyph)
    )
    };
    format!(
        "<div class='votes'>{}<strong>{score}</strong>{}</div>",
        button(1, "Upvote", "up"),
        button(-1, "Downvote", "down")
    )
}

fn post_card(state: &State, post: &Post, back: &str, full: bool, pinned: bool) -> String {
    let id = post.id.to_string();
    let url = format!("/thread?id={id}");
    let item = community(state, &post.community);
    let name = item.map(|c| c.name.as_str()).unwrap_or(&post.community);
    let mark = item.map(community_icon).unwrap_or_default();
    let title = escape(display_post_title(state, post));
    let wrapper = if full {
        format!("<h1>{title}</h1>")
    } else {
        format!("<h2><a href='{url}'>{title}</a></h2>")
    };
    let body_excerpt = if full || post.body.chars().count() <= 300 {
        post.body.clone()
    } else {
        format!("{}…", post.body.chars().take(297).collect::<String>())
    };
    let body = if body_excerpt.is_empty() {
        String::new()
    } else {
        format!("<p class='post-body'>{}</p>", rich_text(&body_excerpt))
    };
    let outgoing = if post.url.starts_with("https://") || post.url.starts_with("http://") {
        format!("<a class='outgoing' href='{}' target='_blank' rel='noopener noreferrer'>Open linked article ↗</a>", escape(&post.url))
    } else {
        String::new()
    };
    let image = if post.image {
        "<img class='post-image' src='/plaza.jpg' width='1260' height='419' alt='People crossing a sunlit public plaza with concrete terraces and green trees'>"
    } else {
        ""
    };
    let sensitive = is_sensitive(state, post);
    let media = post_attachments(post);
    let poll = post_poll(state, post, back);
    let repost = repost_card(state, post);
    let content = format!("{body}{outgoing}{image}{media}{poll}{repost}");
    let preview = if (post.spoiler || sensitive && state.sensitive != "show") && !content.is_empty()
    {
        let label = if post.spoiler {
            "Reveal spoiler"
        } else {
            "Show sensitive preview"
        };
        format!("<details class='sensitive-preview'><summary>{label}</summary>{content}</details>")
    } else {
        content
    };
    let mut flair = if post.flair.is_empty() {
        String::new()
    } else {
        format!(
            "<span class='flair{}'>{}</span>",
            if post.sensitive { " sensitive" } else { "" },
            escape(&post.flair)
        )
    };
    if sensitive {
        flair.push_str(nsfw_badge());
    }
    if post.spoiler {
        flair.push_str("<span class='content-tag spoiler-tag'>Spoiler</span>");
    }
    if post.brand_affiliate {
        flair.push_str("<span class='content-tag affiliate-tag'>Brand affiliate</span>");
    }
    let saved = state.saved.contains(&post.id);
    let hide_label = if state.hidden.contains(&post.id) {
        "Unhide for me"
    } else {
        "Hide for me"
    };
    let mut menu = action_form("hide", back, &[("id", &id)], hide_label, "", "menu-item");
    let source_id = post.repost_of.unwrap_or(post.id);
    if state.repost_source(source_id).is_some() {
        write!(
            menu,
            "<a class='menu-item' href='/new?type=repost&amp;source={source_id}'>Repost</a>"
        )
        .unwrap();
    }
    menu.push_str(&action_form(
        "mute",
        back,
        &[("community", &post.community)],
        if state.muted.contains(&post.community) {
            "Unmute community"
        } else {
            "Mute community"
        },
        "",
        "menu-item",
    ));
    if post.author == state.username {
        if can_edit_post(state, post) {
            write!(
                menu,
                "<a class='menu-item' href='/edit?id={id}'>Edit post</a>"
            )
            .unwrap();
        }
        menu.push_str(&action_form(
            "delete",
            "/",
            &[("id", &id)],
            "Delete post",
            "",
            "menu-item",
        ));
    } else {
        menu.push_str(&action_form(
            "block",
            back,
            &[("author", &post.author)],
            if state.blocked.contains(&post.author) {
                "Unblock account"
            } else {
                "Block account"
            },
            "",
            "menu-item",
        ));
    }
    write!(
        menu,
        "<a class='menu-item' href='/report?id={id}'>Report a legal concern</a>"
    )
    .unwrap();
    let comment_count = post.reply_count;
    let event = event_details(state, post.id);
    let pin_label = if pinned {
        format!(
            "<div class='pinned-label'>{} Pinned discussion</div>",
            icon("pin")
        )
    } else {
        String::new()
    };
    format!(
        r#"<article class="panel post{kind}">{pin_label}<div class="post-meta"><a class="post-community" href="/?view={slug}">{mark}{name}</a><span class="meta-dot">·</span>{author}<span class="meta-dot">·</span><span>{age}</span>{flair}<details class="post-menu"><summary aria-label="Post actions">{more}</summary><div class="menu-popover">{menu}</div></details></div>
{wrapper}{event}{preview}<div class="post-actions">{votes}<a class="action comment-link" href="{url}#comments">{chat} <span>{comment_count} comments</span></a>{save}</div></article>"#,
        kind = if full { " full-post" } else { "" },
        slug = encode(&post.community),
        name = escape(name),
        author = if state.has_profile(&post.author) {
            format!(
                "<a class='post-author' href='{}'>u/{}</a>",
                format_args!("/u/{}", encode(&post.author)),
                escape(&post.author)
            )
        } else {
            format!("<span>u/{}</span>", escape(&post.author))
        },
        age = escape(&post.age),
        more = icon("more"),
        votes = votes(
            "vote",
            post.id,
            score(state, post),
            *state.votes.get(&post.id).unwrap_or(&0),
            back
        ),
        chat = icon("chat"),
        save = action_form(
            "save",
            back,
            &[("id", &id)],
            if saved { "Saved" } else { "Save" },
            "save",
            if saved { "action saved" } else { "action" }
        ),
    )
}

fn post_attachments(post: &Post) -> String {
    let Some(first) = post.media.first() else {
        return String::new();
    };
    let source = |index| format!("/posts/media?id={}&amp;index={index}", post.id);
    if first.content_type.starts_with("video/") {
        let url = source(0);
        let mime = escape(&first.content_type);
        return format!("<div class='post-video'><video controls preload='metadata' playsinline aria-label='Video attachment'><source src='{url}' type='{mime}'><a href='{url}'>Open video attachment</a></video></div>");
    }

    let class = if post.media.len() == 1 {
        "post-gallery post-gallery-single"
    } else {
        "post-gallery"
    };
    let mut html = format!("<div class='{class}' aria-label='Image attachments'>");
    for index in 0..post.media.len() {
        let url = source(index);
        let label = index + 1;
        write!(html, "<a href='{url}' target='_blank' rel='noopener' aria-label='Open image {label}'><img src='{url}' alt='Image attachment {label}' loading='lazy'></a>").unwrap();
    }
    html.push_str("</div>");
    html
}

fn post_poll(state: &State, post: &Post, back: &str) -> String {
    let Some(poll) = &post.poll else {
        return String::new();
    };
    let closed = schedule_time::now() >= poll.closes_at;
    let own_vote = poll.votes.get(&state.username).copied();
    let total = poll.votes.len();
    let singular = if total == 1 { "vote" } else { "votes" };
    let deadline = if closed {
        "Closed".into()
    } else {
        format!(
            "Closes <time datetime='{}Z'>{}</time>",
            schedule_time::to_local(poll.closes_at, "UTC"),
            escape(&schedule_time::display(poll.closes_at, "America/Chicago"))
        )
    };
    let mut html = format!("<section class='post-poll' aria-label='Poll'><p class='poll-summary'>{total} {singular} · {deadline}</p>");
    if !closed && own_vote.is_none() {
        write!(html, "<form method='post' action='/action' class='poll-vote'>{}<fieldset><legend class='sr-only'>Choose an answer</legend>", action_fields("vote-poll", back, &[("id", &post.id.to_string())])).unwrap();
        for (index, option) in poll.options.iter().enumerate() {
            write!(
                html,
                "<label><input type='radio' name='option' value='{index}' required> {}</label>",
                escape(option)
            )
            .unwrap();
        }
        html.push_str(
            "</fieldset><button class='button light' type='submit'>Vote in poll</button></form>",
        );
    } else {
        html.push_str("<ul class='poll-results'>");
        for (index, option) in poll.options.iter().enumerate() {
            let group_votes = poll
                .votes
                .values()
                .filter(|choice| **choice == index)
                .count();
            let percent = group_votes.saturating_mul(100) / total.max(1);
            let picked = if own_vote == Some(index) {
                " <span class='poll-picked'>Your vote</span>"
            } else {
                ""
            };
            write!(html, "<li><div><span>{}{picked}</span><strong>{percent}%</strong></div><meter min='0' max='100' value='{percent}' aria-label='{} vote share'>{percent}%</meter></li>", escape(option), escape(option)).unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

fn repost_card(state: &State, post: &Post) -> String {
    let Some(id) = post.repost_of else {
        return String::new();
    };
    let Some(original) = state.repost_source(id) else {
        return "<p class='post-repost muted'>Original discussion unavailable.</p>".into();
    };
    let source = community(state, &original.community)
        .map_or(original.community.as_str(), |item| item.name.as_str());
    format!("<aside class='post-repost' aria-label='Original discussion'><p>Originally posted in <a href='{}'>{}</a></p><a class='repost-original-title' href='/thread?id={id}'>{}</a></aside>", community_url(&original.community), escape(source), escape(&original.title))
}

fn event_details(state: &State, post_id: u64) -> String {
    let details = state.scheduled.iter().find(|item| {
        item.post_id == Some(post_id) && item.kind == "event" && item.status == "published"
    });
    let Some(event) = details else {
        return String::new();
    };
    let Some(start) = event.event_start else {
        return String::new();
    };
    let time = |timestamp| {
        format!(
            "<time datetime='{}Z'>{}</time>",
            schedule_time::to_local(timestamp, "UTC"),
            escape(&schedule_time::display(timestamp, "America/Chicago"))
        )
    };
    let end = event
        .event_end
        .map(|stamp| format!("<span><strong>Ends</strong> {}</span>", time(stamp)))
        .unwrap_or_default();
    format!("<div class='post-event'>{}<div><span><strong>Event starts</strong> {}</span>{end}</div></div>", icon("calendar"), time(start))
}

fn rich_text(mut remaining: &str) -> String {
    let mut html = String::new();
    while let Some(index) = remaining.find(['*', '`']) {
        html.push_str(&escape(&remaining[..index]));
        let span = &remaining[index..];
        let (delimiter, tag) = if span.starts_with("**") {
            ("**", "strong")
        } else if span.starts_with('*') {
            ("*", "em")
        } else {
            ("`", "code")
        };
        let text = &span[delimiter.len()..];
        if let Some(end) = text
            .find(delimiter)
            .filter(|end| *end > 0 && !text[..*end].contains('\n'))
        {
            write!(html, "<{tag}>{}</{tag}>", escape(&text[..end])).unwrap();
            remaining = &text[end + delimiter.len()..];
        } else {
            html.push_str(delimiter);
            remaining = text;
        }
    }
    html.push_str(&escape(remaining));
    html
}

#[derive(Clone, Copy)]
enum CommentAction<'a> {
    Read,
    Reply(u64),
    Edit {
        id: u64,
        draft: Option<&'a str>,
        error: &'a str,
    },
    Delete {
        id: u64,
        error: &'a str,
    },
}

impl CommentAction<'_> {
    fn from_query(query: &BTreeMap<String, String>) -> Self {
        if let Ok(id) = field(query, "edit-comment").parse() {
            Self::Edit {
                id,
                draft: None,
                error: "",
            }
        } else if let Ok(id) = field(query, "delete-comment").parse() {
            Self::Delete { id, error: "" }
        } else if let Ok(id) = field(query, "reply").parse() {
            Self::Reply(id)
        } else {
            Self::Read
        }
    }
}

fn thread(state: &State, post: &Post, action: CommentAction<'_>) -> String {
    let back = format!("/thread?id={}", post.id);
    let mut html = String::from("<a class='back-link' href='/'>← Front page</a>");
    let pinned =
        community(state, &post.community).is_some_and(|item| item.pinned_post == Some(post.id));
    html.push_str(&post_card(state, post, &back, true, pinned));
    html.push_str("<section class='panel discussion' id='comments'><h2>Discussion</h2>");
    let can_reply = can_write_here(state, &post.community);
    if can_reply {
        html.push_str(&reply_form(post.id, None, &back));
    } else if state.username.is_empty() {
        html.push_str("<p class='community-restriction-note muted'><a class='subtle-link' href='/account/login'>Sign in</a> to reply.</p>");
    } else if state.is_banned(&post.community, &state.username) {
        html.push_str("<p class='community-restriction-note muted'>Your account is banned from replying in this community.</p>");
    } else {
        html.push_str(
            "<p class='community-restriction-note muted'>This community limits who can reply.</p>",
        );
    }
    let branches = comment_branches(state, post.id);
    if branches.is_empty() {
        html.push_str(if can_reply {
            "<p class='muted'>Be the first to reply.</p>"
        } else {
            "<p class='muted'>No replies yet.</p>"
        });
    }
    let mut stack: Vec<_> = branches
        .get(&None)
        .into_iter()
        .flatten()
        .rev()
        .map(|comment| (*comment, 0usize))
        .collect();
    while let Some((comment, depth)) = stack.pop() {
        if comment.deleted {
            write!(html, "<article class='comment comment-deleted depth-{}' id='comment-{}'><p>Comment deleted</p></article>", depth.min(3), comment.id).unwrap();
        } else if !state.blocked.contains(&comment.author) {
            html.push_str(&comment_card(
                state, comment, depth, &back, action, can_reply,
            ));
        }
        if let Some(children) = branches.get(&Some(comment.id)) {
            stack.extend(children.iter().rev().map(|child| (*child, depth + 1)));
        }
    }
    html.push_str("</section>");
    html
}

fn comment_branches(state: &State, post: u64) -> BTreeMap<Option<u64>, Vec<&Comment>> {
    let comments: BTreeMap<_, _> = state
        .comments
        .iter()
        .filter(|comment| comment.post == post)
        .map(|comment| (comment.id, comment))
        .collect();
    // Keep a deleted ancestor only when it anchors a surviving reply.
    let mut ancestors = BTreeSet::new();
    for comment in comments.values().filter(|comment| !comment.deleted) {
        let mut parent = comment.parent;
        while let Some(placeholder) = parent
            .and_then(|id| comments.get(&id))
            .filter(|item| item.deleted && ancestors.insert(item.id))
        {
            parent = placeholder.parent;
        }
    }
    let mut branches: BTreeMap<_, Vec<&Comment>> = BTreeMap::new();
    for comment in comments
        .values()
        .filter(|comment| !comment.deleted || ancestors.contains(&comment.id))
    {
        branches.entry(comment.parent).or_default().push(comment);
    }
    for siblings in branches.values_mut() {
        siblings.sort_by_key(|comment| std::cmp::Reverse(state.comment_score(comment)));
    }
    branches
}

fn comment_card(
    state: &State,
    comment: &Comment,
    depth: usize,
    back: &str,
    action: CommentAction<'_>,
    can_reply: bool,
) -> String {
    let vote = *state.comment_votes.get(&comment.id).unwrap_or(&0);
    let avatar = escape(&initials(&comment.author));
    let handle = escape(&comment.author);
    let author = if state.has_profile(&comment.author) {
        format!(
            "<a class='post-author' href='/u/{}'>u/{handle}</a>",
            encode(&comment.author)
        )
    } else {
        format!("<strong>u/{handle}</strong>")
    };
    let owns = comment.author == state.username;
    let (body, controls) = match action {
        CommentAction::Edit { id, draft, error } if id == comment.id && owns && can_reply => (
            discussion::editor(comment, back, draft.unwrap_or(&comment.body), error),
            String::new(),
        ),
        _ => {
            let controls = match action {
                CommentAction::Reply(id) if id == comment.id && can_reply => {
                    reply_form(comment.post, Some(comment.id), back)
                }
                CommentAction::Delete { id, error } if id == comment.id && owns => {
                    discussion::confirm_delete(comment, back, error)
                }
                _ => String::new(),
            };
            (format!("<p>{}</p>", escape(&comment.body)), controls)
        }
    };
    let edited = comment
        .edited_at
        .map(|timestamp| {
            format!(
                "<time class='comment-edited' datetime='{}Z' title='{}'>Edited</time>",
                schedule_time::to_local(timestamp, "UTC"),
                escape(&schedule_time::display(timestamp, "America/Chicago"))
            )
        })
        .unwrap_or_default();
    let menu = discussion::menu(state, comment, back, can_reply);
    let reply_link = if can_reply {
        format!(
            "<a class='action' href='{back}&amp;reply={}#comment-{}'>{} Reply</a>",
            comment.id,
            comment.id,
            icon("chat")
        )
    } else {
        String::new()
    };
    format!("<article class='comment depth-{}' id='comment-{}'><div class='comment-head'><span class='avatar'>{avatar}</span>{author}<span>· {}</span>{edited}{menu}</div>{body}<div class='post-actions'>{}{reply_link}</div>{controls}</article>",
        depth.min(3), comment.id, escape(&comment.age), votes("comment-vote", comment.id, state.comment_score(comment), vote, back)
    )
}

fn reply_form(post: u64, parent: Option<u64>, back: &str) -> String {
    let id = post.to_string();
    let target = parent
        .map(|value| hidden("parent", &value.to_string()))
        .unwrap_or_default();
    format!("<form class='reply-form' method='post' action='/action'>{}{target}<label><span class='sr-only'>Write a reply</span><textarea name='body' required maxlength='4000' rows='3' placeholder='Add to the discussion'></textarea></label><button class='button dark' type='submit'>Post reply</button></form>", action_fields("comment", back, &[("post", &id)]))
}

fn editor(state: &State, post: Option<&Post>, selected: &str, draft: Option<&PostDraft>) -> String {
    let (title, body, url, flair, tags) = match (post, draft) {
        (Some(item), _) => (
            item.title.as_str(),
            item.body.as_str(),
            item.url.as_str(),
            item.flair.as_str(),
            [item.sensitive, item.spoiler, item.brand_affiliate],
        ),
        (_, Some(item)) => (
            item.title.as_str(),
            item.body.as_str(),
            item.url.as_str(),
            item.flair.as_str(),
            [item.sensitive, item.spoiler, item.brand_affiliate],
        ),
        _ => ("", "", "", state.profile_flair.as_str(), [false; 3]),
    };
    let id = post.map(|item| item.id.to_string()).unwrap_or_default();
    let selected = draft
        .map(|item| item.community.as_str())
        .unwrap_or(selected);
    let action = if post.is_some() { "edit" } else { "create" };
    let back = post
        .map(|p| format!("/thread?id={}", p.id))
        .unwrap_or_else(|| "/".into());
    let options = state
        .communities
        .iter()
        .filter(|item| state.can_contribute(item))
        .map(|item| {
            format!(
                "<option value='{}'{}>{}</option>",
                escape(&item.slug),
                if item.slug == selected {
                    " selected"
                } else {
                    ""
                },
                escape(&item.name)
            )
        })
        .collect::<String>();
    let new_fields = if post.is_none() {
        format!("<label>Community<select name='community' required><option value=''>Choose a community</option>{options}</select></label><label>Post flair <small>Optional</small><input name='flair' maxlength='32' value='{}' placeholder='Discussion, question, or link'></label>", escape(flair))
    } else {
        String::new()
    };
    let tag_fields = [
        ("sensitive", "NSFW (18+)", "Contains mature or adult content"),
        ("spoiler", "Spoiler", "Collapses the post body until someone reveals it"),
        ("brand_affiliate", "Brand affiliate", "Discloses that this post was made for a brand or business"),
    ].into_iter().zip(tags).map(|((name, label, help), checked)| {
        format!("<label class='composer-toggle'><span><strong>{label}</strong><small>{help}</small></span><input type='checkbox' name='{name}'{}></label>", if checked { " checked" } else { "" })
    }).collect::<String>();
    let resume = if post.is_none() && state.draft.is_some() && draft.is_none() {
        "<a class='button light' href='/new?draft=1'>Resume saved draft</a>"
    } else {
        ""
    };
    let save = if post.is_none() {
        "<button class='button light' type='submit' name='save_draft' value='1' formnovalidate>Save draft</button>"
    } else {
        ""
    };
    let discard = if draft.is_some() {
        action_form(
            "delete-draft",
            "/new",
            &[],
            "Discard draft",
            "",
            "button light",
        )
    } else {
        String::new()
    };
    let tabs = if post.is_none() {
        publishing::tabs("text", selected)
    } else {
        String::new()
    };
    format!(
        r#"<section class="panel standalone composer"><header class="creator-header"><h1>{heading}</h1>{resume}</header>{tabs}<form class="stack-form" method="post" action="/action">{fields}{new_fields}
<label>Title<input name="title" required maxlength="160" value="{title}" placeholder="What are we discussing?"></label>
<details class="composer-tags"{tags_open}><summary>{tag_icon} Add tags</summary><div class="composer-tag-options">{tag_fields}</div></details>
<label>Body text <small>Optional</small><textarea name="body" rows="7" maxlength="8000" placeholder="Add context or ask a question">{body}</textarea></label>
<details class="markup-guide"><summary>Formatting help</summary><p>Use <code>**bold**</code>, <code>*italic*</code>, or <code>`inline code`</code> in the body.</p></details>
<label>Link <small>Optional</small><input name="url" type="url" maxlength="600" value="{url}" placeholder="https://"></label>
<div class="form-actions"><a class="button light" href="{back}">Cancel</a><button class="button dark" type="submit">{submit}</button>{save}</div></form>{discard}</section>"#,
        heading = if post.is_some() {
            "Edit post"
        } else {
            "New post"
        },
        fields = action_fields(
            action,
            &back,
            &[
                ("id", &id),
                ("tags_present", "1"),
                ("from_draft", if draft.is_some() { "1" } else { "0" })
            ]
        ),
        tag_icon = icon("tag"),
        tags_open = if tags.contains(&true) { " open" } else { "" },
        title = escape(title),
        body = escape(body),
        url = escape(url),
        submit = if post.is_some() {
            "Save changes"
        } else {
            "Publish post"
        },
    )
}

fn preferences(state: &State) -> String {
    let mut html = String::from("<section class='panel standalone'><h1>Feed controls</h1><p>These preferences apply only to your view of Commonline.</p>");
    let radios = [("show", "Show"), ("blur", "Collapse previews"), ("hide", "Hide")].into_iter().map(|(option, label)| {
        format!("<label class='checkbox'><input type='radio' name='sensitive' value='{option}'{}> {label}</label>", if state.sensitive == option { " checked" } else { "" })
    }).collect::<String>();
    write!(html, "<form class='stack-form' method='post' action='/action'>{}<fieldset><legend>Sensitive posts</legend>{radios}</fieldset><label>Muted keywords <small>One per line</small><textarea name='keywords' rows='4' maxlength='2000' placeholder='Enter a word or phrase'>{}</textarea></label><button class='button dark' type='submit'>Save preferences</button></form>", action_fields("preferences", "/settings", &[]), escape(&state.keywords.join("\n"))).unwrap();
    html.push_str("<div class='preference-list'><h2>Muted communities</h2>");
    if state.muted.is_empty() {
        html.push_str("<p class='muted'>You haven’t muted any communities.</p>");
    }
    for slug in &state.muted {
        let name = community(state, slug)
            .map(|item| item.name.as_str())
            .unwrap_or(slug);
        write!(
            html,
            "<div class='preference-row'><span>{}</span>{}</div>",
            escape(name),
            action_form(
                "mute",
                "/settings",
                &[("community", slug)],
                "Unmute",
                "",
                "button light"
            )
        )
        .unwrap();
    }
    html.push_str("<h2>Blocked accounts</h2>");
    if state.blocked.is_empty() {
        html.push_str("<p class='muted'>You haven’t blocked any accounts.</p>");
    }
    for author in &state.blocked {
        write!(
            html,
            "<div class='preference-row'><span>u/{}</span>{}</div>",
            escape(author),
            action_form(
                "block",
                "/settings",
                &[("author", author)],
                "Unblock",
                "",
                "button light"
            )
        )
        .unwrap();
    }
    html.push_str("<h2>Hidden posts</h2>");
    if state.hidden.is_empty() {
        html.push_str("<p class='muted'>You haven’t hidden any posts.</p>");
    }
    for post in state
        .posts
        .iter()
        .filter(|post| state.hidden.contains(&post.id))
    {
        write!(
            html,
            "<div class='preference-row'><span>{}</span>{}</div>",
            escape(&post.title),
            action_form(
                "hide",
                "/settings",
                &[("id", &post.id.to_string())],
                "Unhide",
                "",
                "button light"
            )
        )
        .unwrap();
    }
    html.push_str("</div></section>");
    html
}

fn policy() -> String {
    String::from(
        r#"<section class="panel standalone policy-page"><div class="overline"><span></span> MODERATION POLICY</div><h1>Lawful speech stays up.</h1><p>Commonline is designed to allow lawful discussion, including controversial viewpoints and sensitive topics. You choose what appears in your feed.</p>
<h2>Personal controls</h2><p>Mute communities or keywords, block an account, hide a post, or collapse sensitive previews. These choices apply to your view.</p>
<h2>Legal review</h2><p>Use a post’s menu to report a possible legal violation. Tell us which country or region applies and why the content should be reviewed. Submitting a report leaves the content available while the issue is reviewed.</p>
<h2>Restrictions and review requests</h2><p>The proposed policy would identify the reason and region when applicable law requires a restriction and provide a way to request a review. This prototype records sample reports on this device.</p>
<div class="form-actions"><a class="button dark" href="/reports">View review log</a><a class="button light" href="/settings">Feed controls</a></div></section>"#,
    )
}

fn reports(state: &State) -> String {
    let mut html = String::from("<section class='panel standalone'><h1>Legal review log</h1><p>Reports you submitted appear here.</p>");
    let mut reports = state
        .reports
        .iter()
        .rev()
        .filter(|report| report.reporter == state.username)
        .peekable();
    if reports.peek().is_none() {
        html.push_str("<div class='log-empty'><h2>No reports yet</h2><p>Use a post’s menu to report a legal concern.</p></div>");
    }
    for report in reports {
        let title = state
            .posts
            .iter()
            .find(|post| post.id == report.post)
            .map(|post| post.title.as_str())
            .unwrap_or("Discussion unavailable");
        let status = if report.reviewed {
            "Reviewed"
        } else {
            "Pending review"
        };
        write!(html, "<article class='log-item'><span class='flair'>{status}</span><h2><a href='/thread?id={}'>{}</a></h2><p><strong>Region</strong> {}</p><p>{}</p></article>", report.post, escape(title), escape(&report.region), escape(&report.details)).unwrap();
    }
    html.push_str("</section>");
    html
}

fn report_form(post: &Post) -> String {
    format!(
        r#"<section class="panel standalone"><h1>Report a legal concern</h1><p>Report a possible legal violation in “{title}”.</p><form class="stack-form" method="post" action="/action">{fields}
<label>Country or region<input name="region" required maxlength="90" placeholder="Where does the concern apply?"></label>
<label>Why should this be reviewed?<textarea name="details" required maxlength="4000" rows="6" placeholder="Describe the issue and include a legal reference if you have one."></textarea></label>
<p class="muted">Your report will be recorded in the local review log as pending.</p><div class="form-actions"><a class="button light" href="/thread?id={id}">Cancel</a><button class="button dark" type="submit">Submit report</button></div></form></section>"#,
        title = escape(&post.title),
        id = post.id,
        fields = action_fields("report", "/reports", &[("post", &post.id.to_string())]),
    )
}

fn missing(state: &State) -> Response {
    Response::not_found(layout(state, "", "Page unavailable", "<section class='panel standalone'><h1>Page unavailable</h1><p>This discussion or page couldn’t be found.</p><a class='button dark' href='/'>Front page</a></section>", "", ""))
}

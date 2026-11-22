use std::collections::BTreeMap;

use super::{
    action_fields, community, community_art, community_art_details, community_editor,
    community_image_url, community_owner, community_preview_image_url, encode, escape, field,
    hidden, layout, missing,
};
use crate::http::{Request, Response};
use crate::model::{Community, SetupArtwork, State};

const PREVIEW_ROUTE: &str = "/communities/appearance/review";
const EDIT_ROUTE: &str = "/communities/appearance/settings";

pub(super) fn submit(mut request: Request, state: &mut State) -> Response {
    let slug = field(&request.form, "community");
    if community(state, slug).is_none_or(|item| !community_owner(state, item)) {
        return missing(state);
    }
    let reference = field(&request.form, "artwork_preview");
    let original = reference.parse().ok();
    if !reference.is_empty() && original.is_none() {
        return expired(state, slug);
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
    match state.preview_community_artwork(
        slug,
        original,
        icon_mime.as_deref().zip(icon_bytes),
        banner_mime.as_deref().zip(banner_bytes),
        field(&request.form, "remove_icon") == "on",
        field(&request.form, "remove_banner") == "on",
    ) {
        Ok(id) => Response::redirect(&format!(
            "{PREVIEW_ROUTE}?community={}&artwork_preview={id}",
            encode(slug)
        )),
        Err(message) => error_page(state, slug, original, &message),
    }
}

pub(super) fn review_page(state: &State, query: &BTreeMap<String, String>) -> Response {
    let slug = field(query, "community");
    let Some(item) = community(state, slug).filter(|item| community_owner(state, item)) else {
        return missing(state);
    };
    let draft = field(query, "artwork_preview")
        .parse()
        .ok()
        .and_then(|id| state.community_artwork(slug, id));
    let Some(draft) = draft else {
        return expired(state, slug);
    };
    let (icon, banner) = artwork_urls(item, Some(draft));
    let preview = community_art(
        icon.as_deref(),
        banner.as_deref(),
        &community_art_details(item, false),
    );
    let settings = settings_url(slug);
    let edit = format!(
        "{settings}&artwork_preview={}#community-appearance",
        draft.id
    );
    let cancel = format!("{settings}#community-appearance");
    let fields = action_fields(
        "save-community-artwork",
        &settings,
        &[
            ("community", slug),
            ("artwork_preview", &draft.id.to_string()),
        ],
    );
    let content = format!("<section class='panel standalone appearance-review'><h1>Preview appearance</h1><div class='appearance-art'>{preview}</div><form class='form-actions' method='post' action='/action'>{fields}<button class='button dark' type='submit'>Save appearance</button><a class='button light' href='{}'>Edit images</a><a class='button light' href='{}'>Cancel</a></form></section>", escape(&edit), escape(&cancel));
    Response::html(layout(state, slug, "Preview appearance", &content, "", ""))
}

pub(super) fn settings_content(state: &State, item: &Community, reference: &str) -> String {
    let draft = reference
        .parse()
        .ok()
        .and_then(|id| state.community_artwork(&item.slug, id));
    let error = if !reference.is_empty() && draft.is_none() {
        "This preview has expired. Preview your changes again."
    } else {
        ""
    };
    community_editor(state, Some(item)) + &settings_panel(item, draft, error)
}

pub(super) fn error_page(
    state: &State,
    slug: &str,
    original: Option<u64>,
    message: &str,
) -> Response {
    let Some(item) = community(state, slug).filter(|item| community_owner(state, item)) else {
        return missing(state);
    };
    let draft = original.and_then(|id| state.community_artwork(slug, id));
    let content = settings_panel(item, draft, message);
    Response::html(layout(
        state,
        slug,
        "Community appearance",
        &content,
        "",
        "",
    ))
}

fn expired(state: &State, slug: &str) -> Response {
    error_page(
        state,
        slug,
        None,
        "This preview has expired. Preview your changes again.",
    )
}

fn settings_panel(item: &Community, draft: Option<&SetupArtwork>, error: &str) -> String {
    let fields = hidden("community", &item.slug)
        + &draft.map_or_else(String::new, |item| {
            hidden("artwork_preview", &item.id.to_string())
        });
    let (icon, banner) = artwork_urls(item, draft);
    let pickers = super::community_setup::artwork_fields(icon.as_deref(), banner.as_deref());
    let alert = if error.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='notice setup-error' role='alert'>{}</p>",
            escape(error)
        )
    };
    let status = if draft.is_some() {
        "<p class='muted'>Editing an unsaved preview.</p>"
    } else {
        ""
    };
    let discard = if draft.is_some() {
        format!(
            "<a class='button light' href='{}#community-appearance'>Discard preview</a>",
            escape(&settings_url(&item.slug))
        )
    } else {
        String::new()
    };
    format!("<section class='panel standalone community-appearance' id='community-appearance'><h2>Appearance</h2>{alert}{status}<form class='stack-form' action='{EDIT_ROUTE}' method='post' enctype='multipart/form-data'>{fields}{pickers}<div class='form-actions'><button class='button dark' type='submit'>Preview changes</button>{discard}</div></form></section>")
}

fn artwork_urls(
    item: &Community,
    draft: Option<&SetupArtwork>,
) -> (Option<String>, Option<String>) {
    if let Some(draft) = draft {
        (
            draft
                .icon
                .as_ref()
                .map(|_| community_preview_image_url(draft.id, "icon")),
            draft
                .banner
                .as_ref()
                .map(|_| community_preview_image_url(draft.id, "banner")),
        )
    } else {
        (
            item.icon_image
                .as_ref()
                .map(|_| community_image_url(&item.slug, "icon")),
            item.banner_image
                .as_ref()
                .map(|_| community_image_url(&item.slug, "banner")),
        )
    }
}

fn settings_url(slug: &str) -> String {
    format!("/communities/settings?community={}", encode(slug))
}

use std::{collections::BTreeMap, path::Path};

use crate::http::{Request, Response, Upload};
use crate::media;
use crate::model::{Comment, State};

use super::{escape, field, icon};

pub(super) fn submit(
    state: &mut State,
    path: &Path,
    form: &BTreeMap<String, String>,
    upload: Option<Upload>,
) -> Result<&'static str, String> {
    let image = upload
        .map(|upload| media::store_comment_image(path, upload))
        .transpose()?;
    let result = state.apply_comment(form, image.clone(), path);
    if result.is_err() {
        media::remove(path, image.as_slice());
    }
    result
}

pub(super) fn response(state: &State, path: &Path, request: &Request) -> Response {
    let image = field(&request.query, "id")
        .parse::<u64>()
        .ok()
        .and_then(|id| state.comments.iter().find(|comment| comment.id == id))
        .filter(|comment| state.can_view_comment(comment))
        .and_then(|comment| comment.image.as_ref())
        .and_then(|image| media::read(path, &image.asset));
    match image {
        Some((mime, bytes)) => Response::media(mime, bytes, request.range.as_deref()),
        None => Response::text(
            404,
            "text/plain; charset=utf-8",
            "Comment attachment unavailable.",
        ),
    }
}

pub(super) fn picker(description: &str, select_again: bool) -> String {
    let open = if select_again || !description.is_empty() {
        " open"
    } else {
        ""
    };
    let hint = if select_again {
        "Choose the file again before resubmitting. PNG, JPEG, WebP or GIF up to 2 MiB."
    } else {
        "PNG, JPEG, WebP or GIF up to 2 MiB."
    };
    format!(
        "<details class='comment-attachment-picker'{open}><summary>{}Attach an image or GIF</summary><div class='comment-attachment-fields'><label>Image file<input type='file' name='image' accept='image/png,image/jpeg,image/webp,image/gif'></label><small class='muted'>{hint}</small><label>Image description <span class='muted'>(optional)</span><input type='text' name='image_alt' maxlength='240' value='{}'></label></div></details>",
        icon("image"),
        escape(description),
    )
}

pub(super) fn render(comment: &Comment) -> String {
    let Some(image) = &comment.image else {
        return String::new();
    };
    let alt = if image.alt.is_empty() {
        if image.asset.content_type == "image/gif" {
            "GIF attached to this comment"
        } else {
            "Image attached to this comment"
        }
    } else {
        &image.alt
    };
    format!(
        "<figure class='comment-attachment'><img src='/comments/image?id={}' alt='{}' loading='lazy' decoding='async'></figure>",
        comment.id,
        escape(alt),
    )
}

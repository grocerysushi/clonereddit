use super::{action_fields, escape, icon};
use crate::model::{Comment, State};

pub(super) fn menu(state: &State, comment: &Comment, back: &str, can_edit: bool) -> String {
    if comment.deleted || comment.author != state.username {
        return String::new();
    }

    let edit = if can_edit {
        format!(
            "<a class='menu-item' href='{}'>Edit comment</a>",
            escape(&action_url(back, "edit-comment", comment.id))
        )
    } else {
        String::new()
    };
    let delete = escape(&action_url(back, "delete-comment", comment.id));
    format!(
        "<details class='comment-menu'><summary class='action' aria-label='Comment actions'>{}</summary><div class='menu-popover comment-menu-popover'>{edit}<a class='menu-item' href='{delete}'>Delete comment</a></div></details>",
        icon("more")
    )
}

pub(super) fn editor(comment: &Comment, back: &str, body: &str, error: &str) -> String {
    let fields = action_fields("edit-comment", back, &[("id", &comment.id.to_string())]);
    let cancel = escape(&comment_url(back, comment.id));
    let body = escape(body);
    let alert = form_error(error);
    format!(
        r#"<form class="stack-form comment-edit-form" method="post" action="/action" aria-label="Edit comment">{fields}{alert}
<label>Edit comment<textarea name="body" required maxlength="4000" rows="4">{body}</textarea></label>
<div class="form-actions comment-form-actions"><button class="button dark" type="submit">Save changes</button><a class="button light" href="{cancel}">Cancel</a></div></form>"#
    )
}

pub(super) fn confirm_delete(comment: &Comment, back: &str, error: &str) -> String {
    let id = comment.id;
    let fields = action_fields("delete-comment", back, &[("id", &id.to_string())]);
    let cancel = escape(&comment_url(back, id));
    let alert = form_error(error);
    format!(
        r#"<section class="comment-delete-confirm" aria-labelledby="delete-comment-heading-{id}"><h3 id="delete-comment-heading-{id}">Delete this comment?</h3><p class="muted">Replies will stay in the discussion.</p>{alert}
<form class="form-actions comment-delete-form comment-form-actions" method="post" action="/action">{fields}<button class="button dark" type="submit">Delete comment</button><a class="button light" href="{cancel}">Cancel</a></form></section>"#
    )
}

fn action_url(back: &str, action: &str, id: u64) -> String {
    let separator = if back.contains('?') { '&' } else { '?' };
    format!("{back}{separator}{action}={id}#comment-{id}")
}

fn comment_url(back: &str, id: u64) -> String {
    format!("{back}#comment-{id}")
}

fn form_error(error: &str) -> String {
    if error.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='notice comment-form-error' role='alert'>{}</p>",
            escape(error)
        )
    }
}

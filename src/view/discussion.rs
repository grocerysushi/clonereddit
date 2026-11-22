use super::{action_fields, escape, icon, reporting};
use crate::model::{Comment, ContentKind, State};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CommentSort {
    Best,
    Top,
    New,
    Old,
}

impl CommentSort {
    pub(super) fn parse(value: &str) -> Self {
        match value {
            "top" => Self::Top,
            "new" => Self::New,
            "old" => Self::Old,
            _ => Self::Best,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Best => "best",
            Self::Top => "top",
            Self::New => "new",
            Self::Old => "old",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Best => "Best",
            Self::Top => "Top",
            Self::New => "New",
            Self::Old => "Old",
        }
    }

    pub(super) fn thread_url(self, post: u64) -> String {
        format!("/thread?id={post}&comment_sort={}", self.key())
    }

    pub(super) fn form_field(self) -> (&'static str, &'static str) {
        ("comment_sort", self.key())
    }
}

pub(super) fn sort_tabs(post: u64, current: CommentSort) -> String {
    let links: String = [
        CommentSort::Best,
        CommentSort::Top,
        CommentSort::New,
        CommentSort::Old,
    ]
    .into_iter()
    .map(|sort| {
        let selection = if sort == current {
            " aria-current='page'"
        } else {
            ""
        };
        format!(
            "<a href='{}#comments'{selection}>{}</a>",
            escape(&sort.thread_url(post)),
            sort.label()
        )
    })
    .collect();
    format!(
        "<nav class='comment-sort' aria-label='Sort comments'><span>Sort by</span>{links}</nav>"
    )
}

pub(super) fn open_branch(id: u64, depth: usize) -> String {
    format!("<details class='comment-branch depth-{}' open data-comment='{id}'><summary>{}<span class='branch-collapse-label'>Hide replies</span><span class='branch-expand-label'>Show replies</span></summary>", depth.min(3), icon("chevron"))
}

pub(super) fn menu(
    state: &State,
    comment: &Comment,
    back: &str,
    origin: &str,
    can_edit: bool,
    moderation: Option<&str>,
) -> String {
    if comment.deleted {
        return String::new();
    }
    let signed_in = !state.username.is_empty();
    let owns = signed_in && comment.author == state.username;

    let edit = if owns && can_edit {
        format!(
            "<a class='menu-item' href='{}'>Edit comment</a>",
            escape(&action_url(back, "edit-comment", comment.id))
        )
    } else {
        String::new()
    };
    let delete = if owns {
        let url = escape(&action_url(back, "delete-comment", comment.id));
        format!("<a class='menu-item' href='{url}'>Delete comment</a>")
    } else {
        String::new()
    };
    let moderate = moderation.map_or_else(String::new, |url| {
        format!(
            "<a class='menu-item' href='{}'>Moderate comment</a>",
            escape(url)
        )
    });
    let report = if signed_in {
        let url = escape(&reporting::content_url(ContentKind::Comment, comment.id));
        format!("<a class='menu-item' href='{url}'>Report comment</a>")
    } else {
        String::new()
    };
    let share = share(comment, origin);
    format!(
        "<details class='comment-menu'><summary class='action' aria-label='Comment actions'>{}</summary><div class='menu-popover comment-menu-popover'>{share}{edit}{delete}{report}{moderate}</div></details>",
        icon("more")
    )
}

fn share(comment: &Comment, origin: &str) -> String {
    let permalink = comment_url(&format!("/thread?id={}", comment.post), comment.id);
    let absolute = escape(&format!("{origin}{permalink}"));
    let permalink = escape(&permalink);
    format!("<details class='comment-share' data-comment-share><summary class='menu-item'>Share</summary><div class='comment-share-controls'><label>Comment link<input type='url' readonly data-comment-link value='{absolute}'></label><div class='comment-share-actions'><button type='button' class='button light' data-comment-copy hidden>Copy link</button><a class='subtle-link' data-comment-open href='{permalink}'>Open comment</a></div><p class='muted' data-comment-share-status role='status'></p></div></details>")
}

pub(super) fn editor(
    comment: &Comment,
    back: &str,
    sort: CommentSort,
    body: &str,
    error: &str,
) -> String {
    let fields = action_fields(
        "edit-comment",
        back,
        &[("id", &comment.id.to_string()), sort.form_field()],
    );
    let cancel = escape(&comment_url(back, comment.id));
    let body = escape(body);
    let alert = form_error(error);
    format!(
        r#"<form class="stack-form comment-edit-form" method="post" action="/action" aria-label="Edit comment">{fields}{alert}
<label>Edit comment<textarea name="body" required maxlength="4000" rows="4">{body}</textarea></label>
<div class="form-actions comment-form-actions"><button class="button dark" type="submit">Save changes</button><a class="button light" href="{cancel}">Cancel</a></div></form>"#
    )
}

pub(super) fn confirm_delete(
    comment: &Comment,
    back: &str,
    sort: CommentSort,
    error: &str,
) -> String {
    let id = comment.id;
    let fields = action_fields(
        "delete-comment",
        back,
        &[("id", &id.to_string()), sort.form_field()],
    );
    let cancel = escape(&comment_url(back, id));
    let alert = form_error(error);
    format!(
        r#"<section class="comment-delete-confirm" aria-labelledby="delete-comment-heading-{id}"><h3 id="delete-comment-heading-{id}">Delete this comment?</h3><p class="muted">Replies will stay in the discussion.</p>{alert}
<form class="form-actions comment-delete-form comment-form-actions" method="post" action="/action">{fields}<button class="button dark" type="submit">Delete comment</button><a class="button light" href="{cancel}">Cancel</a></form></section>"#
    )
}

pub(super) fn action_url(back: &str, action: &str, id: u64) -> String {
    let separator = if back.contains('?') { '&' } else { '?' };
    format!("{back}{separator}{action}={id}#comment-{id}")
}

pub(super) fn comment_url(back: &str, id: u64) -> String {
    format!("{back}#comment-{id}")
}

pub(super) fn form_error(error: &str) -> String {
    if error.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='notice comment-form-error' role='alert'>{}</p>",
            escape(error)
        )
    }
}

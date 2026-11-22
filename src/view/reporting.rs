use std::collections::BTreeMap;
use std::fmt::Write;

use crate::model::{ContentKind, ReportReason, State};

use super::{action_fields, escape, field, visible_post};

pub(super) const COMMUNITY_HISTORY: &str = "/reports?view=community";

pub(super) fn content_url(kind: ContentKind, id: u64) -> String {
    format!("/report/content?kind={}&id={id}", kind.as_str())
}

pub(super) fn content_form(
    state: &State,
    values: &BTreeMap<String, String>,
    error: &str,
) -> Option<String> {
    let kind = ContentKind::parse(field(values, "kind"))?;
    let id = field(values, "id").parse::<u64>().ok()?;
    let (post, comment) = state.report_target(kind, id)?;
    let title = kind.as_str();
    let form_url = content_url(kind, id);
    let fields = action_fields(
        "report-content",
        &form_url,
        &[("kind", title), ("id", &id.to_string())],
    );
    let context = if let Some(comment) = comment {
        let quote: String = comment.body.chars().take(240).collect();
        let suffix = if quote.len() < comment.body.len() {
            "…"
        } else {
            ""
        };
        format!("<p class='muted'>Comment in “{}”</p><blockquote class='report-context'>{}{suffix}</blockquote>", escape(&post.title), escape(&quote))
    } else {
        format!("<p class='report-context'>{}</p>", escape(&post.title))
    };
    let back = thread_url(post.id, comment.map(|comment| comment.id));
    let error = if error.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='notice account-error' role='alert'>{}</p>",
            escape(error)
        )
    };
    let chosen = field(values, "reason");
    let mut options = String::from("<option value=''>Choose a reason</option>");
    for reason in ReportReason::ALL {
        let selected = if reason.as_str() == chosen {
            " selected"
        } else {
            ""
        };
        write!(
            options,
            "<option value='{}'{selected}>{}</option>",
            reason.as_str(),
            escape(reason.label())
        )
        .unwrap();
    }
    Some(format!(
        r#"<section class="panel standalone report-page"><h1>Report {title}</h1>{context}{error}
<form class="stack-form content-report-form" method="post" action="/action">{fields}
<label>Reason<select name="reason" required>{options}</select></label>
<label>Additional details <small>Optional</small><textarea name="details" maxlength="4000" rows="5">{details}</textarea></label>
<p class="muted">Community moderators review this report. You can track its status in your reports.</p>
<div class="form-actions"><a class="button light" href="{back}">Cancel</a><button class="button dark" type="submit">Submit report</button></div></form></section>"#,
        details = escape(field(values, "details")),
    ))
}

pub(super) fn history(state: &State, community: bool) -> String {
    let (title, legal_current, community_current) = if community {
        ("Community reports", "", " aria-current='page'")
    } else {
        ("Legal review log", " aria-current='page'", "")
    };
    let mut html = format!("<section class='panel standalone report-history'><h1>{title}</h1><nav class='account-notifications-nav' aria-label='Report history'><a href='{COMMUNITY_HISTORY}'{community_current}>Community reports</a><a href='/reports'{legal_current}>Legal review log</a></nav>");
    let mut reports = state
        .reports
        .iter()
        .rev()
        .filter(|report| {
            !state.username.is_empty()
                && report.reporter == state.username
                && report.reason.is_some() == community
        })
        .peekable();
    if reports.peek().is_none() {
        let copy = if community {
            "Use the menu on a post or comment to report it to community moderators."
        } else {
            "Use a post’s menu to report a legal concern."
        };
        write!(
            html,
            "<div class='log-empty'><h2>No reports yet</h2><p>{copy}</p></div>"
        )
        .unwrap();
    }
    for report in reports {
        let kind = report.kind();
        let title = if let Some(comment_id) = report.comment {
            state.report_target(kind, comment_id).map_or_else(
                || "Comment unavailable".to_owned(),
                |(post, _)| {
                    format!(
                        "<a href='{}'>Comment in {}</a>",
                        thread_url(post.id, Some(comment_id)),
                        escape(&post.title)
                    )
                },
            )
        } else {
            visible_post(state, report.post).map_or_else(
                || "Discussion unavailable".to_owned(),
                |post| {
                    format!(
                        "<a href='{}'>{}</a>",
                        thread_url(post.id, None),
                        escape(&post.title)
                    )
                },
            )
        };
        let status = if report.reviewed {
            "Reviewed"
        } else {
            "Pending review"
        };
        let (type_label, type_value) = report.reason.map_or_else(
            || ("Region", escape(&report.region)),
            |reason| ("Reason", escape(reason.label())),
        );
        let details = if report.details.is_empty() {
            String::new()
        } else {
            format!(
                "<p class='account-message-body'>{}</p>",
                escape(&report.details)
            )
        };
        write!(html, "<article class='log-item report-record' id='report-{}'><span class='flair'>{status}</span><h2>{title}</h2><p><strong>{type_label}</strong> {type_value}</p>{details}</article>", report.id).unwrap();
    }
    html.push_str("</section>");
    html
}

fn thread_url(post: u64, comment: Option<u64>) -> String {
    match comment {
        Some(id) => format!("/thread?id={post}#comment-{id}"),
        None => format!("/thread?id={post}"),
    }
}

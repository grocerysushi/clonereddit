use std::collections::BTreeMap;
use std::fmt::Write;

use crate::model::{Appeal, AppealStatus, Community, ModeratorRole, State};
use crate::schedule_time;

use super::super::{account_byline, action_fields, action_form, community_owner, escape, field};
use super::{empty, tools_url};

type Draft<'a> = Option<(&'a BTreeMap<String, String>, &'a str)>;

pub(super) fn approved_members(
    state: &State,
    item: &Community,
    registered: &[String],
    draft: Draft<'_>,
) -> String {
    let back = tools_url(item, "approved-members");
    let mut members: Vec<_> = state
        .approved_members
        .iter()
        .filter(|member| member.community == item.slug)
        .collect();
    members.sort_by(|left, right| left.handle.cmp(&right.handle));

    let access = match item.visibility.as_str() {
        "private" => "Approved accounts can view, post, and comment here.",
        "restricted" => "Anyone can view this community. Approved accounts can post and comment.",
        _ => "This community is public. Approvals take effect if it becomes private or restricted.",
    };
    let mut suggestions = String::new();
    for handle in registered.iter().filter(|handle| {
        item.owner.as_deref() != Some(handle.as_str())
            && state.moderator_role(item, handle) != Some(ModeratorRole::Moderator)
            && !state.is_approved_member(&item.slug, handle)
    }) {
        write!(suggestions, "<option value='{}'></option>", escape(handle)).unwrap();
    }

    let handle = draft
        .filter(|(form, _)| field(form, "action") == "approve-member")
        .map_or("", |(form, _)| field(form, "handle"));
    let fields = action_fields("approve-member", &back, &[("community", &item.slug)]);
    let mut html = format!(
        "{}<section class='panel mod-section mod-access-add'><h2>Approve an account</h2><p id='mod-access-help' class='muted'>{access} Joining is a separate choice. Bans block posting, editing, and commenting. Mutes block mod mail.</p><form class='stack-form' action='/action' method='post'>{fields}<label>Username<input name='handle' value='{}' list='mod-access-usernames' minlength='2' maxlength='26' autocomplete='off' autocapitalize='none' spellcheck='false' aria-describedby='mod-access-help' required></label><datalist id='mod-access-usernames'>{suggestions}</datalist><button class='button dark' type='submit'>Approve member</button></form></section>",
        alert(draft), escape(handle)
    );

    html.push_str(
        "<section class='panel mod-section mod-access-roster'><h2>Approved accounts</h2>",
    );
    if members.is_empty() {
        html.push_str("<p class='muted'>No approved accounts yet.</p>");
    } else {
        html.push_str("<ul>");
        for member in members {
            let name = escape(&member.handle);
            let ban = if state.is_banned(&item.slug, &member.handle) {
                "<span class='flair'>Banned</span>"
            } else {
                ""
            };
            let mute = if state.is_mod_mail_muted(&item.slug, &member.handle) {
                "<span class='flair'>Mod mail muted</span>"
            } else {
                ""
            };
            let revoke = action_fields(
                "revoke-approved-member",
                &back,
                &[("community", &item.slug), ("handle", &member.handle)],
            );
            write!(html, "<li><div class='mod-access-account'><strong>u/{name}</strong>{ban}{mute}</div><form action='/action' method='post'>{revoke}<button type='submit' class='button light' aria-label='Revoke approval for u/{name}'>Revoke approval</button></form></li>").unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

pub(super) fn roles(
    state: &State,
    item: &Community,
    registered: &[String],
    draft: Draft<'_>,
) -> String {
    let owner = item.owner.as_deref().unwrap_or_default();
    let can_manage = community_owner(state, item);
    let back = tools_url(item, "roles");
    let mut staff: Vec<_> = state
        .moderator_roles
        .iter()
        .filter(|entry| entry.community == item.slug)
        .collect();
    staff.sort_by(|left, right| left.handle.cmp(&right.handle));

    let mut html = alert(draft);
    if can_manage {
        let mut suggestions = String::new();
        for handle in registered.iter().filter(|handle| {
            handle.as_str() != owner
                && !staff.iter().any(|entry| &entry.handle == *handle)
                && !state.is_banned(&item.slug, handle)
                && !state.is_mod_mail_muted(&item.slug, handle)
        }) {
            write!(suggestions, "<option value='{}'></option>", escape(handle)).unwrap();
        }
        let new_entry = draft.filter(|(form, _)| {
            field(form, "action") == "set-moderator-role"
                && !staff
                    .iter()
                    .any(|entry| entry.handle == field(form, "handle"))
        });
        let (handle, role) = new_entry.map_or(("", "moderator"), |(form, _)| {
            (field(form, "handle"), field(form, "role"))
        });
        let fields = action_fields("set-moderator-role", &back, &[("community", &item.slug)]);
        let options = role_options(role);
        write!(html, "<section class='panel mod-section mod-role-add'><h2>Add moderator</h2><form class='stack-form' action='/action' method='post'>{fields}<label>Username<input name='handle' value='{}' list='mod-role-usernames' minlength='2' maxlength='24' autocomplete='off' autocapitalize='none' spellcheck='false' required></label><datalist id='mod-role-usernames'>{suggestions}</datalist><label>Role<select name='role'>{options}</select></label><button class='button dark' type='submit'>Add moderator</button></form></section>", escape(handle)).unwrap();
    }
    write!(html, "<section class='panel mod-section mod-role-roster'><h2>Moderation team</h2><ul><li><div><strong>{}</strong><p class='muted'>Owner</p></div></li>", account_byline(owner)).unwrap();
    for entry in staff {
        write!(
            html,
            "<li><div><strong>u/{}</strong><p class='muted'>{}</p></div>",
            escape(&entry.handle),
            entry.role.label()
        )
        .unwrap();
        if can_manage {
            let target = [
                ("community", item.slug.as_str()),
                ("handle", entry.handle.as_str()),
            ];
            let select = draft
                .filter(|(form, _)| field(form, "handle") == entry.handle)
                .map_or(entry.role.as_str(), |(form, _)| field(form, "role"));
            let options = role_options(select);
            let fields = action_fields("set-moderator-role", &back, &target);
            let remove = action_form(
                "remove-moderator-role",
                &back,
                &target,
                "Remove access",
                "",
                "button light",
            );
            write!(html, "<div class='mod-role-actions'><form action='/action' method='post'>{fields}<label><span class='sr-only'>Role for u/{}</span><select name='role'>{options}</select></label><button class='button light' type='submit'>Save role</button></form>{remove}</div>", escape(&entry.handle)).unwrap();
        }
        html.push_str("</li>");
    }
    html.push_str("</ul></section><section class='panel mod-section mod-role-help'><h2>What each role can do</h2><dl><dt>Moderator</dt><dd>Remove and restore posts and comments, review reports and appeals, manage approved members, bans and mutes, reply to mod mail, and schedule posts and events.</dd><dt>Appeals reviewer</dt><dd>Review and decide appeals for this community.</dd><dt>Owner</dt><dd>Set community access and appearance, pin posts, and manage moderator roles.</dd></dl></section>");
    html
}

fn role_options(selected: &str) -> String {
    [ModeratorRole::Moderator, ModeratorRole::AppealsReviewer]
        .into_iter()
        .map(|role| option(role.as_str(), role.label(), selected))
        .collect()
}

pub(super) fn appeals(
    state: &State,
    item: &Community,
    query: &BTreeMap<String, String>,
    draft: Draft<'_>,
) -> String {
    let resolved = field(query, "status") == "resolved";
    let base = tools_url(item, "appeals");
    let pending_url = escape(&base);
    let resolved_url = escape(&format!("{base}&status=resolved"));
    let current = |selected| if selected { " aria-current='page'" } else { "" };
    let mut cases: Vec<_> = state
        .appeals
        .iter()
        .filter(|case| case.community == item.slug)
        .collect();
    let pending = cases
        .iter()
        .filter(|case| case.status == AppealStatus::Pending)
        .count();
    let completed = cases.len() - pending;
    let mut html = format!("{}<nav class='mod-queue-tabs mod-appeal-tabs' aria-label='Appeal status'><a href='{pending_url}'{}>Pending<span>{pending}</span></a><a href='{resolved_url}'{}>Resolved<span>{completed}</span></a></nav>", alert(draft), current(!resolved), current(resolved));

    cases.retain(|case| (case.status != AppealStatus::Pending) == resolved);
    cases.sort_by_key(|case| {
        std::cmp::Reverse((case.decided_at.unwrap_or(case.created_at), case.id))
    });
    if cases.is_empty() {
        let (title, text) = if resolved {
            (
                "No resolved appeals",
                "Decisions on appeals for this community will appear here.",
            )
        } else {
            (
                "No appeals to review",
                "Appeals of community bans and mod mail mutes will appear here.",
            )
        };
        html.push_str(&empty(title, text, "shield"));
    }
    for case in cases {
        let reason = state
            .restrictions
            .iter()
            .find(|restriction| {
                restriction.id == case.restriction_id && restriction.community == item.slug
            })
            .map_or("", |restriction| restriction.reason.as_str());
        render_appeal(&mut html, case, item, &base, reason, draft);
    }
    html
}

fn render_appeal(
    html: &mut String,
    case: &Appeal,
    item: &Community,
    back: &str,
    reason: &str,
    draft: Draft<'_>,
) {
    let kind = if case.kind == "ban" {
        "Community ban"
    } else {
        "Mod mail mute"
    };
    let id = case.id.to_string();
    let restriction = case.restriction_id;
    write!(html, "<article class='panel mod-record mod-appeal-record'><header><span class='flair'>{}</span><span class='muted'>Appeal #{id}</span></header><h2>{kind}</h2><p class='muted'>u/{} · Restriction #{restriction} · {}</p><p class='mod-record-detail'>{}</p>", case.status.label(), escape(&case.appellant), time(case.created_at), escape(&case.body)).unwrap();
    if !reason.is_empty() {
        write!(html, "<blockquote class='moderation-reply'><strong>Moderator’s restriction reason</strong><p class='mod-record-detail'>{}</p></blockquote>", escape(reason)).unwrap();
    }
    if case.status == AppealStatus::Pending {
        let fields = action_fields(
            "decide-appeal",
            back,
            &[("community", &item.slug), ("appeal", &id)],
        );
        let case_draft = draft.filter(|(form, _)| field(form, "appeal") == id);
        let (decision, response, opened) = case_draft.map_or(("", "", ""), |(form, _)| {
            (field(form, "decision"), field(form, "response"), " open")
        });
        let options = option("", "Choose a decision", decision)
            + &option("approve", "Approve and lift this restriction", decision)
            + &option("deny", "Deny appeal", decision);
        write!(html, "<details class='mod-appeal-decision'{opened}><summary class='button light'>Review appeal</summary><form class='stack-form' action='/action' method='post'>{fields}<label>Decision<select name='decision' required>{options}</select></label><label>Response to appellant<textarea name='response' rows='4' maxlength='4000' required>{}</textarea></label><button class='button dark' type='submit'>Save decision</button></form></details>", escape(response)).unwrap();
    } else {
        let reviewer = case
            .reviewer
            .as_deref()
            .map_or_else(String::new, |handle| format!(" by u/{}", escape(handle)));
        let decided = case
            .decided_at
            .map_or_else(String::new, |timestamp| format!(" · {}", time(timestamp)));
        write!(html, "<blockquote class='moderation-reply'><strong>{}{reviewer}</strong><small class='muted'>{decided}</small><p class='mod-record-detail'>{}</p></blockquote>", case.status.label(), escape(&case.response)).unwrap();
    }
    html.push_str("</article>");
}

fn alert(draft: Draft<'_>) -> String {
    draft.map_or_else(String::new, |(_, error)| {
        format!(
            "<p class='notice setup-error' role='alert'>{}</p>",
            escape(error)
        )
    })
}

fn option(value: &str, label: &str, current: &str) -> String {
    let selected = if value == current { " selected" } else { "" };
    format!("<option value='{value}'{selected}>{label}</option>")
}

fn time(timestamp: u64) -> String {
    format!(
        "<time datetime='{}Z'>{}</time>",
        schedule_time::to_local(timestamp, "UTC"),
        escape(&schedule_time::display(timestamp, "America/Chicago"))
    )
}

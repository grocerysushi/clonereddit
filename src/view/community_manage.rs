use std::collections::BTreeMap;
use std::fmt::Write;

use crate::model::{
    Appeal, AppealStatus, Community, ContentAppeal, ContentKind, ModPermission, ModeratorRole,
    State,
};
use crate::schedule_time;

use super::super::{account_byline, action_fields, community_owner, encode, escape, field, hidden};
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
        let pending = state
            .outgoing_moderator_invitations(item)
            .collect::<Vec<_>>();
        let mut suggestions = String::new();
        for handle in registered.iter().filter(|handle| {
            handle.as_str() != owner
                && !staff.iter().any(|entry| &entry.handle == *handle)
                && !pending.iter().any(|entry| &entry.handle == *handle)
                && !state.is_banned(&item.slug, handle)
                && !state.is_mod_mail_muted(&item.slug, handle)
        }) {
            write!(suggestions, "<option value='{}'></option>", escape(handle)).unwrap();
        }
        let invite_form = draft
            .filter(|(form, _)| field(form, "action") == "invite-moderator")
            .map(|(form, _)| form);
        let handle = invite_form.map_or("", |form| field(form, "handle"));
        let fields = action_fields("invite-moderator", &back, &[("community", &item.slug)]);
        let options = permission_options(None, invite_form, "Permissions");
        write!(html, "<section class='panel mod-section mod-role-add'><h2>Invite moderator</h2><p>Access starts after the invited member accepts.</p><form class='stack-form' action='/action' method='post'>{fields}<label>Username<input name='handle' value='{}' list='mod-role-usernames' minlength='2' maxlength='26' autocomplete='off' autocapitalize='none' spellcheck='false' required></label><datalist id='mod-role-usernames'>{suggestions}</datalist>{options}<button class='button dark' type='submit'>Send invitation</button></form></section>", escape(handle)).unwrap();
        html.push_str(
            "<section class='panel mod-section mod-role-roster'><h2>Pending invitations</h2>",
        );
        if pending.is_empty() {
            html.push_str("<p class='muted'>No pending invitations.</p>");
        } else {
            html.push_str("<ul>");
            for invite in pending {
                let id = invite.id.to_string();
                let name = escape(&invite.handle);
                let permissions = super::super::moderator_invitations::permission_list(invite.role);
                let fields = action_fields(
                    "cancel-moderator-invitation",
                    &back,
                    &[("community", &item.slug), ("id", &id)],
                );
                write!(html, "<li><div><strong>u/{name}</strong><p class='muted'>{permissions}</p></div><form action='/action' method='post'>{fields}<button class='button light' type='submit' aria-label='Cancel invitation for u/{name}'>Cancel invitation</button></form></li>").unwrap();
            }
            html.push_str("</ul>");
        }
        html.push_str("</section>");
    }
    write!(html, "<section class='panel mod-section mod-role-roster'><h2>Moderation team</h2><ul><li><div><strong>{}</strong><p class='muted'>Owner</p></div></li>", account_byline(owner)).unwrap();
    for entry in staff {
        let permission_list = super::super::moderator_invitations::permission_list(entry.role);
        write!(
            html,
            "<li><div><strong>u/{}</strong><p class='muted'>{permission_list}</p></div>",
            escape(&entry.handle),
        )
        .unwrap();
        if can_manage {
            let target = [
                ("community", item.slug.as_str()),
                ("handle", entry.handle.as_str()),
            ];
            let rejected = draft
                .filter(|(form, _)| {
                    field(form, "action") == "set-moderator-permissions"
                        && field(form, "handle") == entry.handle
                })
                .map(|(form, _)| form);
            let handle = escape(&entry.handle);
            let options = permission_options(
                Some(entry.role),
                rejected,
                &format!("Permissions for u/{handle}"),
            );
            let fields = action_fields("set-moderator-permissions", &back, &target);
            let remove = action_fields("remove-moderator-role", &back, &target);
            let open = if rejected.is_some() { " open" } else { "" };
            write!(html, "<details class='preference-list'{open}><summary>Edit permissions <span class='sr-only'>for u/{handle}</span></summary><form class='stack-form' action='/action' method='post'>{fields}{options}<button class='button dark' type='submit'>Save permissions</button></form><form action='/action' method='post'>{remove}<button class='button light' type='submit' aria-label='Remove access for u/{handle}'>Remove access</button></form></details>").unwrap();
        }
        html.push_str("</li>");
    }
    html.push_str("</ul></section>");
    html
}

fn permission_options(
    role: Option<ModeratorRole>,
    form: Option<&BTreeMap<String, String>>,
    legend: &str,
) -> String {
    let mut html = format!("<fieldset><legend>{legend}</legend>");
    for permission in ModPermission::ALL {
        let name = format!("permission-{}", permission.code());
        let checked = if form.map_or(role.is_some_and(|role| role.allows(permission)), |form| {
            field(form, &name) == "on"
        }) {
            " checked"
        } else {
            ""
        };
        write!(
            html,
            "<label class='checkbox'><input type='checkbox' name='{name}'{checked}> {}</label>",
            permission.label()
        )
        .unwrap();
    }
    html.push_str("</fieldset>");
    html
}

const APPEAL_TYPES: &[(&str, &str)] = &[
    ("all", "All types"),
    ("post", "Post removal"),
    ("comment", "Comment removal"),
    ("ban", "Ban"),
    ("mute", "Mute"),
];

const APPEAL_OUTCOMES: &[(&str, &str)] = &[
    ("all", "All decisions"),
    ("approved", "Approved"),
    ("denied", "Denied"),
    ("closed", "Closed"),
];

#[derive(Clone, Copy)]
struct AppealFilters<'a> {
    kind: &'static str,
    resolved: bool,
    outcome: &'static str,
    search: &'a str,
}

impl<'a> AppealFilters<'a> {
    fn read(query: &'a BTreeMap<String, String>) -> Self {
        let resolved = field(query, "status") == "resolved";
        let outcome = if resolved {
            appeal_filter_key(APPEAL_OUTCOMES, field(query, "outcome"))
        } else {
            "all"
        };
        Self {
            kind: appeal_filter_key(APPEAL_TYPES, field(query, "kind")),
            resolved,
            outcome,
            search: field(query, "appeal_q").trim(),
        }
    }

    fn matches(self, kind: &str) -> bool {
        self.kind == "all" || self.kind == kind
    }

    fn matches_resolved(self, status: AppealStatus) -> bool {
        match self.outcome {
            "approved" => status == AppealStatus::Approved,
            "denied" => status == AppealStatus::Denied,
            "closed" => matches!(
                status,
                AppealStatus::Lifted
                    | AppealStatus::Expired
                    | AppealStatus::Restored
                    | AppealStatus::Unavailable
            ),
            _ => status != AppealStatus::Pending,
        }
    }

    fn url(self, item: &Community) -> String {
        let mut url = tools_url(item, "appeals");
        if self.resolved {
            url.push_str("&status=resolved");
        }
        if self.kind != "all" {
            write!(url, "&kind={}", self.kind).unwrap();
        }
        if self.resolved && self.outcome != "all" {
            write!(url, "&outcome={}", self.outcome).unwrap();
        }
        if !self.search.is_empty() {
            write!(url, "&appeal_q={}", encode(self.search)).unwrap();
        }
        url
    }

    fn form(self, item: &Community) -> String {
        let options: String = APPEAL_TYPES
            .iter()
            .map(|&(kind, label)| option(kind, label, self.kind))
            .collect();
        let decision = if self.resolved {
            let decisions: String = APPEAL_OUTCOMES
                .iter()
                .map(|&(outcome, label)| option(outcome, label, self.outcome))
                .collect();
            format!("<label>Decision<select name='outcome'>{decisions}</select></label>")
        } else {
            String::new()
        };
        let clear = if self.search.is_empty() {
            String::new()
        } else {
            let url = AppealFilters { search: "", ..self }.url(item);
            format!(
                "<a class='button light mod-appeal-clear' href='{}'>Clear search</a>",
                escape(&url)
            )
        };
        let status = if self.resolved { "resolved" } else { "pending" };
        format!("<form class='mod-queue-sort mod-appeal-filters' action='/moderation/tools' method='get'>{}{}{}<label class='mod-appeal-search'>Search appeals<input type='search' name='appeal_q' value='{}' placeholder='Username or appeal ID' autocapitalize='none' spellcheck='false'></label><label>Appeal type<select name='kind'>{options}</select></label>{decision}<button type='submit' class='button light'>Apply filters</button>{clear}</form>",
            hidden("community", &item.slug), hidden("section", "appeals"), hidden("status", status), escape(self.search))
    }
}

enum AppealSearch {
    All,
    Id(u64),
    Username(String),
    NoMatch,
}

impl AppealSearch {
    fn read(query: &str) -> Self {
        if query.is_empty() {
            return Self::All;
        }
        if let Some(username) = query
            .strip_prefix('@')
            .or_else(|| query.strip_prefix("u/"))
            .or_else(|| query.strip_prefix("U/"))
        {
            let username = username.trim();
            return if username.is_empty() {
                Self::NoMatch
            } else {
                Self::Username(username.to_ascii_lowercase())
            };
        }
        if query.starts_with('#') || query.bytes().all(|byte| byte.is_ascii_digit()) {
            let digits = query.strip_prefix('#').unwrap_or(query).trim();
            return if digits.bytes().all(|byte| byte.is_ascii_digit()) {
                digits.parse().map_or(Self::NoMatch, Self::Id)
            } else {
                Self::NoMatch
            };
        }
        Self::Username(query.to_ascii_lowercase())
    }

    fn matches(&self, id: u64, username: &str) -> bool {
        match self {
            Self::All => true,
            Self::Id(expected) => id == *expected,
            Self::Username(expected) => username.to_ascii_lowercase().contains(expected),
            Self::NoMatch => false,
        }
    }
}

fn appeal_filter_key(options: &[(&'static str, &str)], requested: &str) -> &'static str {
    options
        .iter()
        .find_map(|&(key, _)| (key == requested).then_some(key))
        .unwrap_or("all")
}

pub(super) fn appeal_return_url(item: &Community, form: &BTreeMap<String, String>) -> String {
    AppealFilters::read(form).url(item)
}

pub(super) fn appeals(
    state: &State,
    item: &Community,
    query: &BTreeMap<String, String>,
    draft: Draft<'_>,
) -> String {
    enum Case<'a> {
        Restriction(&'a Appeal),
        Removal(&'a ContentAppeal),
    }
    let filters = AppealFilters::read(query);
    let search = AppealSearch::read(filters.search);
    let resolved = filters.resolved;
    let pending_url = escape(
        &AppealFilters {
            resolved: false,
            ..filters
        }
        .url(item),
    );
    let resolved_url = escape(
        &AppealFilters {
            resolved: true,
            ..filters
        }
        .url(item),
    );
    let current = |selected| if selected { " aria-current='page'" } else { "" };
    let mut cases: Vec<_> = state
        .appeals
        .iter()
        .filter(|case| {
            case.community == item.slug
                && filters.matches(&case.kind)
                && search.matches(case.id, &case.appellant)
        })
        .map(|case| {
            (
                case.status,
                (case.decided_at.unwrap_or(case.created_at), case.id),
                Case::Restriction(case),
            )
        })
        .chain(
            state
                .content_appeals
                .iter()
                .filter(|case| {
                    case.community == item.slug
                        && filters.matches(case.kind.as_str())
                        && search.matches(case.id, &case.appellant)
                })
                .map(|case| {
                    (
                        case.status,
                        (case.decided_at.unwrap_or(case.created_at), case.id),
                        Case::Removal(case),
                    )
                }),
        )
        .collect();
    let pending = cases
        .iter()
        .filter(|(status, _, _)| *status == AppealStatus::Pending)
        .count();
    let completed = cases
        .iter()
        .filter(|(status, _, _)| filters.matches_resolved(*status))
        .count();
    let mut html = format!("{}<div class='mod-queue-controls'><nav class='mod-queue-tabs mod-appeal-tabs' aria-label='Appeal status'><a href='{pending_url}'{}>Pending<span>{pending}</span></a><a href='{resolved_url}'{}>Resolved<span>{completed}</span></a></nav>{}</div>", alert(draft), current(!resolved), current(resolved), filters.form(item));

    cases.retain(|(status, _, _)| {
        if resolved {
            filters.matches_resolved(*status)
        } else {
            *status == AppealStatus::Pending
        }
    });
    cases.sort_by_key(|(_, key, _)| std::cmp::Reverse(*key));
    if cases.is_empty() {
        let title = if resolved {
            "No resolved appeals"
        } else {
            "No appeals to review"
        };
        let text = if !filters.search.is_empty() {
            "No appeals match this search and the selected filters."
        } else if filters.outcome != "all" {
            "No appeals match this decision and appeal type."
        } else if filters.kind != "all" {
            "No appeals match this type and status."
        } else if resolved {
            "Decisions on appeals for this community will appear here."
        } else {
            "Appeals of content removals, community bans, and mod mail mutes will appear here."
        };
        html.push_str(&empty(title, text, "shield"));
    }
    for (_, _, case) in cases {
        match case {
            Case::Restriction(case) => {
                let reason = state
                    .restrictions
                    .iter()
                    .find(|restriction| {
                        restriction.id == case.restriction_id && restriction.community == item.slug
                    })
                    .map_or("", |restriction| restriction.reason.as_str());
                render_appeal(&mut html, case, item, filters, reason, draft);
            }
            Case::Removal(case) => {
                render_content_appeal(&mut html, state, case, item, filters, draft)
            }
        }
    }
    html
}

fn render_appeal(
    html: &mut String,
    case: &Appeal,
    item: &Community,
    filters: AppealFilters<'_>,
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
        decision_form(html, item, filters, &id, None, draft);
    } else {
        render_decision(
            html,
            case.status,
            case.reviewer.as_deref(),
            case.decided_at,
            &case.response,
        );
    }
    html.push_str("</article>");
}

fn render_content_appeal(
    html: &mut String,
    state: &State,
    case: &ContentAppeal,
    item: &Community,
    filters: AppealFilters<'_>,
    draft: Draft<'_>,
) {
    let id = case.id.to_string();
    let label = case.kind.label();
    write!(html, "<article class='panel mod-record mod-appeal-record mod-content-appeal' data-appeal-id='{id}'><header><span class='flair'>{}</span><span class='muted'>Appeal #{id}</span></header><h2>{label} removal</h2><p class='muted'>u/{} · {label} #{} · {}</p><p class='mod-record-detail'>{}</p><blockquote class='moderation-reply'><strong>Original removal reason</strong><p class='mod-record-detail'>{}</p></blockquote>", case.status.label(), escape(&case.appellant), case.content_id, time(case.created_at), escape(&case.body), escape(&case.reason)).unwrap();
    if case.status == AppealStatus::Pending {
        if let Some(post) = state
            .posts
            .iter()
            .find(|post| post.id == case.post_id && post.community == item.slug)
        {
            match case.kind {
                ContentKind::Post => {
                    write!(html, "<details class='moderation-reply'><summary>Removed post</summary><p class='mod-record-detail'><strong>{}</strong></p><p class='mod-record-detail'>{}</p></details>", escape(&post.title), escape(&post.body)).unwrap();
                }
                ContentKind::Comment => {
                    if let Some(comment) = state.comments.iter().find(|comment| {
                        comment.id == case.content_id && comment.post == post.id && !comment.deleted
                    }) {
                        write!(html, "<details class='moderation-reply'><summary>Removed comment</summary><p class='mod-record-detail'>{}</p></details>", escape(&comment.body)).unwrap();
                    }
                    if state.is_post_removed(post.id) {
                        html.push_str("<p class='muted'>This comment’s post is also removed.</p>");
                    }
                }
            }
        }
        decision_form(html, item, filters, &id, Some(case.kind), draft);
    } else {
        render_decision(
            html,
            case.status,
            case.reviewer.as_deref(),
            case.decided_at,
            &case.response,
        );
    }
    html.push_str("</article>");
}

fn decision_form(
    html: &mut String,
    item: &Community,
    filters: AppealFilters<'_>,
    id: &str,
    content: Option<ContentKind>,
    draft: Draft<'_>,
) {
    let (action, approve, label) = match content {
        Some(ContentKind::Post) => (
            "decide-content-appeal",
            "Approve and restore this post",
            "Decision reason",
        ),
        Some(ContentKind::Comment) => (
            "decide-content-appeal",
            "Approve and restore this comment",
            "Decision reason",
        ),
        None => (
            "decide-appeal",
            "Approve and lift this restriction",
            "Response to appellant",
        ),
    };
    let fields = action_fields(
        action,
        &filters.url(item),
        &[
            ("community", &item.slug),
            ("appeal", id),
            ("kind", filters.kind),
            ("appeal_q", filters.search),
        ],
    );
    let case_draft =
        draft.filter(|(form, _)| field(form, "appeal") == id && field(form, "action") == action);
    let (decision, response, opened) = case_draft.map_or(("", "", ""), |(form, _)| {
        (field(form, "decision"), field(form, "response"), " open")
    });
    let options = option("", "Choose a decision", decision)
        + &option("approve", approve, decision)
        + &option("deny", "Deny appeal", decision);
    write!(html, "<details class='mod-appeal-decision'{opened}><summary class='button light'>Review appeal</summary><form class='stack-form' action='/action' method='post'>{fields}<label>Decision<select name='decision' required>{options}</select></label><label>{label}<textarea name='response' rows='4' maxlength='4000' required>{}</textarea></label><button class='button dark' type='submit'>Save decision</button></form></details>", escape(response)).unwrap();
}

fn render_decision(
    html: &mut String,
    status: AppealStatus,
    reviewer: Option<&str>,
    decided: Option<u64>,
    response: &str,
) {
    let reviewer = reviewer.map_or_else(String::new, |handle| format!(" by u/{}", escape(handle)));
    let decided = decided.map_or_else(String::new, |timestamp| format!(" · {}", time(timestamp)));
    write!(html, "<blockquote class='moderation-reply'><strong>{}{reviewer}</strong><small class='muted'>{decided}</small><p class='mod-record-detail'>{}</p></blockquote>", status.label(), escape(response)).unwrap();
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

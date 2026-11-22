use std::{
    collections::BTreeMap,
    fmt::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::model::{Community, ContentKind, ModPermission, Post, Report, State};

use super::{
    account_byline, action_fields, action_form, can_view_post, community_icon, community_owner,
    community_url, display_post_title, encode, escape, field, hidden, icon, score,
};
use super::{modmail, restricted, scheduled, wiki};

#[path = "automod.rs"]
mod automod;
#[path = "community_manage.rs"]
mod community_manage;
#[path = "community_rules.rs"]
mod community_rules;
#[path = "moderate_content.rs"]
mod moderate_content;
#[path = "post_flair_tools.rs"]
mod post_flair_tools;

const SCHEDULED_TITLE: &str = "Scheduled posts and events";
const RESTRICTED_TITLE: &str = "Restricted Users";
const CONTENT_TITLE: &str = "Posts & comments";
const HIGHLIGHTS_TITLE: &str = "Community Highlights";
const AUTOMOD_TITLE: &str = "AutoMod";
const RULES_TITLE: &str = "Rules";
const POST_FLAIR_TITLE: &str = "Post flair";

const TOOLS: &[(&str, &str, &str, &str)] = &[
    (
        "queue",
        "Queue",
        "shield",
        "Review reports and AutoMod flags.",
    ),
    (
        "mail",
        "Mod mail",
        "mail",
        "Read and reply to messages sent to this community.",
    ),
    (
        "scheduled",
        SCHEDULED_TITLE,
        "calendar",
        "Schedule posts and events, and edit or cancel pending items.",
    ),
    (
        "restricted-users",
        RESTRICTED_TITLE,
        "shield",
        "Manage community bans and mod mail mutes.",
    ),
    (
        "appeals",
        "Appeals",
        "log",
        "Review requests to lift community bans and mod mail mutes.",
    ),
    (
        "content",
        CONTENT_TITLE,
        "chat",
        "Review, remove, and restore community posts and comments.",
    ),
    (
        "automod",
        AUTOMOD_TITLE,
        "sliders",
        "Configure rules that flag or remove posts and comments.",
    ),
    (
        "discussions",
        "Discussions",
        "chat",
        "Browse discussions in this community.",
    ),
    (
        "highlights",
        HIGHLIGHTS_TITLE,
        "pin",
        "Pin posts and set their order on the community feed.",
    ),
    (
        "rules",
        RULES_TITLE,
        "log",
        "Manage reporting rules and view topic guidelines.",
    ),
    (
        "wiki",
        wiki::TITLE,
        "book",
        "Create and publish wiki pages and manage their editors.",
    ),
    (
        "post-flair",
        POST_FLAIR_TITLE,
        "tag",
        "Manage post flair and choose whether it is required.",
    ),
    (
        "members",
        "Members",
        "grid",
        "See ownership, membership, and access.",
    ),
    (
        "approved-members",
        "Approved members",
        "check",
        "Manage access to private and restricted communities.",
    ),
    (
        "roles",
        "Moderator roles",
        "shield",
        "View and manage this community’s moderation team.",
    ),
    (
        "insights",
        "Insights",
        "trend",
        "View activity and review totals.",
    ),
    (
        "log",
        "Mod log",
        "log",
        "See recorded moderation actions and when they happened.",
    ),
    (
        "responses",
        "Saved responses",
        "save",
        "Manage reusable replies for mod mail.",
    ),
];

pub(super) fn can_view(state: &State, item: &Community, section: &str) -> bool {
    let permission = match section {
        "" => return state.can_open_mod_tools(item),
        "queue" => {
            return state.can_moderate(item, ModPermission::Reports)
                || state.can_moderate(item, ModPermission::Content);
        }
        "discussions" | "roles" | "insights" | "log" => ModPermission::Reports,
        "members" | "approved-members" => ModPermission::Members,
        "mail" | "responses" => ModPermission::Mail,
        "scheduled" => ModPermission::Schedule,
        "restricted-users" => ModPermission::Restrictions,
        "appeals" => ModPermission::Appeals,
        "content" | "automod" | "highlights" | "rules" | "post-flair" | "wiki" => {
            ModPermission::Content
        }
        _ => return false,
    };
    state.can_moderate(item, permission)
}

pub(super) fn render(
    state: &State,
    item: &Community,
    query: &BTreeMap<String, String>,
    registered: &[String],
) -> String {
    let workspace = Workspace::new(state, item);
    let available = || {
        TOOLS
            .iter()
            .copied()
            .filter(|(key, _, _, _)| can_view(state, item, key))
    };
    let (section, title, _, _) = TOOLS
        .iter()
        .copied()
        .find(|(key, _, _, _)| *key == field(query, "section") && can_view(state, item, key))
        .or_else(|| available().next())
        .unwrap_or(("", "Mod tools", "shield", ""));
    let search = field(query, "tool_q").trim();
    let body = if search.is_empty() {
        match section {
            "mail" => modmail::render(state, Some(item), query),
            "scheduled" => scheduled::render(state, item, query),
            "restricted-users" => restricted::render(state, item),
            "appeals" => community_manage::appeals(state, item, query, None),
            "content" => moderate_content::render(state, item, &workspace.posts, query, None),
            "automod" => automod::rules(state, item, field(query, "edit"), None),
            "discussions" => workspace.discussions(state),
            "highlights" => workspace.highlights(state),
            "rules" => workspace.rules(state, field(query, "edit"), field(query, "delete"), None),
            "wiki" => wiki::tools(state, item, query, None),
            "post-flair" => {
                post_flair_tools::render(item, field(query, "edit"), field(query, "delete"), None)
            }
            "members" => workspace.members(state),
            "approved-members" => community_manage::approved_members(state, item, registered, None),
            "roles" => community_manage::roles(state, item, registered, None),
            "insights" => workspace.insights(state),
            "log" => workspace.log(state),
            "responses" => workspace.responses(state),
            "queue" => workspace.queue(state, query),
            _ => empty(
                "Mod tools unavailable",
                "Your account needs a moderator role to view these tools.",
                "shield",
            ),
        }
    } else {
        search_results(state, item, search)
    };
    let heading = if search.is_empty() {
        title
    } else {
        "Search tools"
    };
    workspace.frame(state, section, heading, search, &body)
}

pub(super) fn automod_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let workspace = Workspace::new(state, item);
    if field(form, "action") == "dismiss-automod-flag" {
        let filters =
            QueueFilters::flag_return(form, state.can_moderate(item, ModPermission::Reports));
        let body = automod::error_notice(error) + &workspace.queue_filtered(state, filters);
        workspace.frame(state, "queue", "Queue", "", &body)
    } else {
        let body = automod::rules(state, item, field(form, "id"), Some((form, error)));
        workspace.frame(state, "automod", AUTOMOD_TITLE, "", &body)
    }
}

pub(super) fn wiki_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let new_page = BTreeMap::new();
    let query = if field(form, "action") == "create-wiki-page" {
        &new_page
    } else {
        form
    };
    let body = wiki::tools(state, item, query, Some((form, error)));
    Workspace::new(state, item).frame(state, "wiki", wiki::TITLE, "", &body)
}

pub(super) fn automod_return_url(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
) -> String {
    QueueFilters::flag_return(form, state.can_moderate(item, ModPermission::Reports)).url(item)
}

pub(super) fn content_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let workspace = Workspace::new(state, item);
    let body = moderate_content::render(state, item, &workspace.posts, form, Some(error));
    workspace.frame(state, "content", CONTENT_TITLE, "", &body)
}

pub(super) fn highlights_error(state: &State, item: &Community, error: &str) -> String {
    let workspace = Workspace::new(state, item);
    let body = format!(
        "<p class='notice setup-error' role='alert'>{}</p>{}",
        escape(error),
        workspace.highlights(state)
    );
    workspace.frame(state, "highlights", HIGHLIGHTS_TITLE, "", &body)
}

pub(super) fn rules_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let workspace = Workspace::new(state, item);
    let edit = if field(form, "action") == "save-community-rule" {
        field(form, "id")
    } else {
        ""
    };
    let body = workspace.rules(state, edit, "", Some((form, error)));
    workspace.frame(state, "rules", RULES_TITLE, "", &body)
}

pub(super) fn post_flair_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let (edit, delete) = match field(form, "action") {
        "save-post-flair" => (field(form, "id"), ""),
        "delete-post-flair" => ("", field(form, "id")),
        _ => ("", ""),
    };
    let body = post_flair_tools::render(item, edit, delete, Some((form, error)));
    Workspace::new(state, item).frame(state, "post-flair", POST_FLAIR_TITLE, "", &body)
}

pub(super) fn role_error(
    state: &State,
    item: &Community,
    registered: &[String],
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let body = community_manage::roles(state, item, registered, Some((form, error)));
    Workspace::new(state, item).frame(state, "roles", "Moderator roles", "", &body)
}

pub(super) fn approved_member_error(
    state: &State,
    item: &Community,
    registered: &[String],
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let body = community_manage::approved_members(state, item, registered, Some((form, error)));
    Workspace::new(state, item).frame(state, "approved-members", "Approved members", "", &body)
}

pub(super) fn decision_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let body = community_manage::appeals(state, item, form, Some((form, error)));
    Workspace::new(state, item).frame(state, "appeals", "Appeals", "", &body)
}

pub(super) fn appeal_return_url(item: &Community, form: &BTreeMap<String, String>) -> String {
    community_manage::appeal_return_url(item, form)
}

pub(super) fn schedule_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let body = scheduled::render_error(state, item, form, error);
    Workspace::new(state, item).frame(state, "scheduled", SCHEDULED_TITLE, "", &body)
}

pub(super) fn restriction_error(
    state: &State,
    item: &Community,
    form: &BTreeMap<String, String>,
    error: &str,
) -> String {
    let body = restricted::render_error(state, item, form, error);
    Workspace::new(state, item).frame(state, "restricted-users", RESTRICTED_TITLE, "", &body)
}

fn tools_url(item: &Community, section: &str) -> String {
    format!(
        "/moderation/tools?community={}&section={section}",
        encode(&item.slug)
    )
}

fn settings_url(item: &Community) -> String {
    format!("/communities/settings?community={}", encode(&item.slug))
}

fn navigation(state: &State, item: &Community, section: &str, search: &str) -> String {
    let community = hidden("community", &item.slug);
    let section_field = hidden("section", section);
    let mut options = String::new();
    for owned in state
        .communities
        .iter()
        .filter(|group| state.can_open_mod_tools(group))
    {
        let selected = if owned.slug == item.slug {
            " selected"
        } else {
            ""
        };
        write!(
            options,
            "<option value='{}'{selected}>{}</option>",
            escape(&owned.slug),
            escape(&owned.name)
        )
        .unwrap();
    }
    let mut links = String::new();
    for (key, label, glyph, _) in TOOLS {
        if !can_view(state, item, key) {
            continue;
        }
        let active = if *key == section && search.is_empty() {
            " aria-current='page'"
        } else {
            ""
        };
        write!(
            links,
            "<a href='{}'{active}>{}<span>{label}</span></a>",
            escape(&tools_url(item, key)),
            icon(glyph)
        )
        .unwrap();
    }
    let brand_content = format!(
        "{}<span>{}<small>Mod tools</small></span>",
        if state.can_view_community(item) {
            community_icon(item)
        } else {
            icon("shield")
        },
        escape(&item.name),
    );
    let brand = if state.can_view_community(item) {
        format!(
            "<a class='mod-community-brand' href='{}'>{brand_content}</a>",
            community_url(&item.slug)
        )
    } else {
        format!("<div class='mod-community-brand'>{brand_content}</div>")
    };
    let settings = if community_owner(state, item) {
        format!(
            "<a href='{}'>{}<span>Community settings</span></a>",
            escape(&settings_url(item)),
            icon("sliders")
        )
    } else {
        String::new()
    };
    format!(
        r#"<aside class="mod-tools-nav" aria-label="Moderation tools">
{brand}
<form class="mod-scope" action="/moderation/tools" method="get"><label>Community<select name="community">{options}</select></label><button type="submit" class="button light">Switch</button></form>
<form class="mod-tool-search" action="/moderation/tools" method="get" role="search">{community}{section_field}<label class="sr-only" for="mod-tool-search">Search moderation tools</label><input id="mod-tool-search" name="tool_q" type="search" maxlength="80" value="{search}" placeholder="Search tools"><button type="submit" class="button light" aria-label="Search moderation tools">{search_icon}</button></form>
<nav aria-label="Mod tools">{links}</nav><nav class="mod-management-links" aria-label="Community management">{settings}<a href="/moderation?section=manage">{grid}<span>Manage communities</span></a></nav>
</aside>"#,
        search = escape(search),
        search_icon = icon("search"),
        grid = icon("grid"),
    )
}

fn search_results(state: &State, item: &Community, search: &str) -> String {
    let needle = search.to_lowercase();
    let mut html = format!(
        "<section class='panel mod-section'><h2>Results for “{}”</h2><ul class='mod-tool-results'>",
        escape(search)
    );
    let mut any = false;
    for (key, label, glyph, description) in TOOLS {
        if can_view(state, item, key)
            && [*label, *description]
                .iter()
                .any(|text| text.to_lowercase().contains(&needle))
        {
            any = true;
            write!(html, "<li><a href='{}'>{}<span><strong>{label}</strong><small>{description}</small></span></a></li>", escape(&tools_url(item, key)), icon(glyph)).unwrap();
        }
    }
    html.push_str("</ul>");
    if !any {
        html.push_str("<p class='muted'>No tools match that search.</p>");
    }
    html.push_str("</section>");
    html
}

#[derive(Clone, Copy)]
pub(super) struct QueueKind(Option<ContentKind>);

impl QueueKind {
    pub(super) fn parse(value: &str) -> Self {
        Self(match value {
            "post" => Some(ContentKind::Post),
            "comment" => Some(ContentKind::Comment),
            _ => None,
        })
    }

    pub(super) fn as_str(self) -> &'static str {
        self.0.map_or("all", ContentKind::as_str)
    }

    pub(super) fn allows(self, kind: ContentKind) -> bool {
        self.0.is_none_or(|selected| selected == kind)
    }

    pub(super) fn select(self) -> String {
        let mut options = String::new();
        for (value, label) in [
            ("all", "All content"),
            ("post", "Posts"),
            ("comment", "Comments"),
        ] {
            let selected = if self.as_str() == value {
                " selected"
            } else {
                ""
            };
            write!(
                options,
                "<option value='{value}'{selected}>{label}</option>"
            )
            .unwrap();
        }
        format!("<label>Content type<select name='kind'>{options}</select></label>")
    }
}

#[derive(Clone, Copy)]
struct QueueFilters {
    status: &'static str,
    sort: &'static str,
    kind: QueueKind,
}

impl QueueFilters {
    fn read(values: &BTreeMap<String, String>, automatic: bool) -> Self {
        let status = match field(values, "status") {
            "reported" => "reported",
            "reviewed" => "reviewed",
            "automod" if automatic => "automod",
            _ => "needs-review",
        };
        let sort = if field(values, "sort") == "oldest" {
            "oldest"
        } else {
            "newest"
        };
        Self {
            status,
            sort,
            kind: QueueKind::parse(field(values, "kind")),
        }
    }

    fn flag_return(form: &BTreeMap<String, String>, reports: bool) -> Self {
        let mut filters = Self::read(form, true);
        if !reports || field(form, "status") != "needs-review" {
            filters.status = "automod";
        }
        filters
    }

    fn url(self, item: &Community) -> String {
        format!(
            "{}&status={}&sort={}&kind={}",
            tools_url(item, "queue"),
            self.status,
            self.sort,
            self.kind.as_str()
        )
    }
}

struct Workspace<'a> {
    item: &'a Community,
    posts: BTreeMap<u64, &'a Post>,
    reports: Vec<(&'a Report, Option<&'a Post>)>,
    comments: usize,
    upcoming: usize,
}

impl<'a> Workspace<'a> {
    fn new(state: &'a State, item: &'a Community) -> Self {
        let posts: BTreeMap<_, _> = state
            .posts
            .iter()
            .filter(|post| post.community == item.slug)
            .map(|post| (post.id, post))
            .collect();
        let sees_reports = state.can_moderate(item, ModPermission::Reports);
        let reports = state
            .reports
            .iter()
            .filter(|report| {
                sees_reports
                    && state
                        .report_community(report)
                        .is_some_and(|community| community.slug == item.slug)
            })
            .map(|report| (report, posts.get(&report.post).copied()))
            .collect();
        let comments = state
            .comments
            .iter()
            .filter(|comment| !comment.deleted && posts.contains_key(&comment.post))
            .count();
        let upcoming = state
            .scheduled
            .iter()
            .filter(|entry| entry.community == item.slug && entry.status == "pending")
            .count();
        Self {
            item,
            posts,
            reports,
            comments,
            upcoming,
        }
    }

    fn frame(
        &self,
        state: &State,
        section: &str,
        heading: &str,
        search: &str,
        body: &str,
    ) -> String {
        let navigation = navigation(state, self.item, section, search);
        let community = format!("c/{}", escape(&self.item.slug));
        let community = if state.can_view_community(self.item) {
            format!(
                "<a class='subtle-link' href='{}'>{community}</a>",
                community_url(&self.item.slug)
            )
        } else {
            format!("<span class='muted'>{community}</span>")
        };
        let (summary, workspace_class) = if can_view(state, self.item, "insights") {
            (self.summary(state), "")
        } else {
            (String::new(), " mod-workspace-limited")
        };
        format!(
            "<div class='mod-workspace{workspace_class}'>{navigation}<section class='mod-tools-main' aria-labelledby='mod-tools-title'><header class='mod-tools-header'>{community}<h1 id='mod-tools-title'>{heading}</h1></header>{body}</section>{summary}</div>",
        )
    }

    fn pending(&self) -> usize {
        self.reports
            .iter()
            .filter(|(report, _)| !report.reviewed)
            .count()
    }

    fn queue(&self, state: &State, query: &BTreeMap<String, String>) -> String {
        let automatic = state.can_moderate(self.item, ModPermission::Content);
        self.queue_filtered(state, QueueFilters::read(query, automatic))
    }

    fn queue_filtered(&self, state: &State, mut filters: QueueFilters) -> String {
        let automatic = state.can_moderate(self.item, ModPermission::Content);
        let reports = state.can_moderate(self.item, ModPermission::Reports);
        if !reports {
            filters.status = "automod";
        }
        let QueueFilters { status, sort, kind } = filters;
        let back = filters.url(self.item);
        let mut flags: Vec<_> = state
            .automod_flags
            .iter()
            .filter(|flag| automatic && flag.community == self.item.slug && kind.allows(flag.kind))
            .collect();
        let flag_count = flags.len();
        let mut entries: Vec<_> = self
            .reports
            .iter()
            .copied()
            .filter(|(report, _)| kind.allows(report.kind()))
            .collect();
        let pending = entries
            .iter()
            .filter(|(report, _)| !report.reviewed)
            .count();
        let statuses = [
            ("needs-review", "Needs review", pending + flag_count),
            ("reported", "Reported", entries.len()),
            ("reviewed", "Reviewed", entries.len() - pending),
            ("automod", "AutoMod flags", flag_count),
        ];
        let mut tabs = String::new();
        for (key, label, count) in statuses {
            if (key == "automod" && !automatic) || (key != "automod" && !reports) {
                continue;
            }
            let active = if key == status {
                " aria-current='page'"
            } else {
                ""
            };
            let href = escape(
                &QueueFilters {
                    status: key,
                    ..filters
                }
                .url(self.item),
            );
            write!(
                tabs,
                "<a href='{href}'{active}>{label}<span>{count}</span></a>"
            )
            .unwrap();
        }
        let mut html = format!(
            "<div class='mod-queue-controls'><nav class='mod-queue-tabs' aria-label='Queue status'>{tabs}</nav><form class='mod-queue-sort' action='/moderation/tools' method='get'>{}{}{}{}<label>Sort<select name='sort'><option value='newest'{}>Newest first</option><option value='oldest'{}>Oldest first</option></select></label><button type='submit' class='button light'>Apply</button></form></div>",
            hidden("community", &self.item.slug),
            hidden("section", "queue"),
            hidden("status", status),
            kind.select(),
            if sort == "newest" { " selected" } else { "" },
            if sort == "oldest" { " selected" } else { "" },
        );
        if status == "automod" {
            if flags.is_empty() {
                html.push_str(&empty(
                    "No AutoMod flags",
                    "Flags matching this content type will appear here.",
                    "check",
                ));
            } else {
                html.push_str(&automod::flags(
                    state, self.item, &mut flags, filters, &back,
                ));
            }
            return html;
        }
        entries.retain(|(report, _)| match status {
            "reviewed" => report.reviewed,
            "reported" => true,
            _ => !report.reviewed,
        });
        let pending_flags = status == "needs-review" && !flags.is_empty();
        entries.sort_by_key(|(report, _)| report.id);
        if sort == "newest" {
            entries.reverse();
        }
        if entries.is_empty() && !pending_flags {
            let (heading, detail) = match status {
                "reported" => (
                    "No reports",
                    "Reports matching this content type will appear here.",
                ),
                "reviewed" => (
                    "No reviewed reports",
                    "Reports you mark reviewed will appear here.",
                ),
                _ if automatic => (
                    "You’re caught up",
                    "New reports and AutoMod flags matching this content type will appear here.",
                ),
                _ => (
                    "You’re caught up",
                    "New reports matching this content type will appear here.",
                ),
            };
            html.push_str(&empty(heading, detail, "check"));
        }
        for (report, post) in entries {
            html.push_str(&report_record(state, self.item, post, report, &back, false));
        }
        if pending_flags {
            html.push_str("<section class='mod-queue-flags' aria-labelledby='pending-flags'><h2 id='pending-flags'>AutoMod flags</h2>");
            html.push_str(&automod::flags(
                state, self.item, &mut flags, filters, &back,
            ));
            html.push_str("</section>");
        }
        html
    }

    fn discussions(&self, state: &State) -> String {
        if self.posts.is_empty() {
            return empty(
                "No discussions yet",
                "Posts in this community will appear here.",
                "chat",
            );
        }
        let mut html =
            String::from("<section class='panel mod-section'><ul class='mod-discussions-list'>");
        let back = tools_url(self.item, "discussions");
        for post in self.posts.values().rev() {
            let pinned = if self.item.highlighted_posts().contains(&post.id) {
                "<span class='flair'>Pinned</span>"
            } else {
                ""
            };
            let (title, review) = self.post_review_links(state, post);
            let lock = moderate_content::discussion_lock_controls(state, self.item, post, &back);
            write!(html, "<li class='mod-discussion-row' data-post-id='{}'><div>{title}<p class='muted'>{} · {} · {} net votes · {} replies</p></div><div class='mod-discussion-actions'>{pinned}{review}{lock}</div></li>", post.id, account_byline(&post.author), escape(&post.age_label(crate::schedule_time::now())), score(state, post), state.visible_reply_count(post)).unwrap();
        }
        html.push_str("</ul></section>");
        html
    }

    fn highlights(&self, state: &State) -> String {
        let back = tools_url(self.item, "highlights");
        let highlighted = self.item.highlighted_posts();
        let mut options = String::new();
        for post in self
            .posts
            .values()
            .rev()
            .filter(|post| !state.is_post_removed(post.id) && !highlighted.contains(&post.id))
        {
            write!(
                options,
                "<option value='{}'>{} · {}</option>",
                post.id,
                escape(display_post_title(state, post)),
                account_byline(&post.author)
            )
            .unwrap();
        }
        let picker = if options.is_empty() {
            let message = if self
                .posts
                .values()
                .any(|post| !state.is_post_removed(post.id))
            {
                "All available posts are already highlighted."
            } else {
                "Publish or restore a post to highlight it."
            };
            format!("<p class='muted'>{message}</p>")
        } else {
            let fields = action_fields("highlight-post", &back, &[("community", &self.item.slug)]);
            format!(
                "<form class='stack-form mod-highlight-add' method='post' action='/action'>{fields}<label>Post to pin<select name='post' required><option value=''>Choose a post</option>{options}</select></label><button class='button dark' type='submit'>Pin post</button></form>"
            )
        };
        let mut html = format!(
            "<section class='panel mod-section mod-highlight-panel' aria-labelledby='highlight-order'><h2 id='highlight-order'>Pinned posts</h2><p class='muted'>Posts appear at the top of the community feed in this order.</p>{picker}"
        );
        if highlighted.is_empty() {
            html.push_str(
                "<p class='mod-highlight-empty muted'>No posts pinned yet.</p></section>",
            );
            return html;
        }

        html.push_str("<ol class='mod-highlight-list' aria-label='Pinned posts in display order'>");
        for (index, id) in highlighted.iter().copied().enumerate() {
            let post = self.posts.get(&id).copied();
            let title =
                escape(post.map_or("Unavailable post", |post| display_post_title(state, post)));
            let title_link = if post.is_some_and(|post| can_view_post(state, post)) {
                format!("<a href='/thread?id={id}'>{title}</a>")
            } else {
                title.clone()
            };
            let status = if state.is_post_removed(id) {
                " <span class='flair'>Removed</span>"
            } else {
                ""
            };
            let id_field = id.to_string();
            let fields = [
                ("community", self.item.slug.as_str()),
                ("post", id_field.as_str()),
            ];
            let move_button = |direction, label, neighbor: Option<&u64>| {
                let expected = neighbor.map(u64::to_string).unwrap_or_default();
                let fields = action_fields(
                    "move-highlight",
                    &back,
                    &[
                        fields[0],
                        fields[1],
                        ("direction", direction),
                        ("neighbor", &expected),
                    ],
                );
                let disabled = if neighbor.is_none() { " disabled" } else { "" };
                format!(
                    "<form class='inline' method='post' action='/action'>{fields}<button class='button light' type='submit' aria-label='{label} {title}'{disabled}>{label}</button></form>"
                )
            };
            let previous = index.checked_sub(1).and_then(|i| highlighted.get(i));
            let move_up = move_button("up", "Move up", previous);
            let move_down = move_button("down", "Move down", highlighted.get(index + 1));
            let unpin = action_fields("unhighlight-post", &back, &fields);
            write!(html, "<li data-post-id='{id}'><div class='mod-highlight-row'><strong>{title_link}{status}</strong><div class='mod-highlight-actions'>{move_up}{move_down}<form class='inline' method='post' action='/action'>{unpin}<button class='button light' type='submit' aria-label='Unpin {title}'>Unpin</button></form></div></div></li>").unwrap();
        }
        html.push_str("</ol></section>");
        html
    }

    fn post_review_links(&self, state: &State, post: &Post) -> (String, String) {
        let title = escape(&post.title);
        let review = if state.can_moderate(self.item, ModPermission::Content) {
            let url = moderate_content::detail_url(self.item, ContentKind::Post, post.id);
            format!(
                "<a class='button light' href='{}'>Review post</a>",
                escape(&url)
            )
        } else {
            String::new()
        };
        let title = if state.content_removal(ContentKind::Post, post.id).is_some() {
            format!("<span>{title}</span> <span class='flair'>Removed</span>")
        } else {
            format!("<a href='/thread?id={}'>{title}</a>", post.id)
        };
        (title, review)
    }

    fn rules(
        &self,
        state: &State,
        edit: &str,
        delete: &str,
        rejected: Option<(&BTreeMap<String, String>, &str)>,
    ) -> String {
        let guidelines = if self.item.guidelines.is_empty() {
            "<p class='muted'>No topic guidelines have been added.</p>".to_owned()
        } else {
            format!(
                "<p class='mod-record-detail'>{}</p>",
                escape(&self.item.guidelines)
            )
        };
        let edit_guidelines = if community_owner(state, self.item) {
            format!(
                "<a class='button light' href='{}'>Edit guidelines</a>",
                escape(&settings_url(self.item))
            )
        } else {
            String::new()
        };
        let mut html = community_rules::render(self.item, edit, delete, rejected);
        write!(html, "<section class='panel mod-section'><h2>Topic guidelines</h2>{guidelines}{edit_guidelines}</section>").unwrap();
        html
    }

    fn members(&self, state: &State) -> String {
        let owner = self.item.owner.as_deref().unwrap_or(&state.username);
        let membership = if state.joined.contains(&self.item.slug) {
            "Joined"
        } else {
            "Not joined"
        };
        let visibility = match self.item.visibility.as_str() {
            "restricted" => "Restricted",
            "private" => "Private",
            _ => "Public",
        };
        let roles = if can_view(state, self.item, "roles") {
            format!(
                "<a class='button light' href='{}'>Moderator roles</a>",
                escape(&tools_url(self.item, "roles"))
            )
        } else {
            String::new()
        };
        format!("<section class='panel mod-section'><h2>Community membership</h2><dl class='mod-metrics'><div><dt>Owner</dt><dd>{}</dd></div><div><dt>Members</dt><dd>{}</dd></div><div><dt>Your membership</dt><dd>{membership}</dd></div><div><dt>Access</dt><dd>{visibility}</dd></div></dl><div class='form-actions mod-members-links'><a class='button light' href='{}'>View community</a><a class='button light' href='{}'>Approved members</a>{roles}</div></section>", account_byline(owner), state.community_member_count(self.item), community_url(&self.item.slug), escape(&tools_url(self.item, "approved-members")))
    }

    fn insights(&self, state: &State) -> String {
        let pending = self.pending();
        format!("<section class='panel mod-section'><h2>Community totals</h2><dl class='mod-metrics'><div><dt>Members</dt><dd>{}</dd></div><div><dt>Discussions</dt><dd>{}</dd></div><div><dt>Comments</dt><dd>{}</dd></div><div><dt>Reports</dt><dd>{}</dd></div><div><dt>Pending reports</dt><dd>{pending}</dd></div><div><dt>Reviewed</dt><dd>{}</dd></div></dl></section>", state.community_member_count(self.item), self.posts.len(), self.comments, self.reports.len(), self.reports.len() - pending)
    }

    fn log(&self, state: &State) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut entries = state
            .mod_log
            .iter()
            .rev()
            .filter(|event| event.community == self.item.slug)
            .peekable();
        if entries.peek().is_none() {
            return empty(
                "No moderation actions yet",
                "Report reviews and moderator replies will appear here.",
                "log",
            );
        }
        let mut html = String::from("<section class='panel mod-section'><ol class='mod-discussions-list' aria-label='Latest moderation actions'>");
        for event in entries {
            let age = elapsed(now.saturating_sub(event.at));
            write!(
                html,
                "<li><div><strong>{}</strong><p class='muted'>{age}</p></div></li>",
                escape(&event.action)
            )
            .unwrap();
        }
        html.push_str("</ol></section>");
        html
    }

    fn responses(&self, state: &State) -> String {
        let back = tools_url(self.item, "responses");
        let fields = action_fields(
            "add-saved-response",
            &back,
            &[("community", &self.item.slug)],
        );
        let mut html = format!("<section class='panel mod-section'><h2>Add a saved response</h2><form class='stack-form' method='post' action='/action'>{fields}<label>Response title<input name='title' maxlength='80' required></label><label>Response text<textarea name='body' rows='4' maxlength='4000' required></textarea></label><button type='submit' class='button dark'>Save response</button></form></section>");
        for response in state
            .saved_responses
            .iter()
            .rev()
            .filter(|response| response.community == self.item.slug)
        {
            let delete = action_form(
                "delete-saved-response",
                &back,
                &[("id", &response.id.to_string())],
                "Delete response",
                "",
                "button light",
            );
            write!(html, "<article class='panel mod-record'><h2>{}</h2><p class='mod-record-detail'>{}</p><div class='form-actions'>{delete}</div></article>", escape(&response.title), escape(&response.body)).unwrap();
        }
        html
    }

    fn summary(&self, state: &State) -> String {
        let scheduled = if can_view(state, self.item, "scheduled") {
            format!("<div><dt>Scheduled</dt><dd>{}</dd></div>", self.upcoming)
        } else {
            String::new()
        };
        format!("<aside class='mod-tools-insights' aria-label='Community insights'><section class='panel mod-summary'><h2>Community activity</h2><dl class='mod-metrics'><div><dt>Discussions</dt><dd>{}</dd></div><div><dt>Comments</dt><dd>{}</dd></div><div><dt>Pending reports</dt><dd>{}</dd></div>{scheduled}</dl><a class='subtle-link' href='{}'>View insights</a></section><section class='panel mod-summary'><h2>Review policy</h2><a class='subtle-link' href='/policy'>Read the moderation policy</a></section></aside>", self.posts.len(), self.comments, self.pending(), escape(&tools_url(self.item, "insights")))
    }
}

pub(super) fn report_record(
    state: &State,
    item: &Community,
    post: Option<&Post>,
    report: &Report,
    back: &str,
    aggregate: bool,
) -> String {
    let (status, value, button) = if report.reviewed {
        ("Reviewed", "no", "Reopen report")
    } else {
        ("Pending review", "yes", "Mark reviewed")
    };
    let id = report.id;
    let action = action_form(
        "review-report",
        back,
        &[("id", &id.to_string()), ("reviewed", value)],
        button,
        "",
        "button light",
    );
    let kind = report.kind();
    let (heading, review) = report_target_links(state, item, post, report);
    let (source, reason) = report.reason_label().map_or_else(
        || {
            (
                "Legal report",
                format!("<strong>Region</strong> {}", escape(&report.region)),
            )
        },
        |reason| {
            (
                "Community report",
                format!("<strong>Reason</strong> {}", escape(reason)),
            )
        },
    );
    let rule_description = report
        .rule
        .as_ref()
        .filter(|rule| !rule.description.is_empty())
        .map_or_else(String::new, |rule| {
            format!(
                "<p class='mod-record-detail'><strong>Rule description when reported</strong><br>{}</p>",
                escape(&rule.description)
            )
        });
    let details = if report.details.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='mod-record-detail'>{}</p>",
            escape(&report.details)
        )
    };
    let mut scope = String::new();
    if aggregate {
        let name = escape(&item.name);
        scope = if state.can_view_community(item) {
            format!(
                "<a class='subtle-link' href='{}'>{name}</a> · ",
                community_url(&item.slug)
            )
        } else {
            format!("{name} · ")
        };
    }
    let class = if aggregate {
        "log-item moderation-record"
    } else {
        "panel mod-record"
    };
    format!("<article class='{class} mod-report' data-report-id='{id}' data-content-kind='{}'><header><span class='flair'>{status}</span><span class='muted'>{source} · Report #{id}</span></header><h2>{heading}</h2><p class='mod-report-meta'>{scope}{} · {reason}</p>{rule_description}{details}<div class='form-actions'>{action}{review}</div></article>", kind.as_str(), kind.label())
}

fn report_target_links(
    state: &State,
    item: &Community,
    post: Option<&Post>,
    report: &Report,
) -> (String, String) {
    let kind = report.kind();
    let content_id = report.content_id();
    let unavailable = || (format!("{} unavailable", kind.label()), String::new());
    let Some(post) = post else {
        return unavailable();
    };
    let title = escape(&post.title);
    let url = format!("/thread?id={}", post.id);
    let (heading, url, visible_target) = if let Some(comment_id) = report.comment {
        let Some(comment) = state.comments.iter().find(|comment| {
            comment.id == comment_id && comment.post == post.id && !comment.deleted
        }) else {
            return unavailable();
        };
        (
            format!("Comment on {title}"),
            format!("{url}#comment-{comment_id}"),
            !state.blocked.contains(&comment.author),
        )
    } else {
        (title, url, true)
    };
    let heading = if state.content_removal(kind, content_id).is_some() {
        format!("{heading} <span class='flair'>Removed</span>")
    } else if state.is_post_removed(post.id) {
        format!("{heading} <span class='flair'>Thread removed</span>")
    } else if visible_target && can_view_post(state, post) {
        format!("<a href='{}'>{heading}</a>", escape(&url))
    } else {
        heading
    };
    let review = if state.can_moderate(item, ModPermission::Content) {
        let url = moderate_content::detail_url(item, kind, content_id);
        format!(
            "<a class='button light' href='{}'>Review content</a>",
            escape(&url)
        )
    } else {
        String::new()
    };
    (heading, review)
}

fn elapsed(seconds: u64) -> String {
    let (amount, unit) = match seconds {
        ..60 => return "Just now".into(),
        ..3600 => (seconds / 60, "minute"),
        ..86400 => (seconds / 3600, "hour"),
        _ => (seconds / 86400, "day"),
    };
    let plural = if amount == 1 { "" } else { "s" };
    format!("{amount} {unit}{plural} ago")
}

fn empty(title: &str, detail: &str, glyph: &str) -> String {
    format!("<section class='panel mod-empty'><span class='mod-empty-icon'>{}</span><h2>{title}</h2><p class='muted'>{detail}</p></section>", icon(glyph))
}

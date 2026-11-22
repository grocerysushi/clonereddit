use std::{
    collections::BTreeMap,
    fmt::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::model::{Community, Post, Report, State};

use super::{
    action_fields, action_form, community_icon, community_owner, community_url, encode, escape,
    field, hidden, icon, score,
};
use super::{restricted, scheduled};

const SCHEDULED_TITLE: &str = "Scheduled posts and events";
const RESTRICTED_TITLE: &str = "Restricted Users";

const TOOLS: &[(&str, &str, &str, &str)] = &[
    (
        "queue",
        "Queue",
        "shield",
        "Review legal reports and update their status.",
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
        "discussions",
        "Discussions",
        "chat",
        "Browse discussions in this community.",
    ),
    (
        "rules",
        "Rules",
        "log",
        "View the community’s topic guidelines.",
    ),
    (
        "members",
        "Members",
        "grid",
        "See ownership, membership, and access.",
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

pub(super) fn render(state: &State, item: &Community, query: &BTreeMap<String, String>) -> String {
    let workspace = Workspace::new(state, item);
    let (section, title, _, _) = TOOLS
        .iter()
        .copied()
        .find(|(key, _, _, _)| *key == field(query, "section"))
        .unwrap_or(TOOLS[0]);
    let search = field(query, "tool_q").trim();
    let body = if search.is_empty() {
        match section {
            "mail" => workspace.mail(state),
            "scheduled" => scheduled::render(state, item, query),
            "restricted-users" => restricted::render(state, item),
            "discussions" => workspace.discussions(state),
            "rules" => workspace.rules(),
            "members" => workspace.members(state),
            "insights" => workspace.insights(state),
            "log" => workspace.log(state),
            "responses" => workspace.responses(state),
            _ => workspace.queue(query),
        }
    } else {
        search_results(item, search)
    };
    let heading = if search.is_empty() {
        title
    } else {
        "Search tools"
    };
    workspace.frame(state, section, heading, search, &body)
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
        .filter(|group| community_owner(state, group))
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
    format!(
        r#"<aside class="mod-tools-nav" aria-label="Moderation tools">
<a class="mod-community-brand" href="{community_url}">{mark}<span>{name}<small>Mod tools</small></span></a>
<form class="mod-scope" action="/moderation/tools" method="get">{section_field}<label>Community<select name="community">{options}</select></label><button type="submit" class="button light">Switch</button></form>
<form class="mod-tool-search" action="/moderation/tools" method="get" role="search">{community}{section_field}<label class="sr-only" for="mod-tool-search">Search moderation tools</label><input id="mod-tool-search" name="tool_q" type="search" maxlength="80" value="{search}" placeholder="Search tools"><button type="submit" class="button light" aria-label="Search moderation tools">{search_icon}</button></form>
<nav aria-label="Mod tools">{links}</nav><nav class="mod-management-links" aria-label="Community management"><a href="{settings}">{settings_icon}<span>Community settings</span></a><a href="/moderation?section=manage">{grid}<span>Manage communities</span></a></nav>
</aside>"#,
        community_url = community_url(&item.slug),
        mark = community_icon(item),
        name = escape(&item.name),
        search = escape(search),
        search_icon = icon("search"),
        settings = escape(&settings_url(item)),
        settings_icon = icon("sliders"),
        grid = icon("grid"),
    )
}

fn search_results(item: &Community, search: &str) -> String {
    let needle = search.to_lowercase();
    let mut html = format!(
        "<section class='panel mod-section'><h2>Results for “{}”</h2><ul class='mod-tool-results'>",
        escape(search)
    );
    let mut any = false;
    for (key, label, glyph, description) in TOOLS {
        if [*label, *description]
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

struct Workspace<'a> {
    item: &'a Community,
    posts: BTreeMap<u64, &'a Post>,
    reports: Vec<(&'a Report, &'a Post)>,
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
        let reports = state
            .reports
            .iter()
            .filter_map(|report| posts.get(&report.post).map(|post| (report, *post)))
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
        format!(
            "<div class='mod-workspace'>{navigation}<section class='mod-tools-main' aria-labelledby='mod-tools-title'><header class='mod-tools-header'><a class='subtle-link' href='{}'>c/{}</a><h1 id='mod-tools-title'>{heading}</h1></header>{body}</section>{}</div>",
            community_url(&self.item.slug),
            escape(&self.item.slug),
            self.summary(),
        )
    }

    fn pending(&self) -> usize {
        self.reports
            .iter()
            .filter(|(report, _)| !report.reviewed)
            .count()
    }

    fn queue(&self, query: &BTreeMap<String, String>) -> String {
        let status = match field(query, "status") {
            "reported" => "reported",
            "reviewed" => "reviewed",
            _ => "needs-review",
        };
        let sort = if field(query, "sort") == "oldest" {
            "oldest"
        } else {
            "newest"
        };
        let base = tools_url(self.item, "queue");
        let back = format!("{base}&status={status}&sort={sort}");
        let pending = self.pending();
        let statuses = [
            ("needs-review", "Needs review", pending),
            ("reported", "Reported", self.reports.len()),
            ("reviewed", "Reviewed", self.reports.len() - pending),
        ];
        let mut tabs = String::new();
        for (key, label, count) in statuses {
            let active = if key == status {
                " aria-current='page'"
            } else {
                ""
            };
            let href = escape(&format!("{base}&status={key}&sort={sort}"));
            write!(
                tabs,
                "<a href='{href}'{active}>{label}<span>{count}</span></a>"
            )
            .unwrap();
        }
        let mut html = format!(
            "<div class='mod-queue-controls'><nav class='mod-queue-tabs' aria-label='Queue status'>{tabs}</nav><form class='mod-queue-sort' action='/moderation/tools' method='get'>{}{}{}<label>Sort reports<select name='sort'><option value='newest'{}>Newest first</option><option value='oldest'{}>Oldest first</option></select></label><button type='submit' class='button light'>Apply</button></form></div>",
            hidden("community", &self.item.slug),
            hidden("section", "queue"),
            hidden("status", status),
            if sort == "newest" { " selected" } else { "" },
            if sort == "oldest" { " selected" } else { "" },
        );
        let mut entries: Vec<_> = self
            .reports
            .iter()
            .copied()
            .filter(|(report, _)| match status {
                "reviewed" => report.reviewed,
                "reported" => true,
                _ => !report.reviewed,
            })
            .collect();
        entries.sort_by_key(|(report, _)| report.id);
        if sort == "newest" {
            entries.reverse();
        }
        if entries.is_empty() {
            let (heading, detail) = match status {
                "reported" => (
                    "No reports",
                    "Legal reports for this community will appear here.",
                ),
                "reviewed" => (
                    "No reviewed reports",
                    "Reports you mark reviewed will appear here.",
                ),
                _ => (
                    "You’re caught up",
                    "New legal reports for this community will appear here.",
                ),
            };
            html.push_str(&empty(heading, detail, "check"));
        }
        for (report, post) in entries {
            let (label, value, button) = if report.reviewed {
                ("Reviewed", "no", "Reopen report")
            } else {
                ("Pending review", "yes", "Mark reviewed")
            };
            let action = action_form(
                "review-report",
                &back,
                &[("id", &report.id.to_string()), ("reviewed", value)],
                button,
                "",
                "button light",
            );
            write!(html, "<article class='panel mod-record'><header><span class='flair'>{label}</span><span class='muted'>Report #{}</span></header><h2><a href='/thread?id={}'>{}</a></h2><p><strong>Region</strong> {}</p><p class='mod-record-detail'>{}</p><div class='form-actions'>{action}</div></article>", report.id, post.id, escape(&post.title), escape(&report.region), escape(&report.details)).unwrap();
        }
        html
    }

    fn mail(&self, state: &State) -> String {
        let back = tools_url(self.item, "mail");
        let mut options = String::new();
        for response in state
            .saved_responses
            .iter()
            .filter(|response| response.community == self.item.slug)
        {
            write!(
                options,
                "<option value='{}'>{}</option>",
                response.id,
                escape(&response.title)
            )
            .unwrap();
        }
        let (picker, required) = if options.is_empty() {
            (String::new(), " required")
        } else {
            (format!("<label>Saved response<select name='response_id'><option value=''>Choose a saved response</option>{options}</select></label><small class='muted'>A written reply takes priority.</small>"), "")
        };
        let inbox = state
            .mod_mail
            .iter()
            .rev()
            .filter(|message| message.community == self.item.slug);
        let mut html = String::new();
        for message in inbox {
            write!(html, "<article class='panel mod-record'><header><span class='flair'>{}</span></header><h2>{}</h2><p class='muted'>From u/{}</p><p class='mod-record-detail'>{}</p>", if message.reply.is_empty() { "Awaiting reply" } else { "Replied" }, escape(&message.subject), escape(&message.sender), escape(&message.body)).unwrap();
            if message.reply.is_empty() {
                let fields =
                    action_fields("reply-mod-mail", &back, &[("id", &message.id.to_string())]);
                write!(html, "<form class='stack-form mod-reply-form' method='post' action='/action'>{fields}{picker}<label>Reply<textarea name='reply' rows='3' maxlength='4000'{required}></textarea></label><button type='submit' class='button dark'>Send reply</button></form>").unwrap();
            } else {
                write!(html, "<blockquote class='moderation-reply'><strong>Moderator reply</strong><p>{}</p></blockquote>", escape(&message.reply)).unwrap();
            }
            html.push_str("</article>");
        }
        if html.is_empty() {
            return empty(
                "Inbox is empty",
                "Messages to this community’s moderators will appear here.",
                "mail",
            );
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
        for post in self.posts.values().rev() {
            let pinned = if self.item.pinned_post == Some(post.id) {
                "<span class='flair'>Pinned</span>"
            } else {
                ""
            };
            write!(html, "<li><div><a href='/thread?id={}'>{}</a><p class='muted'>u/{} · {} · {} net votes · {} replies</p></div>{pinned}</li>", post.id, escape(&post.title), escape(&post.author), escape(&post.age), score(state, post), post.reply_count).unwrap();
        }
        html.push_str("</ul></section>");
        html
    }

    fn rules(&self) -> String {
        let guidelines = if self.item.guidelines.is_empty() {
            "<p class='muted'>No topic guidelines have been added.</p>".to_owned()
        } else {
            format!(
                "<p class='mod-record-detail'>{}</p>",
                escape(&self.item.guidelines)
            )
        };
        format!("<section class='panel mod-section'><h2>Topic guidelines</h2>{guidelines}<a class='button light' href='{}'>Edit guidelines</a></section>", escape(&settings_url(self.item)))
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
        format!("<section class='panel mod-section'><h2>Community membership</h2><dl class='mod-metrics'><div><dt>Owner</dt><dd>u/{}</dd></div><div><dt>Members</dt><dd>{}</dd></div><div><dt>Your membership</dt><dd>{membership}</dd></div><div><dt>Access</dt><dd>{visibility}</dd></div></dl><a class='button light' href='{}'>View community</a></section>", escape(owner), state.community_member_count(self.item), community_url(&self.item.slug))
    }

    fn insights(&self, state: &State) -> String {
        let pending = self.pending();
        format!("<section class='panel mod-section'><h2>Community totals</h2><dl class='mod-metrics'><div><dt>Members</dt><dd>{}</dd></div><div><dt>Discussions</dt><dd>{}</dd></div><div><dt>Comments</dt><dd>{}</dd></div><div><dt>Legal reports</dt><dd>{}</dd></div><div><dt>Pending review</dt><dd>{pending}</dd></div><div><dt>Reviewed</dt><dd>{}</dd></div></dl></section>", state.community_member_count(self.item), self.posts.len(), self.comments, self.reports.len(), self.reports.len() - pending)
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

    fn summary(&self) -> String {
        format!("<aside class='mod-tools-insights' aria-label='Community insights'><section class='panel mod-summary'><h2>Community activity</h2><dl class='mod-metrics'><div><dt>Discussions</dt><dd>{}</dd></div><div><dt>Comments</dt><dd>{}</dd></div><div><dt>Needs review</dt><dd>{}</dd></div><div><dt>Scheduled</dt><dd>{}</dd></div></dl><a class='subtle-link' href='{}'>View insights</a></section><section class='panel mod-summary'><h2>Review policy</h2><p class='muted'>Changing a report’s status leaves its discussion visible.</p><a class='subtle-link' href='/policy'>Read the moderation policy</a></section></aside>", self.posts.len(), self.comments, self.pending(), self.upcoming, escape(&tools_url(self.item, "insights")))
    }
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

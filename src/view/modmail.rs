use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use crate::http::Response;
use crate::model::{Community, ModMail, ModPermission, State, DELETED_ACCOUNT};
use crate::schedule_time;

use super::{
    account_byline, action_fields, action_form, community, community_url, encode, escape, field,
    layout, missing,
};

const MAIL: &str = "/moderation?section=mail";
pub(super) const SENT: &str = "/moderation?section=mail&folder=sent";

#[derive(Clone, Copy, Eq, PartialEq)]
enum Folder {
    Inbox,
    Archive,
    Sent,
}

fn has_inbox(state: &State) -> bool {
    state
        .communities
        .iter()
        .any(|item| state.can_moderate(item, ModPermission::Mail))
}

fn select_folder(
    scope: Option<&Community>,
    query: &BTreeMap<String, String>,
    has_inbox: bool,
) -> Folder {
    match field(query, "folder") {
        "sent" if scope.is_none() => Folder::Sent,
        _ if scope.is_none() && !has_inbox => Folder::Sent,
        "archive" => Folder::Archive,
        _ => Folder::Inbox,
    }
}

pub(super) fn sent_folder(state: &State, query: &BTreeMap<String, String>) -> bool {
    select_folder(None, query, has_inbox(state)) == Folder::Sent
}

fn find_thread<'a>(
    state: &'a State,
    scope: Option<&Community>,
    folder: Folder,
    selected: &str,
) -> Option<(&'a Community, &'a ModMail)> {
    let thread = state.visible_mod_mail(selected.parse().ok()?)?;
    let item = state.mod_mail_community(thread)?;
    let allowed = if folder == Folder::Sent {
        state.can_read_sent_mod_mail(item, thread)
    } else {
        scope.is_none_or(|scope| scope.slug == item.slug)
            && state.can_moderate(item, ModPermission::Mail)
    };
    allowed.then_some((item, thread))
}

pub(super) fn requested_thread_available(
    state: &State,
    scope: Option<&Community>,
    query: &BTreeMap<String, String>,
) -> bool {
    let id = field(query, "id");
    id.is_empty()
        || find_thread(
            state,
            scope,
            select_folder(scope, query, has_inbox(state)),
            id,
        )
        .is_some()
}

impl Folder {
    fn key(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Archive => "archive",
            Self::Sent => "sent",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Archive => "Archived",
            Self::Sent => "Sent",
        }
    }
}

fn inbox_url(scope: Option<&Community>, folder: Folder) -> String {
    let base = scope.map_or_else(
        || MAIL.to_owned(),
        |community| {
            format!(
                "/moderation/tools?community={}&section=mail",
                encode(&community.slug)
            )
        },
    );
    format!("{base}&folder={}", folder.key())
}

fn thread_url(inbox: &str, id: u64) -> String {
    format!("{inbox}&id={id}#mod-mail-thread")
}

fn thread_notice_url(inbox: &str, id: u64, notice: &str) -> String {
    format!("{inbox}&id={id}&notice={}#mod-mail-thread", encode(notice))
}

fn writable_folder(thread: &ModMail) -> Folder {
    if thread.archived {
        Folder::Archive
    } else {
        Folder::Inbox
    }
}

pub(super) fn notification_link(state: &State, id: u64) -> String {
    let Some(thread) = state.visible_mod_mail(id) else {
        return String::new();
    };
    let url = if thread.sender == state.username {
        thread_url(SENT, id)
    } else {
        let folder = inbox_url(state.mod_mail_community(thread), writable_folder(thread));
        thread_url(&folder, id)
    };
    format!(
        "<a class='subtle-link' href='{}'>Open mod mail thread</a>",
        escape(&url)
    )
}

pub(super) fn render(
    state: &State,
    scope: Option<&Community>,
    query: &BTreeMap<String, String>,
) -> String {
    let has_inbox = has_inbox(state);
    let folder = select_folder(scope, query, has_inbox);
    let back = inbox_url(scope, folder);
    let selected = field(query, "id");
    let content = if selected.is_empty() {
        let mut nav =
            String::from("<nav class='account-notifications-nav' aria-label='Mod mail folders'>");
        for tab in [Folder::Inbox, Folder::Archive, Folder::Sent] {
            if (tab == Folder::Sent && scope.is_some()) || (tab != Folder::Sent && !has_inbox) {
                continue;
            }
            let current = if tab == folder {
                " aria-current='page'"
            } else {
                ""
            };
            write!(
                nav,
                "<a href='{}'{current}>{}</a>",
                escape(&inbox_url(scope, tab)),
                tab.label()
            )
            .unwrap();
        }
        nav.push_str("</nav>");
        nav + &listing(state, scope, folder, &back)
    } else {
        find_thread(state, scope, folder, selected)
            .map_or_else(
                || format!(
                    "<section class='panel mod-record'><h2>Thread unavailable</h2><a class='subtle-link' href='{}'>Back to {}</a></section>",
                    escape(&back),
                    folder.label()
                ),
                |(item, thread)| detail(state, item, thread, scope, folder, None),
            )
    };
    if scope.is_some() {
        content
    } else {
        format!("<section class='panel standalone moderation-tool'><h1>Mod mail</h1>{content}</section>")
    }
}

fn listing(state: &State, scope: Option<&Community>, folder: Folder, back: &str) -> String {
    let mut threads: Vec<_> = state
        .mod_mail
        .iter()
        .filter_map(|thread| {
            let item = state.mod_mail_community(thread)?;
            let allowed = if folder == Folder::Sent {
                state.can_read_sent_mod_mail(item, thread)
            } else {
                scope.is_none_or(|scope| scope.slug == item.slug)
                    && (folder == Folder::Archive) == thread.archived
                    && state.can_moderate(item, ModPermission::Mail)
            };
            allowed.then_some((item, thread))
        })
        .collect();
    threads.sort_by_key(|(_, thread)| Reverse(thread.last_activity()));
    let mut html = format!(
        "<section class='preference-list' aria-labelledby='mail-folder-heading'><h2 id='mail-folder-heading'>{}</h2>",
        folder.label()
    );
    if threads.is_empty() {
        html.push_str(match folder {
            Folder::Inbox => "<p class='muted'>No open threads in your inbox.</p>",
            Folder::Archive => "<p class='muted'>No archived threads.</p>",
            Folder::Sent => "<p class='muted'>No messages sent.</p>",
        });
    } else {
        html.push_str("<ul class='creator-list' role='list'>");
        for (item, thread) in threads {
            let status = if thread.archived { "Archived" } else { "Open" };
            write!(
                html,
                "<li class='creator-row'><div class='creator-row-main'><a class='subtle-link' href='{}'>{}</a><p class='muted'>{} · {}</p></div><span class='flair'>{status}</span></li>",
                escape(&thread_url(back, thread.id)),
                escape(&thread.subject),
                escape(&item.name),
                account_byline(&thread.sender),
            ).unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

fn detail(
    state: &State,
    community: &Community,
    thread: &ModMail,
    scope: Option<&Community>,
    folder: Folder,
    failed: Option<(&BTreeMap<String, String>, &str)>,
) -> String {
    let moderator = folder != Folder::Sent;
    let list = inbox_url(scope, folder);
    let back = thread_url(&list, thread.id);
    let status = if thread.archived { "Archived" } else { "Open" };
    let mut html = format!(
        "<section class='panel mod-record' id='mod-mail-thread' aria-labelledby='mod-mail-heading' tabindex='-1'><div class='form-actions'><a class='subtle-link' href='{}'>Back to {}</a><span class='flair'>{status}</span></div><h2 id='mod-mail-heading'>{}</h2><p class='muted'>{}</p>",
        escape(&list),
        folder.label(),
        escape(&thread.subject),
        escape(&community.name),
    );
    if let Some((_, error)) = failed {
        write!(
            html,
            "<p class='notice setup-error' role='alert'>{}</p>",
            escape(error)
        )
        .unwrap();
    }
    html.push_str("<ol class='account-message-list preference-list' role='list' aria-label='Thread messages'>");
    message(
        &mut html,
        &account_byline(&thread.sender),
        &thread.body,
        thread.started_at,
    );
    for reply in &thread.replies {
        let author = if reply.moderator {
            if moderator && !reply.author.is_empty() {
                format!("{} · Moderator", account_byline(&reply.author))
            } else {
                "Moderation team".into()
            }
        } else {
            account_byline(&reply.author)
        };
        message(&mut html, &author, &reply.body, reply.at);
    }
    html.push_str("</ol>");
    if thread.archived {
        html.push_str(
            "<p class='muted'>This thread is archived. A moderator can reopen it for replies.</p>",
        );
    } else if thread.sender == DELETED_ACCOUNT {
        html.push_str("<p class='muted'>This account is unavailable for replies.</p>");
    } else if !moderator && state.is_mod_mail_muted(&community.slug, &state.username) {
        html.push_str("<p class='muted'>Your account is muted in this community’s mod mail. <a class='subtle-link' href='/appeals'>View your appeals</a></p>");
    } else {
        let form = failed.map(|(form, _)| form);
        html.push_str(&reply_form(
            state, community, thread, scope, moderator, &back, form,
        ));
    }
    if moderator {
        let (action, label) = if thread.archived {
            ("reopen-mod-mail", "Reopen thread")
        } else {
            ("archive-mod-mail", "Archive thread")
        };
        let id = thread.id.to_string();
        let scope = if scope.is_some() {
            "community"
        } else {
            "global"
        };
        let controls = action_form(
            action,
            &back,
            &[
                ("id", &id),
                ("mail_scope", scope),
                ("community", &community.slug),
            ],
            label,
            "",
            "button light",
        );
        write!(
            html,
            "<div class='form-actions preference-list'>{controls}</div>"
        )
        .unwrap();
    }
    html.push_str("</section>");
    html
}

fn message(html: &mut String, author: &str, body: &str, at: u64) {
    let time = if at == 0 {
        String::new()
    } else {
        format!(
            "<time datetime='{}Z'>{}</time>",
            escape(&schedule_time::to_local(at, "UTC")),
            escape(&schedule_time::display(at, "America/Chicago"))
        )
    };
    write!(
        html,
        "<li class='account-message'><div class='account-message-meta'><strong>{author}</strong>{time}</div><p class='account-message-body'>{}</p></li>",
        escape(body)
    ).unwrap();
}

fn reply_form(
    state: &State,
    community: &Community,
    thread: &ModMail,
    scope: Option<&Community>,
    moderator: bool,
    back: &str,
    submitted: Option<&BTreeMap<String, String>>,
) -> String {
    let field_value = |name| submitted.map_or("", |form| field(form, name));
    let action = if moderator {
        "reply-mod-mail"
    } else {
        "respond-mod-mail"
    };
    let scope = if scope.is_some() {
        "community"
    } else {
        "global"
    };
    let id = thread.id.to_string();
    let fields = action_fields(
        action,
        back,
        &[
            ("id", &id),
            ("mail_scope", scope),
            ("community", &community.slug),
        ],
    );
    let mut templates = String::new();
    if moderator {
        for response in state
            .saved_responses
            .iter()
            .filter(|response| response.community == community.slug)
        {
            let id = response.id.to_string();
            let selected = if id == field_value("response_id") {
                " selected"
            } else {
                ""
            };
            write!(
                templates,
                "<option value='{id}'{selected}>{}</option>",
                escape(&response.title)
            )
            .unwrap();
        }
    }
    let (picker, required) = if templates.is_empty() {
        (String::new(), " required")
    } else {
        (format!("<label>Saved response<select name='response_id'><option value=''>Choose a saved response</option>{templates}</select></label><small class='muted'>A written reply takes priority.</small>"), "")
    };
    let draft = escape(field_value("reply"));
    format!("<form class='stack-form mod-reply-form' method='post' action='/action' aria-label='Reply to mod mail thread'>{fields}{picker}<label>Reply<textarea name='reply' rows='4' maxlength='4000'{required}>{draft}</textarea></label><button type='submit' class='button dark'>Send reply</button></form>")
}

pub(super) fn contact(
    item: &Community,
    failed: Option<(&BTreeMap<String, String>, &str)>,
) -> String {
    let fields = action_fields("send-mod-mail", SENT, &[("community", &item.slug)]);
    let community = escape(&item.name);
    let cancel = community_url(&item.slug);
    let (error, subject, draft) = failed.map_or_else(
        || (String::new(), String::new(), String::new()),
        |(form, error)| {
            (
                format!(
                    "<p class='notice setup-error' role='alert'>{}</p>",
                    escape(error)
                ),
                escape(field(form, "subject")),
                escape(field(form, "body")),
            )
        },
    );
    format!("<section class='panel standalone moderation-tool'><h1>Contact moderators</h1><p>Send a message to the moderators of <strong>{community}</strong>.</p>{error}<form class='stack-form' action='/action' method='post'>{fields}<label>Subject<input name='subject' maxlength='120' value='{subject}' required></label><label>Message<textarea name='body' rows='7' maxlength='4000' required>{draft}</textarea></label><div class='form-actions'><button class='button dark' type='submit'>Send message</button><a class='button light' href='{cancel}'>Cancel</a></div></form><a class='subtle-link' href='{SENT}'>View sent mod mail</a></section>")
}

pub(super) fn submission(
    state: &mut State,
    path: &Path,
    action: &str,
    form: &BTreeMap<String, String>,
) -> Response {
    if action == "send-mod-mail" {
        let slug = field(form, "community");
        let allowed = community(state, slug)
            .is_some_and(|item| item.owner.is_some() && state.can_view_community(item));
        if !allowed {
            return missing(state);
        }
        return match state.apply(action, form, path) {
            Ok(notice) => {
                let thread = state
                    .mod_mail
                    .last()
                    .expect("successful submission creates a thread");
                Response::redirect(&thread_notice_url(SENT, thread.id, notice))
            }
            Err(error) => {
                let Some(item) = community(state, slug) else {
                    return missing(state);
                };
                let body = contact(item, Some((form, &error)));
                Response::html(layout(state, slug, "Contact moderators", &body, "", ""))
            }
        };
    }

    let Some(id) = field(form, "id").parse::<u64>().ok() else {
        return missing(state);
    };
    let Some(thread) = state.visible_mod_mail(id) else {
        return missing(state);
    };
    let Some(item) = state.mod_mail_community(thread) else {
        return missing(state);
    };
    let moderator = action != "respond-mod-mail";
    let allowed = if moderator {
        state.can_moderate(item, ModPermission::Mail)
    } else {
        state.can_read_sent_mod_mail(item, thread)
    };
    if !allowed || form.get("community").is_some_and(|slug| *slug != item.slug) {
        return missing(state);
    }
    let scoped = moderator && field(form, "mail_scope") == "community";
    let folder = if moderator {
        match action {
            "archive-mod-mail" => Folder::Archive,
            "reopen-mod-mail" => Folder::Inbox,
            _ => writable_folder(thread),
        }
    } else {
        Folder::Sent
    };
    let back = inbox_url(scoped.then_some(item), folder);
    match state.apply(action, form, path) {
        Ok(notice) => Response::redirect(&thread_notice_url(&back, id, notice)),
        Err(error) => {
            let Some(thread) = state.visible_mod_mail(id) else {
                return missing(state);
            };
            let Some(item) = state.mod_mail_community(thread) else {
                return missing(state);
            };
            let folder = if moderator {
                writable_folder(thread)
            } else {
                Folder::Sent
            };
            let content = detail(
                state,
                item,
                thread,
                scoped.then_some(item),
                folder,
                Some((form, &error)),
            );
            let active = if moderator { "mail" } else { "mod-mail-sent" };
            let content = format!("<section class='panel standalone moderation-tool'><h1>Mod mail</h1>{content}</section>");
            Response::html(layout(state, active, "Mod mail", &content, "", ""))
        }
    }
}

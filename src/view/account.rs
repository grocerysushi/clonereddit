use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{account_security, action_form, appearance, encode, escape, field, icon, visible_post};
use crate::model::{
    ChatRequestPermission, DirectMessage, DiscussionNotification, Notification,
    NotificationPreferences, State,
};
use crate::schedule_time::{display, to_local};

#[path = "account_chat.rs"]
mod chat;
#[path = "account_live.rs"]
mod live;
pub(crate) use live::live_messages;

const INBOX: &str = "/messages";
const NOTIFICATIONS: &str = "/notifications";

pub(super) fn settings(
    prefs: NotificationPreferences,
    chat: ChatRequestPermission,
    saved: Option<&str>,
    error: &str,
) -> String {
    let feedback = if !error.is_empty() {
        format!(
            "<p class='notice account-error' role='alert'>{}</p>",
            escape(error)
        )
    } else if let Some(notice) = saved {
        format!("<p class='notice' role='status'>{}</p>", escape(notice))
    } else {
        String::new()
    };
    let switches: String = [
        ("replies", "Replies", "Replies to your posts and comments.", prefs.replies),
        ("mentions", "Mentions", "Posts or comments that mention your username.", prefs.mentions),
        ("messages", "Personal messages", "New direct messages. Conversations stay in Messages.", prefs.messages),
        ("moderation", "Moderation notices", "Restrictions, removals, and appeal decisions.", prefs.moderation),
    ].into_iter().map(|(name, label, help, enabled)| {
        let checked = if enabled { " checked" } else { "" };
        format!("<label class='notification-switch'><span class='notification-switch-text'><strong id='notify-{name}-label'>{label}</strong><small id='notify-{name}-help'>{help}</small></span><span class='notification-switch-control'><input type='checkbox' role='switch' name='{name}' value='on' aria-labelledby='notify-{name}-label' aria-describedby='notify-{name}-help'{checked}><span class='notification-switch-track' aria-hidden='true'></span></span></label>")
    }).collect();
    let chat_settings = chat::settings(chat);
    let appearance_url = appearance::PATH;
    format!(
        r#"<section class="panel standalone account-preferences"><h1>Account Settings</h1>{feedback}
<div class="form-actions"><a class="button light" href="{appearance_url}">Appearance</a></div>
<section aria-labelledby="notification-preferences-title"><h2 id="notification-preferences-title">Notification preferences</h2>
<p>Choose which future activity appears in your in-app notifications. Muted moderation notices remain available under <a class="subtle-link" href="/notifications?view=moderation">Moderation updates</a>.</p>
<form class="stack-form notification-settings-form" action="/account/settings" method="post" aria-labelledby="notification-preferences-title"><input type="hidden" name="mentions_present" value="1">{switches}<button type="submit" class="button dark">Save preferences</button></form></section>{chat_settings}</section>"#
    )
}

pub(super) fn auth_form(kind: &str, username: &str, error: &str) -> String {
    let (title, submit, endpoint, password_label, password_attributes, footer) = match kind {
        "register" => (
            "Create account",
            "Create account",
            "/account/register",
            "Password",
            "autocomplete='new-password' minlength='12'",
            "<a class='subtle-link' href='/account/login'>Sign in to an existing account</a>",
        ),
        "recover" => (
            "Reset your password",
            "Reset password",
            "/account/recover",
            "New password",
            "autocomplete='new-password' minlength='12'",
            "<a class='subtle-link' href='/account/reset/request'>Reset by email</a><a class='subtle-link' href='/account/login'>Back to sign in</a>",
        ),
        _ => (
            "Sign in",
            "Sign in",
            "/account/login",
            "Password",
            "autocomplete='current-password'",
            "<a class='subtle-link' href='/account/register'>Create an account</a><a class='subtle-link' href='/account/reset/request'>Reset by email</a><a class='subtle-link' href='/account/recover'>Use a recovery code</a>",
        ),
    };
    let alert = if error.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='notice account-error' role='alert' tabindex='-1' autofocus>{}</p>",
            escape(error)
        )
    };
    let code = if kind == "recover" {
        "<label>Recovery code<input name='recovery_code' required maxlength='64' pattern='[A-Fa-f0-9]{64}' autocomplete='one-time-code' autocapitalize='none' spellcheck='false'><small>Use the code you saved when you created your account or last reset your password.</small></label>"
    } else {
        ""
    };
    let password_help = if matches!(kind, "register" | "recover") {
        "<small>Use at least 12 characters.</small>"
    } else {
        ""
    };
    let otp = if kind == "recover" {
        account_security::otp_field(false)
    } else {
        String::new()
    };
    format!(
        r#"<section class="panel standalone account-auth"><h1>{title}</h1>{alert}
<form class="stack-form account-auth-form" action="{endpoint}" method="post">
<label>Username<input name="username" autocomplete="username" autocapitalize="none" spellcheck="false" minlength="2" maxlength="24" required value="{}"></label>
{code}<label>{password_label}<input type="password" name="password" {password_attributes} maxlength="128" required>{password_help}</label>{otp}
<button class="button dark" type="submit">{submit}</button></form>
<div class="account-auth-links">{footer}</div></section>"#,
        escape(username)
    )
}

pub(super) fn recovery_code(username: &str, code: &str) -> String {
    format!(
        r#"<section class="panel standalone account-auth account-recovery"><h1>Save your recovery code</h1><p>Store this code somewhere private. It can reset your password once. This is the only time it is displayed.</p>
<label class="account-secret">Recovery code for u/{}<input readonly autocomplete="off" spellcheck="false" value="{}"></label>
<div class="account-auth-links"><a class="button dark" href="/profile">Continue to your profile</a></div></section>"#,
        escape(username),
        escape(code)
    )
}

#[derive(Default)]
struct Conversation {
    latest: (u64, u64),
    unread: usize,
}

pub(super) fn inbox(
    state: &State,
    query: &BTreeMap<String, String>,
    recipients: &[String],
) -> String {
    inbox_draft(state, query, recipients, "", "")
}

pub(super) fn inbox_draft(
    state: &State,
    query: &BTreeMap<String, String>,
    recipients: &[String],
    draft: &str,
    error: &str,
) -> String {
    let username = state.username.as_str();
    let contacts: BTreeSet<&str> = recipients
        .iter()
        .map(String::as_str)
        .filter(|recipient| *recipient != username)
        .collect();
    let mut peers = BTreeMap::<&str, Conversation>::new();
    for message in &state.messages {
        if let Some(peer) = other_participant(message, username) {
            let conversation = peers.entry(peer).or_default();
            conversation.latest = conversation.latest.max((message.at, message.id));
            conversation.unread += usize::from(message.to == username && !message.read);
        }
    }
    let requested = field(query, "peer").trim().to_ascii_lowercase();
    let selected =
        if peers.contains_key(requested.as_str()) || contacts.contains(requested.as_str()) {
            Some(requested.as_str())
        } else {
            None
        };
    let navigation = conversations(peers, selected);
    let thread = selected.map_or_else(String::new, |peer| live::conversation(state, peer));
    let compose = chat::compose(state, &contacts, selected, draft);
    let tabs = chat::tabs(state, false);
    let alert = if error.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='notice account-error' role='alert'>{}</p>",
            escape(error)
        )
    };
    format!(
        "<section class='panel standalone account-inbox'><h1>Messages</h1>{tabs}<div class='account-inbox-grid'>{navigation}<div class='account-inbox-content'>{thread}{alert}{compose}</div></div></section>"
    )
}

pub(super) fn chat_requests(state: &State) -> String {
    chat::requests(state)
}

fn other_participant<'a>(message: &'a DirectMessage, username: &str) -> Option<&'a str> {
    if message.from == username {
        Some(message.to.as_str())
    } else if message.to == username {
        Some(message.from.as_str())
    } else {
        None
    }
}

fn inbox_url(peer: &str) -> String {
    format!("{INBOX}?peer={}", encode(peer))
}

fn conversations(peers: BTreeMap<&str, Conversation>, selected: Option<&str>) -> String {
    let mut peers: Vec<_> = peers.into_iter().collect();
    peers.sort_by_key(|(_, info)| Reverse(info.latest));
    let compose = if selected.is_some() {
        format!("<a class='subtle-link' href='{INBOX}'>New message</a>")
    } else {
        String::new()
    };
    let mut html = format!(
        "<nav class='account-conversations' aria-label='Message conversations'><header><h2>Conversations</h2>{compose}</header>"
    );
    if peers.is_empty() {
        html.push_str("<p class='muted account-empty'>No conversations yet.</p>");
    } else {
        html.push_str("<ul>");
        for (peer, info) in peers {
            let active = if selected == Some(peer) {
                " aria-current='page'"
            } else {
                ""
            };
            let unread = match info.unread {
                0 => String::new(),
                count => format!("<small class='account-unread'>{count} unread</small>"),
            };
            write!(
                html,
                "<li><a href='{}'{active}><strong>u/{}</strong>{unread}</a></li>",
                escape(&inbox_url(peer)),
                escape(peer)
            )
            .unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</nav>");
    html
}

pub(super) fn visible_notifications(state: &State) -> impl Iterator<Item = &Notification> {
    state.notifications.iter().filter(|item| {
        item.recipient == state.username
            && !item.silent
            && item.post.is_none_or(|id| visible_post(state, id).is_some())
            && state.can_view_discussion_notification(item)
            && state.can_view_mod_mail_notification(item.id)
    })
}

pub(super) fn notifications(state: &State, moderation_only: bool) -> String {
    let mut items: Vec<_> = if moderation_only {
        state
            .notifications
            .iter()
            .filter(|item| {
                item.recipient == state.username
                    && (item.appeal || item.moderation)
                    && state.can_view_mod_mail_notification(item.id)
            })
            .collect()
    } else {
        visible_notifications(state).collect()
    };
    items.sort_by_key(|item| Reverse((item.at, item.id)));
    let read = if !moderation_only && items.iter().any(|item| !item.read) {
        action_form(
            "read-notifications",
            NOTIFICATIONS,
            &[],
            "Mark all as read",
            "check",
            "button light account-read-form",
        )
    } else {
        String::new()
    };
    let selected = " aria-current='page'";
    let (all_current, moderation_current) = if moderation_only {
        ("", selected)
    } else {
        (selected, "")
    };
    let mut html = format!("<section class='panel standalone account-notifications'><header class='account-notifications-header'><h1>Notifications</h1><div class='account-notifications-actions'><a class='button light' href='/account/settings'>{} Settings</a>{read}</div></header><nav class='account-notifications-nav' aria-label='Notification history'><a href='/notifications'{all_current}>All notifications</a><a href='/notifications?view=moderation'{moderation_current}>Moderation updates</a></nav>", icon("sliders"));
    if items.is_empty() {
        let empty = if moderation_only {
            "No moderation updates yet."
        } else {
            "No notifications yet."
        };
        write!(html, "<p class='muted account-empty'>{empty}</p>").unwrap();
    } else {
        html.push_str("<ul class='account-notification-list'>");
        for item in items {
            let unread = if item.silent {
                "<span class='flair'>Muted</span>"
            } else if item.read {
                ""
            } else {
                "<span class='account-unread'>Unread</span>"
            };
            let link = if let Some(episode) = item.removal {
                removal_notification_link(state, episode)
            } else if item.appeal {
                "<a class='subtle-link' href='/appeals'>View your appeals</a>".to_owned()
            } else if let Some(id) = state.mod_mail_notification_target(item.id) {
                super::modmail::notification_link(state, id)
            } else if item.moderation {
                if (item.body.starts_with("Moderators removed your ")
                    || item.body.starts_with("AutoMod removed your "))
                    && state
                        .content_removals
                        .iter()
                        .any(|removal| state.can_appeal_removal(removal))
                {
                    "<a class='subtle-link' href='/appeals#appealable-removals'>View removals you can appeal</a>".to_owned()
                } else {
                    String::new()
                }
            } else if let Some(id) = item.post {
                let (anchor, label) = match item.discussion {
                    Some(DiscussionNotification::Reply(comment)) => {
                        (format!("#comment-{comment}"), "View reply")
                    }
                    Some(DiscussionNotification::MentionComment(comment)) => {
                        (format!("#comment-{comment}"), "View mentioned comment")
                    }
                    Some(DiscussionNotification::MentionPost) => {
                        (String::new(), "View mentioned post")
                    }
                    None => (String::new(), "View discussion"),
                };
                format!("<a class='subtle-link' href='/thread?id={id}{anchor}'>{label}</a>")
            } else {
                "<a class='subtle-link' href='/messages'>Open messages</a>".to_owned()
            };
            let body_style = if item.moderation {
                " class='account-message-body'"
            } else {
                ""
            };
            write!(
                html,
                "<li class='account-notification'><div class='account-notification-meta'>{unread}{}</div><p{body_style}>{}</p>{link}</li>",
                time(item.at),
                escape(&item.body)
            )
            .unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

fn removal_notification_link(state: &State, episode: u64) -> String {
    if let Some(appeal) = state
        .content_appeals
        .iter()
        .find(|appeal| appeal.removal == episode && appeal.appellant == state.username)
    {
        format!(
            "<a class='subtle-link' href='/appeals#removal-appeal-{}'>View your appeal</a>",
            appeal.id
        )
    } else if state
        .removal_for_author(episode)
        .is_some_and(|removal| state.can_appeal_removal(removal))
    {
        format!("<a class='subtle-link' href='/appeals#removal-{episode}'>Appeal removal</a>")
    } else {
        String::new()
    }
}

fn time(at: u64) -> String {
    format!(
        "<time datetime='{}Z'>{}</time>",
        to_local(at, "UTC"),
        escape(&display(at, "America/Chicago"))
    )
}

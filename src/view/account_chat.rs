use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::fmt::Write;

use super::super::{action_fields, action_form, escape};
use super::{inbox_url, time, INBOX};
use crate::model::{ChatRequest, ChatRequestPermission, State};

const REQUESTS: &str = "/messages?view=requests";

pub(super) fn settings(current: ChatRequestPermission) -> String {
    let choices: String = [
        (ChatRequestPermission::Everyone, "Anyone with an account"),
        (
            ChatRequestPermission::Verified,
            "Accounts with verified email",
        ),
        (ChatRequestPermission::Nobody, "Nobody"),
    ]
    .into_iter()
    .map(|(permission, label)| {
        let value = permission.as_str();
        let selected = if value == current.as_str() {
            " selected"
        } else {
            ""
        };
        format!("<option value='{value}'{selected}>{label}</option>")
    })
    .collect();
    format!(
        r#"<section class="chat-request-settings" aria-labelledby="chat-request-settings-title"><h2 id="chat-request-settings-title">Chat requests</h2><p>You can keep messaging people in existing conversations. Pending requests stay in <a class="subtle-link" href="{REQUESTS}">Requests</a>.</p>
<form class="stack-form chat-request-settings-form" action="/account/settings/chat" method="post" aria-labelledby="chat-request-settings-title"><label>Who can send you new chat requests<select name="chat_requests">{choices}</select></label><button type="submit" class="button dark">Save chat settings</button></form></section>"#
    )
}

pub(super) fn tabs(state: &State, requests: bool) -> String {
    let current = " aria-current='page'";
    let (conversations_current, requests_current) = if requests {
        ("", current)
    } else {
        (current, "")
    };
    let count = incoming(state).count();
    let (badge, label) = if count == 0 {
        (String::new(), String::new())
    } else {
        (
            format!("<span class='account-unread' aria-hidden='true'>{count}</span>"),
            format!(" aria-label='Requests, {count} pending'"),
        )
    };
    format!("<nav class='account-message-tabs' aria-label='Messages'><a href='{INBOX}'{conversations_current}>Conversations</a><a href='{REQUESTS}'{requests_current}{label}>Requests{badge}</a></nav>")
}

pub(super) fn compose(
    state: &State,
    contacts: &BTreeSet<&str>,
    selected: Option<&str>,
    draft: &str,
) -> String {
    let Some(peer) = selected else {
        if contacts.is_empty() {
            return "<p class='muted account-empty'>You can send a message when another account joins ThreadPorch.</p>".into();
        }
        let mut options = String::from("<option value=''>Choose a recipient</option>");
        for &peer in contacts {
            let peer = escape(peer);
            write!(options, "<option value='{peer}'>u/{peer}</option>").unwrap();
        }
        return format!("<form class='stack-form account-peer-form' action='{INBOX}' method='get'><label>Recipient<select name='peer' required>{options}</select></label><button class='button dark' type='submit'>Open chat</button></form>");
    };
    if !contacts.contains(peer) {
        return String::new();
    }
    let pending = if incoming(state).any(|request| request.sender == peer) {
        "You have an incoming chat request from this account."
    } else if outgoing(state).any(|request| request.recipient == peer) {
        "Your chat request is pending."
    } else {
        ""
    };
    if !pending.is_empty() {
        return format!("<p class='account-chat-pending'>{pending} <a class='subtle-link' href='{REQUESTS}'>View Requests</a></p>");
    }

    let (action, label, help) = if state.has_chat_conversation(peer) {
        ("send-direct-message", "Send message", "")
    } else {
        (
            "request-chat",
            "Send request",
            "<p class='muted'>Send a request to start a conversation.</p>",
        )
    };
    let fields = action_fields(action, &inbox_url(peer), &[("recipient", peer)]);
    format!("<form class='stack-form account-message-form' action='/action' method='post'>{fields}{help}<label>Message<textarea name='body' rows='4' maxlength='4000' required>{}</textarea></label><button class='button dark' type='submit'>{label}</button></form>", escape(draft))
}

pub(super) fn requests(state: &State) -> String {
    let incoming = request_list(incoming(state).collect(), true);
    let outgoing = request_list(outgoing(state).collect(), false);
    format!("<section class='panel standalone account-chat-requests'><h1>Messages</h1>{}<div class='account-chat-request-groups'>{incoming}{outgoing}</div></section>", tabs(state, true))
}

fn incoming(state: &State) -> impl Iterator<Item = &ChatRequest> {
    state
        .incoming_chat_requests()
        .filter(move |request| request.recipient == state.username)
}

fn outgoing(state: &State) -> impl Iterator<Item = &ChatRequest> {
    state
        .outgoing_chat_requests()
        .filter(move |request| request.sender == state.username)
}

fn request_list(mut requests: Vec<&ChatRequest>, incoming: bool) -> String {
    let (class, title, empty) = if incoming {
        ("incoming", "Incoming requests", "No incoming requests.")
    } else {
        ("sent", "Sent requests", "No pending sent requests.")
    };
    let mut html = format!("<section class='account-chat-{class}' aria-labelledby='chat-{class}-title'><h2 id='chat-{class}-title'>{title}</h2>");
    if requests.is_empty() {
        write!(html, "<p class='muted account-empty'>{empty}</p>").unwrap();
    } else {
        requests.sort_by_key(|request| Reverse((request.at, request.id)));
        html.push_str("<ul class='account-chat-request-list'>");
        for request in requests {
            let peer = if incoming {
                &request.sender
            } else {
                &request.recipient
            };
            write!(html, "<li class='account-chat-request'><header><a class='subtle-link' href='{}'>u/{}</a>{}</header><p class='account-message-body'>{}</p>", escape(&inbox_url(peer)), escape(peer), time(request.at), escape(&request.body)).unwrap();
            if incoming {
                let id = request.id.to_string();
                let fields = [("id", id.as_str())];
                let accept = action_form(
                    "accept-chat-request",
                    REQUESTS,
                    &fields,
                    "Accept",
                    "",
                    "button dark",
                );
                let decline = action_form(
                    "decline-chat-request",
                    REQUESTS,
                    &fields,
                    "Decline",
                    "",
                    "button light",
                );
                write!(html, "<div class='form-actions account-chat-request-actions'>{accept}{decline}</div>").unwrap();
            } else {
                html.push_str("<span class='flair'>Pending</span>");
            }
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

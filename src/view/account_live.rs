use std::collections::BTreeMap;
use std::fmt::Write;

use crate::http::Response;
use crate::model::{DirectMessage, State};

use super::{action_form, escape, field, inbox_url, other_participant, time};

const LIVE_BATCH_LIMIT: usize = 64;

pub(crate) fn live_messages(
    state: &State,
    viewer: Option<&str>,
    query: &BTreeMap<String, String>,
) -> Response {
    let Some(viewer) =
        viewer.filter(|handle| !handle.is_empty() && *handle == field(query, "viewer"))
    else {
        return Response::text(
            401,
            "text/plain; charset=utf-8",
            "Sign in to view messages.",
        );
    };
    let cursor = field(query, "after");
    let Some(after) = cursor
        .parse::<u64>()
        .ok()
        .filter(|_| cursor.bytes().all(|byte| byte.is_ascii_digit()))
    else {
        return Response::text(400, "text/plain; charset=utf-8", "Invalid message cursor.");
    };
    let peer = field(query, "peer").trim().to_ascii_lowercase();
    let mut messages = conversation_messages(state, viewer, &peer).peekable();
    if peer.is_empty() || peer == viewer || messages.peek().is_none() {
        return Response::text(
            404,
            "text/plain; charset=utf-8",
            "Conversation unavailable.",
        );
    }
    let mut unread = 0;
    let mut newer = Vec::new();
    for message in messages {
        unread += usize::from(unread_by(message, viewer));
        if message.id > after {
            newer.push(message);
        }
    }
    newer.sort_unstable_by_key(|message| message.id);
    newer.truncate(LIVE_BATCH_LIMIT);
    let Some(last) = newer.last() else {
        return Response::text(204, "text/plain; charset=utf-8", "");
    };

    let mut html = format!(
        "<template data-live-cursor='{}' data-live-unread='{unread}'>",
        last.id,
    );
    for message in newer {
        append_message(&mut html, message, viewer);
    }
    html.push_str("</template>");
    Response::html(html)
}

pub(super) fn conversation(state: &State, peer: &str) -> String {
    let viewer = state.username.as_str();
    let mut messages: Vec<_> = conversation_messages(state, viewer, peer).collect();
    messages.sort_unstable_by_key(|message| message.id);
    let peer_label = escape(peer);
    let Some(last) = messages.last() else {
        return format!("<section class='account-conversation'><header><h2>u/{peer_label}</h2></header></section>");
    };
    let cursor = last.id;
    let viewer_label = escape(viewer);
    let unread = messages
        .iter()
        .filter(|message| unread_by(message, viewer))
        .count();
    let read_form = action_form(
        "read-conversation",
        &inbox_url(peer),
        &[("peer", peer)],
        "Mark conversation as read",
        "check",
        "button light account-read-form",
    );
    let hidden = if unread == 0 { " hidden" } else { "" };
    let mut html = format!(
        "<section class='account-conversation' data-live-chat data-live-viewer='{viewer_label}' data-live-peer='{peer_label}' data-live-cursor='{cursor}' data-live-unread='{unread}'>\
         <header><h2>u/{peer_label}</h2><div data-live-read{hidden}>{read_form}</div></header>\
         <p class='sr-only' role='status' data-live-status></p>\
         <div role='log' aria-label='Conversation messages' aria-live='polite' aria-relevant='additions'><ul class='account-message-list' data-live-messages>"
    );
    for message in messages {
        append_message(&mut html, message, viewer);
    }
    html.push_str("</ul></div><script type='module' src='/chat-live.js'></script></section>");
    html
}

fn conversation_messages<'a>(
    state: &'a State,
    viewer: &'a str,
    peer: &'a str,
) -> impl Iterator<Item = &'a DirectMessage> + 'a {
    state
        .messages
        .iter()
        .filter(move |message| other_participant(message, viewer) == Some(peer))
}

fn unread_by(message: &DirectMessage, viewer: &str) -> bool {
    message.to == viewer && !message.read
}

fn append_message(html: &mut String, message: &DirectMessage, viewer: &str) {
    let (style, sender) = if message.from == viewer {
        (" account-message-own", "You".to_owned())
    } else {
        ("", format!("u/{}", escape(&message.from)))
    };
    write!(
        html,
        "<li class='account-message{style}' data-message-id='{}'><div class='account-message-meta'><strong>{sender}</strong>{}</div><p class='account-message-body'>{}</p></li>",
        message.id,
        time(message.at),
        escape(&message.body)
    )
    .unwrap();
}

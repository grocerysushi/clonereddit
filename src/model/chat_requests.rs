use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use super::{
    checked_handle, decode, encode, id_field, next_id, number, required, unix_seconds, value,
    DirectMessage, State,
};

const REQUEST_UNAVAILABLE: &str = "This account is unavailable for chat requests.";
const REQUEST_NOT_FOUND: &str = "This chat request is unavailable.";
const OPENING_MESSAGE_LIMIT: usize = 4_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChatRequestPermission {
    #[default]
    Everyone,
    Verified,
    Nobody,
}

impl ChatRequestPermission {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "everyone" => Some(Self::Everyone),
            "verified" => Some(Self::Verified),
            "nobody" => Some(Self::Nobody),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Everyone => "everyone",
            Self::Verified => "verified",
            Self::Nobody => "nobody",
        }
    }

    fn allows(self, sender_verified: bool) -> bool {
        match self {
            Self::Everyone => true,
            Self::Verified => sender_verified,
            Self::Nobody => false,
        }
    }
}

#[derive(Clone)]
pub struct ChatRequest {
    pub id: u64,
    pub sender: String,
    pub recipient: String,
    pub body: String,
    pub at: u64,
}

impl State {
    pub fn chat_request_permission(&self) -> ChatRequestPermission {
        self.chat_request_permission
    }

    pub fn set_chat_request_permission(&mut self, permission: ChatRequestPermission) {
        if !self.username.is_empty() {
            self.chat_request_permission = permission;
        }
    }

    pub fn has_chat_conversation(&self, peer: &str) -> bool {
        !self.username.is_empty()
            && self
                .messages
                .iter()
                .any(|message| same_pair(&self.username, peer, &message.from, &message.to))
    }

    pub fn incoming_chat_requests(&self) -> impl Iterator<Item = &ChatRequest> {
        self.chat_requests
            .iter()
            .filter(|request| request.recipient == self.username)
    }

    pub fn outgoing_chat_requests(&self) -> impl Iterator<Item = &ChatRequest> {
        self.chat_requests
            .iter()
            .filter(|request| request.sender == self.username)
    }

    pub fn request_chat(
        &mut self,
        form: &BTreeMap<String, String>,
        sender_verified: bool,
        path: &Path,
    ) -> Result<&'static str, String> {
        if self.username.is_empty() {
            return Err("Sign in to continue.".into());
        }
        let recipient = checked_handle(value(form, "recipient"))?;
        if recipient == self.username || self.either_blocked(&self.username, &recipient) {
            return Err(REQUEST_UNAVAILABLE.into());
        }
        if self.has_chat_conversation(&recipient) {
            return Err("You already have an open conversation with this account.".into());
        }
        if self.chat_requests.iter().any(|request| {
            same_pair(
                &self.username,
                &recipient,
                &request.sender,
                &request.recipient,
            )
        }) {
            return Err("A chat request is already pending for this conversation.".into());
        }
        let permission = self
            .account_profiles
            .get(&recipient)
            .map_or(ChatRequestPermission::default(), |profile| {
                profile.chat_request_permission
            });
        if !permission.allows(sender_verified)
            || self
                .declined_chat_requests
                .contains(&(self.username.clone(), recipient.clone()))
        {
            return Err(REQUEST_UNAVAILABLE.into());
        }

        let body = required(form, "body", "Opening message", OPENING_MESSAGE_LIMIT)?;
        let mut updated = self.clone();
        let id = next_id(&mut updated.next_chat_request)?;
        updated.chat_requests.push(ChatRequest {
            id,
            sender: self.username.clone(),
            recipient,
            body: body.into(),
            at: unix_seconds(),
        });
        self.commit(updated, path)?;
        Ok("Chat request sent.")
    }

    pub(super) fn resolve_chat_request(
        &mut self,
        form: &BTreeMap<String, String>,
        accept: bool,
    ) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .chat_requests
            .iter()
            .position(|request| request.id == id && request.recipient == self.username)
            .ok_or(REQUEST_NOT_FOUND)?;
        let request = &self.chat_requests[index];
        if accept
            && (self.either_blocked(&request.sender, &request.recipient)
                || self.has_chat_conversation(&request.sender))
        {
            return Err(REQUEST_NOT_FOUND.into());
        }
        let request = self.chat_requests.remove(index);
        if !accept {
            self.declined_chat_requests
                .insert((request.sender, request.recipient));
            return Ok("Chat request declined.");
        }
        let id = next_id(&mut self.next_message)?;
        self.messages.push(DirectMessage {
            id,
            from: request.sender,
            to: request.recipient,
            body: request.body,
            at: request.at,
            read: true,
        });
        Ok("Chat request accepted.")
    }

    pub(super) fn cancel_chat_requests_with(&mut self, peer: &str) {
        self.chat_requests.retain(|request| {
            !same_pair(&self.username, peer, &request.sender, &request.recipient)
        });
    }

    pub(super) fn delete_account_chat_requests(&mut self, handle: &str) {
        self.chat_requests
            .retain(|request| request.sender != handle && request.recipient != handle);
        self.declined_chat_requests
            .retain(|(sender, recipient)| sender != handle && recipient != handle);
    }

    pub(super) fn read_chat_request_record(&mut self, fields: &[&str]) -> Option<()> {
        match fields {
            ["chat-request-sequence", next] => {
                self.next_chat_request = number(next)?;
                if self.next_chat_request == 0 {
                    return None;
                }
            }
            ["chat-request", id, sender, recipient, body, at] => {
                let id = number(id)?;
                let (sender, recipient) = decode_pair(sender, recipient)?;
                let body = decode(body)?;
                if id == 0
                    || id >= self.next_chat_request
                    || body.trim().is_empty()
                    || body.chars().count() > OPENING_MESSAGE_LIMIT
                    || self.chat_requests.iter().any(|request| {
                        request.id == id
                            || same_pair(&sender, &recipient, &request.sender, &request.recipient)
                    })
                {
                    return None;
                }
                self.chat_requests.push(ChatRequest {
                    id,
                    sender,
                    recipient,
                    body,
                    at: number(at)?,
                });
            }
            ["declined-chat-request", sender, recipient] => {
                if !self
                    .declined_chat_requests
                    .insert(decode_pair(sender, recipient)?)
                {
                    return None;
                }
            }
            _ => return None,
        }
        Some(())
    }

    pub(super) fn write_chat_request_records(&self, file: &mut impl Write) -> io::Result<()> {
        if self.next_chat_request > 1 {
            writeln!(file, "chat-request-sequence\t{}", self.next_chat_request)?;
        }
        for request in &self.chat_requests {
            writeln!(
                file,
                "chat-request\t{}\t{}\t{}\t{}\t{}",
                request.id,
                encode(&request.sender),
                encode(&request.recipient),
                encode(&request.body),
                request.at
            )?;
        }
        for (sender, recipient) in &self.declined_chat_requests {
            writeln!(
                file,
                "declined-chat-request\t{}\t{}",
                encode(sender),
                encode(recipient)
            )?;
        }
        Ok(())
    }
}

fn same_pair(sender: &str, recipient: &str, first: &str, second: &str) -> bool {
    (sender == first && recipient == second) || (sender == second && recipient == first)
}

fn decode_pair(encoded_sender: &str, encoded_recipient: &str) -> Option<(String, String)> {
    let sender = decode(encoded_sender)?;
    let recipient = decode(encoded_recipient)?;
    if sender == recipient
        || checked_handle(&sender).ok()? != sender
        || checked_handle(&recipient).ok()? != recipient
    {
        return None;
    }
    Some((sender, recipient))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use super::*;

    const ALICE: &str = "chat_test_alice";
    const BOB: &str = "chat_test_bob";
    const CAROL: &str = "chat_test_carol";

    struct StateFile(PathBuf);

    impl StateFile {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "commonline-chat-requests-{}-{}.state",
                std::process::id(),
                SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos(),
            )))
        }
    }

    impl Drop for StateFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|&(key, value)| (key.into(), value.into()))
            .collect()
    }

    #[test]
    fn only_the_recipient_can_accept_and_acceptance_is_persistent() {
        let fixture = StateFile::new();
        let mut state = State::seeded();
        state.activate_profile(Some(BOB));
        state.set_chat_request_permission(ChatRequestPermission::Verified);
        state.activate_profile(Some(ALICE));
        let intro = form(&[("recipient", BOB), ("body", "Private introduction")]);
        assert!(state
            .apply("send-direct-message", &intro, &fixture.0)
            .is_err());
        assert!(state.request_chat(&intro, false, &fixture.0).is_err());
        state.request_chat(&intro, true, &fixture.0).unwrap();
        assert!(state.request_chat(&intro, true, &fixture.0).is_err());
        let outgoing = state.outgoing_chat_requests().next().unwrap();
        let id = outgoing.id.to_string();
        let sent_at = outgoing.at;
        let request = form(&[("id", &id)]);
        assert!(state.messages.is_empty());
        assert!(state
            .apply("accept-chat-request", &request, &fixture.0)
            .is_err());

        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some(CAROL));
        assert_eq!(state.incoming_chat_requests().count(), 0);
        assert_eq!(state.outgoing_chat_requests().count(), 0);
        assert!(state
            .apply("decline-chat-request", &request, &fixture.0)
            .is_err());
        state.activate_profile(Some(BOB));
        assert_eq!(
            state.chat_request_permission(),
            ChatRequestPermission::Verified
        );
        assert!(state
            .request_chat(
                &form(&[("recipient", ALICE), ("body", "Reverse request")]),
                true,
                &fixture.0
            )
            .is_err());
        state
            .apply("accept-chat-request", &request, &fixture.0)
            .unwrap();
        assert!(state
            .apply("accept-chat-request", &request, &fixture.0)
            .is_err());
        assert!(state.has_chat_conversation(ALICE));
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].from, ALICE);
        assert_eq!(state.messages[0].to, BOB);
        assert_eq!(state.messages[0].body, "Private introduction");
        assert_eq!(state.messages[0].at, sent_at);

        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some(BOB));
        state.set_chat_request_permission(ChatRequestPermission::Nobody);
        state.activate_profile(Some(ALICE));
        assert!(state.has_chat_conversation(BOB));
        assert_eq!(state.outgoing_chat_requests().count(), 0);
        state
            .apply(
                "send-direct-message",
                &form(&[("recipient", BOB), ("body", "Later message")]),
                &fixture.0,
            )
            .unwrap();
        assert_eq!(state.messages.len(), 2);

        state.activate_profile(Some(CAROL));
        assert!(state
            .request_chat(
                &form(&[("recipient", BOB), ("body", "Not accepted")]),
                true,
                &fixture.0
            )
            .is_err());
    }

    #[test]
    fn declining_erases_the_intro_and_a_block_or_deletion_cancels_pending_requests() {
        let fixture = StateFile::new();
        let intro = form(&[("recipient", BOB), ("body", "DiscardedIntroduction")]);
        let reverse = form(&[("recipient", ALICE), ("body", "DiscardedReverse")]);
        let mut state = State::seeded();
        state.activate_profile(Some(ALICE));
        assert_eq!(
            state.chat_request_permission(),
            ChatRequestPermission::Everyone
        );
        state.request_chat(&intro, false, &fixture.0).unwrap();
        state.activate_profile(Some(BOB));
        let id = state
            .incoming_chat_requests()
            .next()
            .unwrap()
            .id
            .to_string();
        state
            .apply("decline-chat-request", &form(&[("id", &id)]), &fixture.0)
            .unwrap();
        assert_eq!(state.incoming_chat_requests().count(), 0);
        assert!(!state.has_chat_conversation(ALICE));
        assert!(!fs::read_to_string(&fixture.0)
            .unwrap()
            .contains("DiscardedIntroduction"));

        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some(ALICE));
        assert!(state.request_chat(&intro, false, &fixture.0).is_err());
        state.activate_profile(Some(BOB));
        state.request_chat(&reverse, false, &fixture.0).unwrap();
        state.activate_profile(Some(ALICE));
        let id = state
            .incoming_chat_requests()
            .next()
            .unwrap()
            .id
            .to_string();
        let block = form(&[("author", BOB)]);
        state.apply("block", &block, &fixture.0).unwrap();
        assert_eq!(state.incoming_chat_requests().count(), 0);
        assert!(state
            .apply("accept-chat-request", &form(&[("id", &id)]), &fixture.0)
            .is_err());
        state.activate_profile(Some(BOB));
        assert!(state.request_chat(&reverse, false, &fixture.0).is_err());
        state.activate_profile(Some(ALICE));
        state.apply("block", &block, &fixture.0).unwrap();
        state.activate_profile(Some(BOB));
        state.request_chat(&reverse, false, &fixture.0).unwrap();

        state.delete_account_profile(BOB);
        state.save(&fixture.0).unwrap();
        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some(ALICE));
        assert_eq!(state.outgoing_chat_requests().count(), 0);
        assert_eq!(state.incoming_chat_requests().count(), 0);
        assert!(state.declined_chat_requests.is_empty());
        assert!(!fs::read_to_string(&fixture.0)
            .unwrap()
            .contains("DiscardedReverse"));
    }

    #[test]
    fn an_existing_message_pair_can_chat_without_request_metadata() {
        let fixture = StateFile::new();
        let mut state = State::seeded();
        state.activate_profile(Some(BOB));
        state.messages.push(DirectMessage {
            id: next_id(&mut state.next_message).unwrap(),
            from: ALICE.into(),
            to: BOB.into(),
            body: "Message from an older snapshot".into(),
            at: 1,
            read: true,
        });
        state.save(&fixture.0).unwrap();
        assert!(!fs::read_to_string(&fixture.0)
            .unwrap()
            .contains("chat-request-sequence"));

        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some(ALICE));
        assert!(state.has_chat_conversation(BOB));
        state
            .apply(
                "send-direct-message",
                &form(&[("recipient", BOB), ("body", "Still connected")]),
                &fixture.0,
            )
            .unwrap();
        assert_eq!(state.messages.len(), 2);
    }
}

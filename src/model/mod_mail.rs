use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    bounded, canonical_u64, decode, encode, id_field, next_id, required, unix_seconds, value,
    Community, ModPermission, State, DELETED_ACCOUNT,
};

const BODY_LIMIT: usize = 4_000;

#[derive(Clone)]
pub struct ModMail {
    pub id: u64,
    pub community: String,
    pub sender: String,
    pub subject: String,
    pub body: String,
    pub started_at: u64,
    pub replies: Vec<ModMailReply>,
    pub archived: bool,
}

#[derive(Clone)]
pub struct ModMailReply {
    pub author: String,
    pub body: String,
    pub at: u64,
    pub moderator: bool,
}

impl ModMail {
    pub fn last_activity(&self) -> (u64, u64) {
        (
            self.replies
                .last()
                .map_or(self.started_at, |reply| reply.at),
            self.id,
        )
    }
}

impl State {
    pub fn visible_mod_mail(&self, id: u64) -> Option<&ModMail> {
        let thread = self.mod_mail.iter().find(|thread| thread.id == id)?;
        let community = self.mod_mail_community(thread)?;
        (self.can_moderate(community, ModPermission::Mail)
            || self.can_read_sent_mod_mail(community, thread))
        .then_some(thread)
    }

    pub fn mod_mail_community(&self, thread: &ModMail) -> Option<&Community> {
        self.communities
            .iter()
            .find(|community| community.slug == thread.community && community.owner.is_some())
    }

    pub fn can_read_sent_mod_mail(&self, community: &Community, thread: &ModMail) -> bool {
        !self.guest()
            && community.slug == thread.community
            && thread.sender == self.username
            && self.can_view_community(community)
    }

    pub fn mod_mail_notification_target(&self, id: u64) -> Option<u64> {
        self.mod_mail_notifications.get(&id).copied()
    }

    pub fn can_view_mod_mail_notification(&self, id: u64) -> bool {
        self.mod_mail_notification_target(id)
            .is_none_or(|thread| self.visible_mod_mail(thread).is_some())
    }

    pub(super) fn send_mod_mail(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.community_index(form)?;
        let community = &self.communities[index];
        if community.owner.is_none() || !self.can_view_community(community) {
            return Err("This community does not have a moderation inbox.".into());
        }
        if self.is_mod_mail_muted(&community.slug, &self.username) {
            return Err("You are muted in this community’s mod mail.".into());
        }
        let subject = required(form, "subject", "Subject", 120)?;
        let body = required(form, "body", "Message", BODY_LIMIT)?;
        let id = next_id(&mut self.next_mod_mail)?;
        let community = community.slug.clone();
        self.mod_mail.push(ModMail {
            id,
            community: community.clone(),
            sender: self.username.clone(),
            subject: subject.into(),
            body: body.into(),
            started_at: unix_seconds(),
            replies: Vec::new(),
            archived: false,
        });
        self.notify_mod_mail_team(&community, id)?;
        Ok("Message sent to the moderation team.")
    }

    pub(super) fn reply_mod_mail(
        &mut self,
        form: &BTreeMap<String, String>,
        moderator: bool,
    ) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .mod_mail
            .iter()
            .position(|thread| thread.id == id)
            .ok_or("This mod mail thread could not be found.")?;
        let thread = &self.mod_mail[index];
        let community = self
            .mod_mail_community(thread)
            .ok_or("This mod mail thread could not be found.")?;
        let allowed = if moderator {
            self.can_moderate(community, ModPermission::Mail)
        } else {
            self.can_read_sent_mod_mail(community, thread)
        };
        if !allowed || !matches_community(form, &thread.community) {
            return Err("This mod mail thread could not be found.".into());
        }
        if thread.archived {
            return Err("This thread is archived. A moderator can reopen it.".into());
        }
        if !moderator && self.is_mod_mail_muted(&thread.community, &self.username) {
            return Err("You are muted in this community’s mod mail.".into());
        }
        if thread.sender == DELETED_ACCOUNT {
            return Err("This thread’s participant is unavailable.".into());
        }
        let reply = bounded(form, "reply", "Reply", BODY_LIMIT)?;
        let body = if moderator && reply.is_empty() {
            let response_id = canonical_u64(value(form, "response_id"))
                .ok_or("Write a reply or choose a saved response.")?;
            self.saved_responses
                .iter()
                .find(|item| item.id == response_id && item.community == thread.community)
                .map(|item| item.body.clone())
                .ok_or("This saved response is unavailable for this community.")?
        } else if reply.is_empty() {
            return Err("Write a reply.".into());
        } else {
            reply.into()
        };
        let recipient = thread.sender.clone();
        let community = thread.community.clone();
        self.mod_mail[index].replies.push(ModMailReply {
            author: self.username.clone(),
            body,
            at: unix_seconds(),
            moderator,
        });
        if moderator {
            self.notify_mod_mail(&recipient, id)?;
            self.record_moderation_action(&community, "Sent a mod mail reply")?;
        } else {
            self.notify_mod_mail_team(&community, id)?;
        }
        Ok("Mod mail reply sent.")
    }

    pub(super) fn set_mod_mail_archived(
        &mut self,
        form: &BTreeMap<String, String>,
        archived: bool,
    ) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .mod_mail
            .iter()
            .position(|thread| {
                thread.id == id
                    && matches_community(form, &thread.community)
                    && self
                        .mod_mail_community(thread)
                        .is_some_and(|community| self.can_moderate(community, ModPermission::Mail))
            })
            .ok_or("This mod mail thread could not be found.")?;
        let thread = &mut self.mod_mail[index];
        let notice = if archived {
            "Mod mail thread archived."
        } else {
            "Mod mail thread reopened."
        };
        if thread.archived != archived {
            thread.archived = archived;
            let community = thread.community.clone();
            self.record_moderation_action(&community, notice)?;
        }
        Ok(notice)
    }

    fn notify_mod_mail_team(&mut self, community: &str, id: u64) -> Result<(), String> {
        let Some(item) = self.communities.iter().find(|item| item.slug == community) else {
            return Ok(());
        };
        let recipients: Vec<_> = item
            .owner
            .iter()
            .chain(
                self.moderator_roles
                    .iter()
                    .filter(|entry| entry.community == community)
                    .map(|entry| &entry.handle),
            )
            .filter(|handle| self.can_moderate_as(item, handle, ModPermission::Mail))
            .cloned()
            .collect();
        for recipient in recipients {
            self.notify_mod_mail(&recipient, id)?;
        }
        Ok(())
    }

    fn notify_mod_mail(&mut self, recipient: &str, id: u64) -> Result<(), String> {
        if recipient == self.username || recipient.is_empty() || recipient == DELETED_ACCOUNT {
            return Ok(());
        }
        let notification = self.next_notification;
        self.push_moderation_notification(recipient, "There’s a new message in Mod mail.".into())?;
        self.mod_mail_notifications.insert(notification, id);
        Ok(())
    }

    pub(super) fn rename_mod_mail_account(&mut self, previous: &str, next: &str) {
        for thread in &mut self.mod_mail {
            if thread.sender == previous {
                thread.sender = next.into();
            }
            for reply in &mut thread.replies {
                if reply.author == previous {
                    reply.author = next.into();
                }
            }
        }
    }

    pub(super) fn read_mod_mail_record(&mut self, fields: &[&str]) -> Option<()> {
        match fields {
            ["mod-mail", id, community, sender, subject, body, reply, timestamp @ ..] => {
                let id = canonical_u64(id).filter(|&id| id > 0)?;
                if self.mod_mail.iter().any(|thread| thread.id == id) {
                    return None;
                }
                let reply = decode(reply)?;
                let started_at = match timestamp {
                    [] => 0,
                    [at] => canonical_u64(at)?,
                    _ => return None,
                };
                let replies = if reply.is_empty() {
                    Vec::new()
                } else {
                    vec![ModMailReply {
                        author: String::new(),
                        body: reply,
                        at: 0,
                        moderator: true,
                    }]
                };
                self.mod_mail.push(ModMail {
                    id,
                    community: decode(community)?,
                    sender: decode(sender)?,
                    subject: decode(subject)?,
                    body: decode(body)?,
                    started_at,
                    replies,
                    archived: false,
                });
                self.next_mod_mail = self.next_mod_mail.max(id.checked_add(1)?);
            }
            ["mod-mail-reply", id, kind, author, body, at] => {
                let id = canonical_u64(id)?;
                let thread = self.mod_mail.iter_mut().find(|thread| thread.id == id)?;
                let moderator = match *kind {
                    "moderator" => true,
                    "member" => false,
                    _ => return None,
                };
                let author = decode(author)?;
                let body = decode(body)?;
                let at = canonical_u64(at)?;
                if body.trim().is_empty()
                    || body.chars().count() > BODY_LIMIT
                    || (!moderator && author != thread.sender)
                    || (author.is_empty() && (!moderator || at != 0))
                {
                    return None;
                }
                thread.replies.push(ModMailReply {
                    author,
                    body,
                    at,
                    moderator,
                });
            }
            ["mod-mail-archive", id] => {
                let id = canonical_u64(id)?;
                let thread = self.mod_mail.iter_mut().find(|thread| thread.id == id)?;
                thread.archived = true;
            }
            ["mod-mail-notification", notification, thread] => {
                let notification = canonical_u64(notification)?;
                let thread = canonical_u64(thread)?;
                if !self
                    .notifications
                    .iter()
                    .any(|item| item.id == notification)
                    || !self.mod_mail.iter().any(|item| item.id == thread)
                    || self
                        .mod_mail_notifications
                        .insert(notification, thread)
                        .is_some()
                {
                    return None;
                }
            }
            _ => return None,
        }
        Some(())
    }

    pub(super) fn write_mod_mail_records(&self, file: &mut impl Write) -> io::Result<()> {
        for thread in &self.mod_mail {
            writeln!(
                file,
                "mod-mail\t{}\t{}\t{}\t{}\t{}\t\t{}",
                thread.id,
                encode(&thread.community),
                encode(&thread.sender),
                encode(&thread.subject),
                encode(&thread.body),
                thread.started_at
            )?;
            for reply in &thread.replies {
                let kind = if reply.moderator {
                    "moderator"
                } else {
                    "member"
                };
                writeln!(
                    file,
                    "mod-mail-reply\t{}\t{kind}\t{}\t{}\t{}",
                    thread.id,
                    encode(&reply.author),
                    encode(&reply.body),
                    reply.at
                )?;
            }
            if thread.archived {
                writeln!(file, "mod-mail-archive\t{}", thread.id)?;
            }
        }
        for (notification, thread) in &self.mod_mail_notifications {
            writeln!(file, "mod-mail-notification\t{notification}\t{thread}")?;
        }
        Ok(())
    }
}

fn matches_community(form: &BTreeMap<String, String>, community: &str) -> bool {
    form.get("community").is_none_or(|slug| slug == community)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const OWNER: &str = "mail_owner";
    const MEMBER: &str = "mail_member";
    const OUTSIDER: &str = "mail_other";
    const ROOM: &str = "private-mail";

    struct StateFile(PathBuf);

    impl StateFile {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "threadporch-mod-mail-{}-{}.state",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
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
            .map(|&(key, val)| (key.into(), val.into()))
            .collect()
    }

    fn private_community() -> State {
        let mut state = State::seeded();
        state.activate_profile(Some(OWNER));
        state
            .create_community(&form(&[
                ("slug", ROOM),
                ("name", "Private mail"),
                ("visibility", "private"),
            ]))
            .unwrap();
        state
            .approve_member(&form(&[("community", ROOM), ("handle", MEMBER)]))
            .unwrap();
        state
    }

    #[test]
    fn mod_mail_preserves_order_access_and_archive_after_restart() {
        let file = StateFile::new();
        let mut state = private_community();
        let contact = form(&[
            ("community", ROOM),
            ("subject", "Access question"),
            ("body", "First message"),
        ]);
        state.activate_profile(Some(OUTSIDER));
        assert!(state.apply("send-mod-mail", &contact, &file.0).is_err());
        state.activate_profile(Some(MEMBER));
        state.apply("send-mod-mail", &contact, &file.0).unwrap();
        let id = state.mod_mail[0].id;
        let id_value = id.to_string();
        let message = |reply| form(&[("id", &id_value), ("community", ROOM), ("reply", reply)]);
        assert!(state
            .apply("archive-mod-mail", &message(""), &file.0)
            .is_err());
        let owner_notice = state
            .notifications
            .iter()
            .find(|item| item.recipient == OWNER)
            .unwrap()
            .id;
        assert_eq!(state.mod_mail_notification_target(owner_notice), Some(id));

        state.activate_profile(Some(OWNER));
        assert!(state.can_view_mod_mail_notification(owner_notice));
        state
            .apply("reply-mod-mail", &message("First response"), &file.0)
            .unwrap();
        let member_notice = state
            .notifications
            .iter()
            .find(|item| item.recipient == MEMBER)
            .unwrap()
            .id;
        assert_eq!(state.mod_mail_notification_target(member_notice), Some(id));

        state.activate_profile(Some(OUTSIDER));
        assert!(state.visible_mod_mail(id).is_none());
        assert!(state
            .apply("respond-mod-mail", &message("Unrelated reply"), &file.0)
            .is_err());
        state.activate_profile(Some(MEMBER));
        assert!(state.can_view_mod_mail_notification(member_notice));
        state
            .apply("respond-mod-mail", &message("Another question"), &file.0)
            .unwrap();
        state.activate_profile(Some(OWNER));
        let wrong_community = form(&[
            ("id", &id_value),
            ("community", "community-other"),
            ("reply", "Wrong scope"),
        ]);
        assert!(state
            .apply("reply-mod-mail", &wrong_community, &file.0)
            .is_err());
        state
            .apply("reply-mod-mail", &message("Second response"), &file.0)
            .unwrap();
        state
            .apply("archive-mod-mail", &message(""), &file.0)
            .unwrap();

        let mut state = State::load(&file.0).unwrap();
        assert_eq!(state.mod_mail_notification_target(member_notice), Some(id));
        state.activate_profile(Some(MEMBER));
        let thread = state.visible_mod_mail(id).unwrap();
        assert!(thread.archived);
        assert_eq!(
            thread
                .replies
                .iter()
                .map(|reply| (reply.body.as_str(), reply.moderator))
                .collect::<Vec<_>>(),
            [
                ("First response", true),
                ("Another question", false),
                ("Second response", true)
            ],
        );
        assert!(state
            .apply("respond-mod-mail", &message("Archived"), &file.0)
            .is_err());
        state.activate_profile(Some(OWNER));
        assert!(state
            .apply("reply-mod-mail", &message("Archived"), &file.0)
            .is_err());
        state
            .apply("reopen-mod-mail", &message(""), &file.0)
            .unwrap();
        let mute = form(&[("community", ROOM), ("username", MEMBER), ("kind", "mute")]);
        state.apply("restrict-user", &mute, &file.0).unwrap();
        state.activate_profile(Some(MEMBER));
        assert!(state.visible_mod_mail(id).is_some());
        assert!(state
            .apply("respond-mod-mail", &message("Muted"), &file.0)
            .is_err());

        state.activate_profile(Some(OWNER));
        state
            .revoke_approved_member(&form(&[("community", ROOM), ("handle", MEMBER)]))
            .unwrap();
        state.activate_profile(Some(MEMBER));
        assert!(state.visible_mod_mail(id).is_none());
        assert!(!state.can_view_mod_mail_notification(member_notice));
    }

    #[test]
    fn legacy_mod_mail_reply_survives_next_message_and_account_cleanup() {
        let file = StateFile::new();
        let mut state = private_community();
        state.read_record("mod-mail\t7\tprivate-mail\tmail_member\tOlder subject\tOriginal message\tLegacy reply").unwrap();
        let second = form(&[("id", "7"), ("reply", "New response")]);
        state.apply("reply-mod-mail", &second, &file.0).unwrap();
        let mut state = State::load(&file.0).unwrap();
        state.activate_profile(Some(OWNER));
        let thread = state.visible_mod_mail(7).unwrap();
        assert_eq!(thread.body, "Original message");
        assert_eq!(thread.replies.len(), 2);
        assert_eq!(thread.replies[0].body, "Legacy reply");
        assert!(thread.replies[0].author.is_empty());
        assert_eq!(thread.replies[1].body, "New response");
        assert_eq!(thread.replies[1].author, OWNER);
        state.delete_account_profile(MEMBER);
        state.save(&file.0).unwrap();
        let state = State::load(&file.0).unwrap();
        assert_eq!(state.mod_mail[0].sender, DELETED_ACCOUNT);
        assert!(state.mod_mail_notifications.is_empty());
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use super::{
    checked_handle, encode, encode_image, next_id, required, unix_seconds, value,
    ChatRequestPermission, Comment, Community, CustomFeed, DirectMessage, Notification,
    NotificationPreferences, Post, PostDraft, ProfileAvatar, SocialLink, State, DELETED_ACCOUNT,
};

#[derive(Clone, Default)]
pub(super) struct ProfileData {
    pub(super) username: String,
    pub(super) votes: BTreeMap<u64, i8>,
    pub(super) comment_votes: BTreeMap<u64, i8>,
    pub(super) saved: BTreeSet<u64>,
    pub(super) hidden: BTreeSet<u64>,
    pub(super) joined: BTreeSet<String>,
    muted: BTreeSet<String>,
    blocked: BTreeSet<String>,
    keywords: Vec<String>,
    pub(super) sensitive: String,
    pub(super) custom_feeds: Vec<CustomFeed>,
    recent: Vec<String>,
    pub(super) history: Vec<u64>,
    display_name: String,
    bio: String,
    avatar: Option<ProfileAvatar>,
    cover: Option<ProfileAvatar>,
    public_posts: bool,
    public_comments: bool,
    nsfw: bool,
    pub(super) hidden_posts: BTreeSet<u64>,
    pub(super) hidden_comments: BTreeSet<u64>,
    flair: String,
    social_links: Vec<SocialLink>,
    created: u64,
    draft: Option<PostDraft>,
    pub(super) notification_preferences: NotificationPreferences,
    pub(super) chat_request_permission: ChatRequestPermission,
}

impl ProfileData {
    fn blank(username: &str) -> Self {
        let known = !username.is_empty();
        Self {
            username: username.into(),
            sensitive: "blur".into(),
            public_posts: known,
            public_comments: known,
            created: if known { unix_seconds() } else { 0 },
            ..Self::default()
        }
    }

    pub(super) fn capture(state: &State) -> Self {
        Self {
            username: state.username.clone(),
            votes: state.votes.clone(),
            comment_votes: state.comment_votes.clone(),
            saved: state.saved.clone(),
            hidden: state.hidden.clone(),
            joined: state.joined.clone(),
            muted: state.muted.clone(),
            blocked: state.blocked.clone(),
            keywords: state.keywords.clone(),
            sensitive: state.sensitive.clone(),
            custom_feeds: state.custom_feeds.clone(),
            recent: state.recent.clone(),
            history: state.history.clone(),
            display_name: state.display_name.clone(),
            bio: state.bio.clone(),
            avatar: state.avatar.clone(),
            cover: state.cover.clone(),
            public_posts: state.public_posts,
            public_comments: state.public_comments,
            nsfw: state.profile_nsfw,
            hidden_posts: state.profile_hidden_posts.clone(),
            hidden_comments: state.profile_hidden_comments.clone(),
            flair: state.profile_flair.clone(),
            social_links: state.social_links.clone(),
            created: state.profile_created,
            draft: state.draft.clone(),
            notification_preferences: state.notification_preferences,
            chat_request_permission: state.chat_request_permission,
        }
    }

    fn restore(self, state: &mut State) {
        state.username = self.username;
        state.votes = self.votes;
        state.comment_votes = self.comment_votes;
        state.saved = self.saved;
        state.hidden = self.hidden;
        state.joined = self.joined;
        state.muted = self.muted;
        state.blocked = self.blocked;
        state.keywords = self.keywords;
        state.sensitive = self.sensitive;
        state.custom_feeds = self.custom_feeds;
        state.recent = self.recent;
        state.history = self.history;
        state.display_name = self.display_name;
        state.bio = self.bio;
        state.avatar = self.avatar;
        state.cover = self.cover;
        state.public_posts = self.public_posts;
        state.public_comments = self.public_comments;
        state.profile_nsfw = self.nsfw;
        state.profile_hidden_posts = self.hidden_posts;
        state.profile_hidden_comments = self.hidden_comments;
        state.profile_flair = self.flair;
        state.social_links = self.social_links;
        state.profile_created = self.created;
        state.draft = self.draft;
        state.notification_preferences = self.notification_preferences;
        state.chat_request_permission = self.chat_request_permission;
    }

    pub(super) fn from_records(username: &str, sensitive: String, records: &str) -> Option<Self> {
        let mut profile = State::seeded();
        Self::blank(username).restore(&mut profile);
        profile.sensitive = sensitive;
        for line in records.lines() {
            match line.split('\t').next()? {
                "profile"
                | "profile-image"
                | "social-link"
                | "draft"
                | "notification-preferences"
                | "chat-request-permission"
                | "F"
                | "V"
                | "H"
                | "vote"
                | "comment-vote"
                | "saved"
                | "hidden"
                | "profile-hidden-post"
                | "profile-hidden-comment"
                | "joined"
                | "muted"
                | "blocked"
                | "keyword" => profile.read_record(line)?,
                _ => return None,
            };
        }
        Some(Self::capture(&profile))
    }

    pub(super) fn write_records(
        &self,
        file: &mut impl Write,
        next_social_link: u64,
    ) -> io::Result<()> {
        writeln!(
            file,
            "profile\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            encode(&self.display_name),
            encode(&self.bio),
            self.public_posts,
            self.public_comments,
            self.nsfw,
            encode(&self.flair),
            self.created,
            next_social_link
        )?;
        let prefs = self.notification_preferences;
        if prefs != NotificationPreferences::default() {
            writeln!(
                file,
                "notification-preferences\t{}\t{}\t{}",
                prefs.replies, prefs.messages, prefs.moderation,
            )?;
        }
        if self.chat_request_permission != ChatRequestPermission::default() {
            writeln!(
                file,
                "chat-request-permission\t{}",
                self.chat_request_permission.as_str()
            )?;
        }
        for (kind, image) in [("avatar", &self.avatar), ("cover", &self.cover)] {
            if let Some(image) = image {
                writeln!(
                    file,
                    "profile-image\t{kind}\t{}\t{}",
                    image.content_type,
                    encode_image(&image.bytes)
                )?;
            }
        }
        for link in &self.social_links {
            writeln!(
                file,
                "social-link\t{}\t{}\t{}",
                link.id,
                encode(&link.label),
                encode(&link.url)
            )?;
        }
        if let Some(draft) = &self.draft {
            writeln!(
                file,
                "draft\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                encode(&draft.community),
                encode(&draft.title),
                encode(&draft.body),
                encode(&draft.url),
                encode(&draft.flair),
                draft.sensitive,
                draft.spoiler,
                draft.brand_affiliate
            )?;
        }
        for feed in &self.custom_feeds {
            write!(
                file,
                "F\t{}\t{}\t@\t{}\t{}\t{}",
                feed.id,
                encode(&feed.name),
                encode(&feed.description),
                feed.private,
                feed.show_on_profile
            )?;
            for slug in &feed.communities {
                write!(file, "\t{}", encode(slug))?;
            }
            writeln!(file)?;
        }
        for slug in &self.recent {
            writeln!(file, "V\t{}", encode(slug))?;
        }
        for id in &self.history {
            writeln!(file, "H\t{id}")?;
        }
        for (label, votes) in [("vote", &self.votes), ("comment-vote", &self.comment_votes)] {
            for (id, vote) in votes {
                writeln!(file, "{label}\t{id}\t{vote}")?;
            }
        }
        for (label, ids) in [
            ("saved", &self.saved),
            ("hidden", &self.hidden),
            ("profile-hidden-post", &self.hidden_posts),
            ("profile-hidden-comment", &self.hidden_comments),
        ] {
            for id in ids {
                writeln!(file, "{label}\t{id}")?;
            }
        }
        for (label, values) in [
            ("joined", &self.joined),
            ("muted", &self.muted),
            ("blocked", &self.blocked),
        ] {
            for value in values {
                writeln!(file, "{label}\t{}", encode(value))?;
            }
        }
        for keyword in &self.keywords {
            writeln!(file, "keyword\t{}", encode(keyword))?;
        }
        Ok(())
    }
}

impl State {
    pub(super) fn write_account_records(&self, file: &mut impl Write) -> io::Result<()> {
        let profile = if self.guest() {
            ProfileData::blank("")
        } else {
            ProfileData::capture(self)
        };
        profile.write_records(file, self.next_social_link)?;
        if self.managed_profiles {
            writeln!(
                file,
                "account-mode\t{}\t{}",
                encode(&self.username),
                encode(self.legacy_profile.as_deref().unwrap_or_default())
            )?;
            for (handle, profile) in &self.account_profiles {
                let mut records = Vec::new();
                profile.write_records(&mut records, self.next_social_link)?;
                let records = std::str::from_utf8(&records)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                writeln!(
                    file,
                    "account-data\t{}\t{}\t{}",
                    encode(handle),
                    encode(&profile.sensitive),
                    encode(records)
                )?;
            }
        }
        Ok(())
    }

    pub fn activate_profile(&mut self, handle: Option<&str>) {
        let target = handle.and_then(|handle| checked_handle(handle).ok());
        if !self.managed_profiles {
            self.legacy_profile = (!self.username.is_empty()).then(|| self.username.clone());
            self.managed_profiles = true;
        }
        if target.as_deref() == Some(self.username.as_str()) {
            return;
        }
        if !self.username.is_empty() {
            self.account_profiles
                .insert(self.username.clone(), ProfileData::capture(self));
        }
        let profile = match target.as_deref() {
            Some(handle) => self
                .account_profiles
                .remove(handle)
                .unwrap_or_else(|| ProfileData::blank(handle)),
            None => ProfileData::blank(""),
        };
        profile.restore(self);
    }

    pub fn has_profile(&self, handle: &str) -> bool {
        checked_handle(handle)
            .is_ok_and(|name| self.username == name || self.account_profiles.contains_key(&name))
    }

    pub fn account_display_name(&self, handle: &str) -> Option<&str> {
        let name = if handle == self.username {
            self.display_name.as_str()
        } else {
            self.account_profiles.get(handle)?.display_name.as_str()
        };
        (!name.is_empty()).then_some(name)
    }

    pub fn profile_snapshot(&self, handle: &str) -> Option<Self> {
        let handle = checked_handle(handle).ok()?;
        let mut snapshot = self.clone();
        snapshot.activate_profile(Some(&handle));
        Some(snapshot)
    }

    pub fn legacy_username(&self) -> Option<&str> {
        if self.managed_profiles {
            self.legacy_profile.as_deref()
        } else {
            (!self.username.is_empty()).then_some(self.username.as_str())
        }
    }

    pub fn reserved_handle(&self, handle: &str) -> bool {
        checked_handle(handle).is_ok_and(|handle| {
            self.has_profile(&handle)
                || self.posts.iter().any(|post| post.author == handle)
                || self.comments.iter().any(|comment| comment.author == handle)
                || self
                    .communities
                    .iter()
                    .any(|item| item.owner.as_ref() == Some(&handle))
                || self.mod_mail.iter().any(|item| item.sender == handle)
                || self.reports.iter().any(|report| report.reporter == handle)
        })
    }

    pub fn post_score(&self, post: &Post) -> i32 {
        let current = self.votes.get(&post.id).copied().unwrap_or(0);
        self.account_profiles
            .values()
            .filter_map(|profile| profile.votes.get(&post.id))
            .fold(
                post.score.saturating_add(i32::from(current)),
                |score, vote| score.saturating_add(i32::from(*vote)),
            )
    }

    pub fn comment_score(&self, comment: &Comment) -> i32 {
        let current = self.comment_votes.get(&comment.id).copied().unwrap_or(0);
        self.account_profiles
            .values()
            .filter_map(|profile| profile.comment_votes.get(&comment.id))
            .fold(
                comment.score.saturating_add(i32::from(current)),
                |score, vote| score.saturating_add(i32::from(*vote)),
            )
    }

    pub fn community_member_count(&self, community: &Community) -> u32 {
        let demo_members = if community.owner.is_some() {
            0
        } else {
            community.members
        };
        demo_members.saturating_add(self.account_join_count(&community.slug))
    }

    pub(super) fn account_join_count(&self, slug: &str) -> u32 {
        let active = u32::from(self.joined.contains(slug));
        let others = self
            .account_profiles
            .values()
            .filter(|profile| profile.joined.contains(slug))
            .count();
        active.saturating_add(u32::try_from(others).unwrap_or(u32::MAX))
    }

    pub fn shared_custom_feed(&self, id: u64) -> Option<&CustomFeed> {
        self.custom_feeds
            .iter()
            .find(|item| item.id == id)
            .or_else(|| {
                self.account_profiles.values().find_map(|profile| {
                    profile
                        .custom_feeds
                        .iter()
                        .find(|item| item.id == id && !item.private)
                })
            })
    }

    pub(super) fn either_blocked(&self, left: &str, right: &str) -> bool {
        self.blocks(left, right) || self.blocks(right, left)
    }

    fn blocks(&self, owner: &str, other: &str) -> bool {
        if self.username == owner {
            self.blocked.contains(other)
        } else {
            self.account_profiles
                .get(owner)
                .is_some_and(|profile| profile.blocked.contains(other))
        }
    }

    pub(super) fn guest(&self) -> bool {
        self.managed_profiles && self.username.is_empty()
    }

    pub(super) fn send_direct_message(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let to = checked_handle(value(form, "recipient"))?;
        if to == self.username || self.either_blocked(&self.username, &to) {
            return Err("This account is unavailable for messages.".into());
        }
        if !self.has_chat_conversation(&to) {
            return Err("Send a chat request to start a conversation.".into());
        }
        let body = required(form, "body", "Message", 4_000)?;
        let id = next_id(&mut self.next_message)?;
        self.messages.push(DirectMessage {
            id,
            from: self.username.clone(),
            to: to.clone(),
            body: body.into(),
            at: unix_seconds(),
            read: false,
        });
        self.notify(
            &to,
            format!("u/{} sent you a message.", self.username),
            NotificationKind::Message,
        )?;
        Ok("Message sent.")
    }

    pub(super) fn read_conversation(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let peer = checked_handle(value(form, "peer"))?;
        for message in &mut self.messages {
            if message.from == peer && message.to == self.username {
                message.read = true;
            }
        }
        Ok("Conversation marked read.")
    }

    pub(super) fn read_notifications(&mut self) -> Result<&'static str, String> {
        for notification in self
            .notifications
            .iter_mut()
            .filter(|item| item.recipient == self.username)
        {
            notification.read = true;
        }
        Ok("Notifications marked read.")
    }

    pub(super) fn notify_comment(&mut self, post: u64, parent: Option<u64>) -> Result<(), String> {
        let (recipient, subject) = match parent {
            Some(id) => (
                self.comments
                    .iter()
                    .find(|comment| comment.id == id)
                    .map(|item| item.author.clone()),
                "replied to your comment",
            ),
            None => (
                self.posts
                    .iter()
                    .find(|item| item.id == post)
                    .map(|item| item.author.clone()),
                "commented on your post",
            ),
        };
        if let Some(recipient) = recipient {
            self.notify(
                &recipient,
                format!("u/{} {subject}.", self.username),
                NotificationKind::Reply(post),
            )?;
        }
        Ok(())
    }

    pub(super) fn push_appeal_notification(
        &mut self,
        recipient: &str,
        body: String,
    ) -> Result<(), String> {
        self.notify(recipient, body, NotificationKind::Appeal)
    }

    pub(super) fn push_moderation_notification(
        &mut self,
        recipient: &str,
        body: String,
    ) -> Result<(), String> {
        self.notify(recipient, body, NotificationKind::Moderation)
    }

    fn notify(
        &mut self,
        recipient: &str,
        body: String,
        kind: NotificationKind,
    ) -> Result<(), String> {
        if recipient.is_empty() || recipient == DELETED_ACCOUNT {
            return Ok(());
        }
        let prefs = self.notification_preferences_for(recipient);
        let direct = matches!(kind, NotificationKind::Reply(_) | NotificationKind::Message);
        if direct && (recipient == self.username || self.either_blocked(&self.username, recipient))
        {
            return Ok(());
        }
        let (post, appeal, moderation, silent) = match kind {
            NotificationKind::Reply(post) if prefs.replies => (Some(post), false, false, false),
            NotificationKind::Message if prefs.messages => (None, false, false, false),
            NotificationKind::Reply(_) | NotificationKind::Message => return Ok(()),
            NotificationKind::Appeal => (None, true, false, !prefs.moderation),
            NotificationKind::Moderation => (None, false, true, !prefs.moderation),
        };
        let id = next_id(&mut self.next_notification)?;
        self.notifications.push(Notification {
            id,
            recipient: recipient.into(),
            body,
            post,
            appeal,
            moderation,
            silent,
            at: unix_seconds(),
            read: silent,
        });
        Ok(())
    }
}

enum NotificationKind {
    Reply(u64),
    Message,
    Appeal,
    Moderation,
}

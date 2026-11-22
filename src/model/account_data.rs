use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use super::{
    checked_handle, encode, encode_image, next_id, required, unix_seconds, value, Appearance,
    ChatRequestPermission, Comment, Community, CustomFeed, DirectMessage, FeedLayout, Notification,
    NotificationPreferences, Post, PostDraft, ProfileAvatar, SocialLink, State, DELETED_ACCOUNT,
};

#[derive(Clone, Default)]
pub(super) struct ProfileData {
    pub(super) username: String,
    pub(super) votes: BTreeMap<u64, i8>,
    pub(super) comment_votes: BTreeMap<u64, i8>,
    pub(super) saved: BTreeSet<u64>,
    pub(super) saved_comments: BTreeSet<u64>,
    pub(super) hidden: BTreeSet<u64>,
    pub(super) joined: BTreeSet<String>,
    pub(super) followed_profiles: BTreeSet<String>,
    muted: BTreeSet<String>,
    blocked: BTreeSet<String>,
    keywords: Vec<String>,
    pub(super) sensitive: String,
    feed_layout: FeedLayout,
    appearance: Appearance,
    home_joined_only: bool,
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
    drafts: Vec<PostDraft>,
    next_draft: u64,
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
            next_draft: 1,
            ..Self::default()
        }
    }

    pub(super) fn capture(state: &State) -> Self {
        Self {
            username: state.username.clone(),
            votes: state.votes.clone(),
            comment_votes: state.comment_votes.clone(),
            saved: state.saved.clone(),
            saved_comments: state.saved_comments.clone(),
            hidden: state.hidden.clone(),
            joined: state.joined.clone(),
            followed_profiles: state.followed_profiles.clone(),
            muted: state.muted.clone(),
            blocked: state.blocked.clone(),
            keywords: state.keywords.clone(),
            sensitive: state.sensitive.clone(),
            feed_layout: state.feed_layout,
            appearance: state.appearance,
            home_joined_only: state.home_joined_only,
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
            drafts: state.drafts.clone(),
            next_draft: state.next_draft,
            notification_preferences: state.notification_preferences,
            chat_request_permission: state.chat_request_permission,
        }
    }

    fn restore(self, state: &mut State) {
        state.username = self.username;
        state.votes = self.votes;
        state.comment_votes = self.comment_votes;
        state.saved = self.saved;
        state.saved_comments = self.saved_comments;
        state.hidden = self.hidden;
        state.joined = self.joined;
        state.followed_profiles = self.followed_profiles;
        state.muted = self.muted;
        state.blocked = self.blocked;
        state.keywords = self.keywords;
        state.sensitive = self.sensitive;
        state.feed_layout = self.feed_layout;
        state.appearance = self.appearance;
        state.home_joined_only = self.home_joined_only;
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
        state.drafts = self.drafts;
        state.next_draft = self.next_draft;
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
                | "draft-sequence"
                | "notification-preferences"
                | "chat-request-permission"
                | "feed-layout"
                | "appearance"
                | "home-joined-only"
                | "F"
                | "V"
                | "H"
                | "vote"
                | "comment-vote"
                | "saved"
                | "saved-comment"
                | "hidden"
                | "profile-hidden-post"
                | "profile-hidden-comment"
                | "joined"
                | "followed-profile"
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
        if self.feed_layout != FeedLayout::default() {
            writeln!(file, "feed-layout\t{}", self.feed_layout.code())?;
        }
        if self.appearance != Appearance::default() {
            writeln!(file, "appearance\t{}", self.appearance.code())?;
        }
        if self.home_joined_only {
            writeln!(file, "home-joined-only\ttrue")?;
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
        if self.next_draft > 1 {
            writeln!(file, "draft-sequence\t{}", self.next_draft)?;
        }
        for draft in &self.drafts {
            write!(
                file,
                "draft\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                draft.id,
                encode(&draft.community),
                encode(&draft.title),
                encode(&draft.body),
                encode(&draft.url),
                encode(&draft.flair),
                draft.sensitive,
                draft.spoiler,
                draft.brand_affiliate
            )?;
            if let Some(flair_id) = draft.flair_id {
                write!(file, "\tflair\t{flair_id}")?;
            }
            writeln!(file)?;
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
            ("saved-comment", &self.saved_comments),
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
            ("followed-profile", &self.followed_profiles),
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

    pub(super) fn profile_creation_time(&self, handle: &str) -> Option<u64> {
        if handle.is_empty() || handle == DELETED_ACCOUNT {
            return None;
        }
        let created_at = if handle == self.username {
            self.profile_created
        } else {
            self.account_profiles.get(handle)?.created
        };
        (created_at > 0).then_some(created_at)
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

    pub fn can_follow_profile(&self, handle: &str) -> bool {
        !self.username.is_empty()
            && handle != self.username
            && !self.either_blocked(&self.username, handle)
    }

    pub fn following_includes_post(&self, post: &Post, group: &Community) -> bool {
        self.joined.contains(&group.slug)
            || (group.visibility != "private"
                && self.followed_profiles.contains(&post.author)
                && self.can_follow_profile(&post.author)
                && self
                    .account_profiles
                    .get(&post.author)
                    .is_some_and(|profile| {
                        profile.public_posts && !profile.hidden_posts.contains(&post.id)
                    }))
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

    pub(super) fn push_removal_notification(
        &mut self,
        recipient: &str,
        body: String,
        episode: u64,
    ) -> Result<(), String> {
        self.notify(recipient, body, NotificationKind::Removal(episode))
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
        let (post, appeal, moderation, removal, silent) = match kind {
            NotificationKind::Reply(post) if prefs.replies => {
                (Some(post), false, false, None, false)
            }
            NotificationKind::Message if prefs.messages => (None, false, false, None, false),
            NotificationKind::Reply(_) | NotificationKind::Message => return Ok(()),
            NotificationKind::Appeal => (None, true, false, None, !prefs.moderation),
            NotificationKind::Moderation => (None, false, true, None, !prefs.moderation),
            NotificationKind::Removal(episode) => {
                (None, false, true, Some(episode), !prefs.moderation)
            }
        };
        let id = next_id(&mut self.next_notification)?;
        self.notifications.push(Notification {
            id,
            recipient: recipient.into(),
            body,
            post,
            appeal,
            moderation,
            removal,
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
    Removal(u64),
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use super::*;

    struct Fixture(PathBuf);

    impl Drop for Fixture {
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
    fn profile_follows_preserve_privacy_and_end_with_a_block_or_deletion() {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "commonline-profile-follows-{}-{nonce}.state",
            std::process::id()
        )));
        let path = &fixture.0;
        let reader = "follow_reader";
        let author = "follow_author";
        let mut state = State::seeded();
        state.activate_profile(Some(author));
        let mut group = state.communities[0].clone();
        let mut post = state.posts[0].clone();
        post.author = author.into();
        post.community.clone_from(&group.slug);
        state.save(path).unwrap();

        let mut state = State::load(path).unwrap();
        state.activate_profile(Some(reader));
        assert!(state.followed_profiles.is_empty());
        assert!(!state.can_follow_profile(reader));
        assert!(!state.following_includes_post(&post, &group));
        let follow = |state: &mut State, handle| {
            state.apply("follow-profile", &form(&[("handle", handle)]), path)
        };
        follow(&mut state, author).unwrap();
        follow(&mut state, author).unwrap();
        assert_eq!(state.followed_profiles.len(), 1);

        let mut state = State::load(path).unwrap();
        state.activate_profile(Some("other_reader"));
        assert!(state.followed_profiles.is_empty());
        assert!(!state.following_includes_post(&post, &group));
        state.activate_profile(Some(reader));
        assert!(state.following_includes_post(&post, &group));

        let old_visibility = std::mem::replace(&mut group.visibility, "private".into());
        assert!(!state.following_includes_post(&post, &group));
        state.joined.insert(group.slug.clone());
        assert!(state.following_includes_post(&post, &group));
        state.joined.remove(&group.slug);
        group.visibility = old_visibility;

        state.activate_profile(Some(author));
        state.public_posts = false;
        state.activate_profile(Some(reader));
        assert!(!state.following_includes_post(&post, &group));
        state.activate_profile(Some(author));
        state.public_posts = true;
        state.profile_hidden_posts.insert(post.id);
        state.activate_profile(Some(reader));
        assert!(!state.following_includes_post(&post, &group));

        state.activate_profile(Some(author));
        follow(&mut state, reader).unwrap();
        let block = form(&[("author", reader)]);
        state.apply("block", &block, path).unwrap();
        assert!(state.followed_profiles.is_empty());
        state.activate_profile(Some(reader));
        assert!(state.followed_profiles.is_empty());
        assert!(follow(&mut state, author).is_err());

        state.activate_profile(Some(author));
        state.apply("block", &block, path).unwrap();
        state.activate_profile(Some(reader));
        follow(&mut state, author).unwrap();
        state
            .delete_account_profiles(&[author.into()], path)
            .unwrap();
        state = State::load(path).unwrap();
        state.activate_profile(Some(reader));
        assert!(state.followed_profiles.is_empty());
        let unfollow = form(&[("handle", author)]);
        state.apply("unfollow-profile", &unfollow, path).unwrap();
        assert!(state.followed_profiles.is_empty());
    }

    #[test]
    fn saved_comments_are_private_idempotent_and_persist_across_access_changes() {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "commonline-saved-comments-{}-{nonce}.state",
            std::process::id()
        )));
        let path = &fixture.0;
        let mut state = State::seeded();
        let apply = |state: &mut State, action: &str, fields: &[(&str, &str)]| {
            state.apply(action, &form(fields), path).unwrap();
        };
        let community = "private-bookmarks";
        let owner = "bookmark_owner";
        let reader = "bookmark_reader";
        state.activate_profile(Some(owner));
        apply(
            &mut state,
            "create-community",
            &[
                ("name", "Private bookmarks"),
                ("slug", community),
                ("visibility", "private"),
            ],
        );
        apply(
            &mut state,
            "create",
            &[("community", community), ("title", "Bookmark discussion")],
        );
        let post_id = state.posts.last().unwrap().id.to_string();
        apply(
            &mut state,
            "comment",
            &[("post", &post_id), ("body", "Bookmark this comment.")],
        );
        let comment_id = state.comments.last().unwrap().id;
        let comment_key = comment_id.to_string();
        let save = [("id", comment_key.as_str())];
        let member = [("community", community), ("handle", reader)];
        let removal = [
            ("community", community),
            ("kind", "comment"),
            ("id", comment_key.as_str()),
            ("reason", "Temporary removal"),
        ];
        state.activate_profile(Some(reader));
        assert!(state.apply("save-comment", &form(&save), path).is_err());
        state.activate_profile(Some(owner));
        apply(&mut state, "approve-member", &member);
        state.activate_profile(Some(reader));
        apply(&mut state, "block", &[("author", owner)]);
        assert!(state.apply("save-comment", &form(&save), path).is_err());
        apply(&mut state, "block", &[("author", owner)]);
        apply(&mut state, "save-comment", &save);
        apply(&mut state, "save-comment", &save);
        apply(&mut state, "save", &[("id", &post_id)]);
        assert_eq!(state.saved_comments, BTreeSet::from([comment_id]));

        state.activate_profile(Some(owner));
        assert!(state.saved_comments.is_empty());
        apply(&mut state, "remove-content", &removal);
        let mut state = State::load(path).unwrap();
        state.activate_profile(Some(reader));
        assert!(state.saved_comments.contains(&comment_id));
        assert!(!state.can_view_comment(state.comments.last().unwrap()));
        state.activate_profile(Some(owner));
        apply(&mut state, "restore-content", &removal);
        state.activate_profile(Some(reader));
        assert!(state.can_view_comment(state.comments.last().unwrap()));

        state.activate_profile(Some(owner));
        apply(&mut state, "revoke-approved-member", &member);
        state.activate_profile(Some(reader));
        assert!(!state.can_view_comment(state.comments.last().unwrap()));
        apply(&mut state, "unsave-comment", &save);
        apply(&mut state, "unsave-comment", &save);
        let state = State::load(path).unwrap();
        assert!(state.saved_comments.is_empty());
        assert_eq!(state.saved.len(), 1);
    }

    #[test]
    fn deleting_comments_or_posts_clears_bookmarks_for_all_profiles() {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "commonline-deleted-bookmarks-{}-{nonce}.state",
            std::process::id()
        )));
        let path = &fixture.0;
        let mut state = State::seeded();
        let apply = |state: &mut State, action: &str, fields: &[(&str, &str)]| {
            state.apply(action, &form(fields), path).unwrap();
        };
        state.activate_profile(Some("bookmark_author"));
        apply(
            &mut state,
            "create",
            &[("community", "open-debate"), ("title", "Bookmark cleanup")],
        );
        let post_id = state.posts.last().unwrap().id.to_string();
        let mut comment_keys = Vec::new();
        for body in ["First bookmarked comment", "Second bookmarked comment"] {
            apply(&mut state, "comment", &[("post", &post_id), ("body", body)]);
            comment_keys.push(state.comments.last().unwrap().id.to_string());
        }
        for handle in ["bookmark_author", "bookmark_other"] {
            state.activate_profile(Some(handle));
            for key in &comment_keys {
                apply(&mut state, "save-comment", &[("id", key)]);
            }
        }
        state.activate_profile(Some("bookmark_author"));
        apply(&mut state, "delete-comment", &[("id", &comment_keys[0])]);
        assert_eq!(state.saved_comments.len(), 1);
        assert_eq!(
            state.account_profiles["bookmark_other"]
                .saved_comments
                .len(),
            1
        );
        apply(&mut state, "delete", &[("id", &post_id)]);
        let mut state = State::load(path).unwrap();
        assert!(state.saved_comments.is_empty());
        state.activate_profile(Some("bookmark_other"));
        assert!(state.saved_comments.is_empty());
    }

    #[test]
    fn home_recommendations_default_on_and_persist_per_account() {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "commonline-home-recommendations-{}-{nonce}.state",
            std::process::id()
        )));
        let path = &fixture.0;
        let mut state = State::seeded();
        state.activate_profile(Some("recommendation_owner"));
        state.save(path).unwrap();

        let mut state = State::load(path).unwrap();
        assert!(!state.home_joined_only);
        state
            .apply("set-feed-layout", &form(&[("layout", "compact")]), path)
            .unwrap();
        state
            .apply(
                "preferences",
                &form(&[("sensitive", "hide"), ("home_recommendations_present", "1")]),
                path,
            )
            .unwrap();
        assert!(state.home_joined_only);
        assert_eq!(state.feed_layout, FeedLayout::Compact);
        for invalid in [
            form(&[("sensitive", "show"), ("home_recommendations_present", "")]),
            form(&[
                ("sensitive", "show"),
                ("home_recommendations_present", "1"),
                ("home_recommendations", "off"),
            ]),
        ] {
            assert!(state.apply("preferences", &invalid, path).is_err());
            assert!(state.home_joined_only);
            assert_eq!(state.sensitive, "hide");
        }
        state
            .apply(
                "preferences",
                &form(&[("sensitive", "blur"), ("keywords", "quiet")]),
                path,
            )
            .unwrap();
        assert!(state.home_joined_only);

        state.activate_profile(Some("recommendation_other"));
        assert!(!state.home_joined_only);
        state.save(path).unwrap();
        let mut state = State::load(path).unwrap();
        assert!(!state.home_joined_only);
        state.activate_profile(Some("recommendation_owner"));
        assert!(state.home_joined_only);
        assert_eq!(state.keywords, ["quiet"]);
        state
            .apply(
                "preferences",
                &form(&[
                    ("sensitive", "blur"),
                    ("home_recommendations_present", "1"),
                    ("home_recommendations", "on"),
                ]),
                path,
            )
            .unwrap();
        let state = State::load(path).unwrap();
        assert!(!state.home_joined_only);
        assert_eq!(state.feed_layout, FeedLayout::Compact);
    }

    #[test]
    fn drafts_keep_identity_scope_and_content_across_publishing_and_restarts() {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "commonline-drafts-{}-{nonce}.state",
            std::process::id()
        )));
        let path = &fixture.0;
        let mut initial = State::seeded();
        initial.activate_profile(Some("draft_owner"));
        initial.save(path).unwrap();
        let active_draft = "draft\tmedia-lab\tOld title\tOld body\thttps://example.org/old\tOld flair\ttrue\tfalse\ttrue";
        let other_draft = "draft\t\tOther account draft\tOther body\t\t\tfalse\ttrue\tfalse";
        let snapshot = fs::read_to_string(path).unwrap();
        fs::write(
            path,
            format!(
                "{snapshot}{active_draft}\naccount-data\tdraft_other\tblur\t{}\n",
                encode(other_draft)
            ),
        )
        .unwrap();

        let mut state = State::load(path).unwrap();
        let legacy = state.draft(1).unwrap();
        assert_eq!(legacy.title, "Old title");
        assert_eq!(legacy.body, "Old body");
        assert_eq!(legacy.url, "https://example.org/old");
        assert_eq!(legacy.flair, "Old flair");
        assert!(legacy.sensitive && !legacy.spoiler && legacy.brand_affiliate);

        state
            .apply("save-draft", &form(&[("body", "Untitled draft")]), path)
            .unwrap();
        state
            .apply("save-draft", &form(&[("title", "Discard this one")]), path)
            .unwrap();
        let publish_id = state.drafts[1].id;
        let discard_id = state.drafts[2].id;
        assert!(1 < publish_id && publish_id < discard_id);
        let edit = form(&[
            ("draft_id", &publish_id.to_string()),
            ("community", "media-lab"),
            ("title", "Ready to publish"),
            ("body", "Updated body"),
        ]);
        state.apply("save-draft", &edit, path).unwrap();
        assert_eq!(state.drafts.len(), 3);
        assert_eq!(state.drafts[1].id, publish_id);
        assert_eq!(state.drafts[2].id, discard_id);

        state.activate_profile(Some("draft_other"));
        assert_eq!(state.draft(1).unwrap().title, "Other account draft");
        assert!(state.draft(publish_id).is_none());
        assert!(state.apply("save-draft", &edit, path).is_err());
        assert!(state.apply("create", &edit, path).is_err());
        let discard = form(&[("draft_id", &discard_id.to_string())]);
        assert!(state.apply("delete-draft", &discard, path).is_err());
        state.activate_profile(Some("draft_owner"));
        assert_eq!(state.draft(publish_id).unwrap().body, "Updated body");
        state.apply("delete-draft", &discard, path).unwrap();

        let mut state = State::load(path).unwrap();
        assert!(state.draft(discard_id).is_none());
        state
            .apply("save-draft", &form(&[("title", "Keep this draft")]), path)
            .unwrap();
        let next = state.drafts.last().unwrap().id;
        assert!(next > discard_id);
        let invalid = form(&[("draft_id", &publish_id.to_string())]);
        let mut poll = edit.clone();
        poll.extend(form(&[
            ("option_1", "Yes"),
            ("option_2", "No"),
            ("duration", "1"),
        ]));
        state.apply("create-poll", &poll, path).unwrap();
        assert!(state.draft(publish_id).is_some());
        let posts = state.posts.len();
        assert!(state.apply("create", &invalid, path).is_err());
        assert!(state
            .apply("create", &edit, &path.join("invalid.state"))
            .is_err());
        assert!(state.draft(publish_id).is_some());
        assert_eq!(state.posts.len(), posts);

        state.apply("create", &edit, path).unwrap();
        assert!(state.apply("create", &edit, path).is_err());
        assert_eq!(state.posts.len(), posts + 1);
        assert_eq!(state.posts.last().unwrap().title, "Ready to publish");
        assert!(state.draft(publish_id).is_none());
        assert_eq!(state.draft(1).unwrap().title, "Old title");
        assert_eq!(state.draft(next).unwrap().title, "Keep this draft");
        let mut state = State::load(path).unwrap();
        assert!(state.draft(publish_id).is_none());
        for id in [1, next] {
            state
                .apply(
                    "delete-draft",
                    &form(&[("draft_id", &id.to_string())]),
                    path,
                )
                .unwrap();
        }

        let mut state = State::load(path).unwrap();
        assert!(state.drafts.is_empty());
        state
            .apply(
                "save-draft",
                &form(&[("title", "Saved after clearing drafts")]),
                path,
            )
            .unwrap();
        assert!(state.drafts[0].id > next);
        state.activate_profile(Some("draft_other"));
        assert_eq!(state.drafts.len(), 1);
        assert_eq!(state.draft(1).unwrap().title, "Other account draft");
    }
}

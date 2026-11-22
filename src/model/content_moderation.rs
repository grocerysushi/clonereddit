use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    bounded, canonical_u64, checked_handle, decode, encode, next_id, number, required,
    unix_seconds, value, CommunityRule, ModPermission, Post, State, COMMUNITY_RULE_DESCRIPTION_MAX,
    COMMUNITY_RULE_TITLE_MAX,
};

const REASON_LIMIT: usize = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentKind {
    Post,
    Comment,
}

impl ContentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Post => "post",
            Self::Comment => "comment",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Post => "Post",
            Self::Comment => "Comment",
        }
    }

    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "post" => Some(Self::Post),
            "comment" => Some(Self::Comment),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct ContentRemoval {
    pub episode: u64,
    pub kind: ContentKind,
    pub id: u64,
    pub post_id: u64,
    pub community: String,
    pub reason: String,
    pub rule: Option<CommunityRule>,
    pub note: String,
    pub moderator: String,
    pub removed_at: u64,
    pub automated: bool,
}

impl ContentRemoval {
    pub fn reason_summary(&self) -> String {
        let Some(rule) = &self.rule else {
            return self.reason.clone();
        };
        let mut reason = format!("Community rule: {}", rule.title);
        if !rule.description.is_empty() {
            reason.push('\n');
            reason.push_str(&rule.description);
        }
        if !self.note.is_empty() {
            reason.push_str("\n\nNote from the moderators: ");
            reason.push_str(&self.note);
        }
        reason
    }
}

impl State {
    pub fn content_removal(&self, kind: ContentKind, id: u64) -> Option<&ContentRemoval> {
        self.content_removals
            .iter()
            .find(|item| item.kind == kind && item.id == id)
    }

    pub fn is_post_removed(&self, id: u64) -> bool {
        self.content_removal(ContentKind::Post, id).is_some()
    }

    pub fn is_comment_removed(&self, id: u64) -> bool {
        self.content_removal(ContentKind::Comment, id).is_some()
    }

    pub fn visible_reply_count(&self, post: &Post) -> u32 {
        let removed = self
            .content_removals
            .iter()
            .filter(|item| item.kind == ContentKind::Comment && item.post_id == post.id)
            .count();
        post.reply_count
            .saturating_sub(u32::try_from(removed).unwrap_or(u32::MAX))
    }

    pub(super) fn content_moderation_target(
        &self,
        kind: ContentKind,
        id: u64,
        community: &str,
    ) -> Option<(u64, &str)> {
        match kind {
            ContentKind::Post => {
                let post = self
                    .posts
                    .iter()
                    .find(|post| post.id == id && post.community == community)?;
                Some((post.id, &post.author))
            }
            ContentKind::Comment => {
                let comment = self
                    .comments
                    .iter()
                    .find(|comment| comment.id == id && !comment.deleted)?;
                let post = self
                    .posts
                    .iter()
                    .find(|post| post.id == comment.post && post.community == community)?;
                Some((post.id, &comment.author))
            }
        }
    }

    pub(super) fn moderate_content(
        &mut self,
        form: &BTreeMap<String, String>,
        restore: bool,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let community = self.communities[index].slug.clone();
        let kind = ContentKind::parse(value(form, "kind")).ok_or("Choose a post or comment.")?;
        let id = canonical_u64(value(form, "id")).ok_or("Choose a valid post or comment.")?;
        let (post_id, author) = self
            .content_moderation_target(kind, id, &community)
            .ok_or("This post or comment is unavailable in this community.")?;
        let author = author.to_owned();
        let existing = self
            .content_removals
            .iter()
            .position(|item| item.kind == kind && item.id == id && item.community == community);
        let label = kind.as_str();
        if restore {
            let index = existing.ok_or("This post or comment is already visible.")?;
            let removal = self.restore_content_removal(index)?;
            self.resolve_manually_restored_content_appeal(removal.episode);
            let context = self.restoration_context(&removal);
            let notice =
                format!("Moderators restored your {label} #{id} in c/{community}.{context}");
            self.push_moderation_notification(&author, notice)?;
            return Ok("Content restored.");
        }
        if existing.is_some() {
            return Err("This post or comment is already removed.".into());
        }
        let selection = value(form, "rule");
        let (reason, rule, note) = if selection.is_empty() {
            (
                required(form, "reason", "Removal reason", REASON_LIMIT)?.into(),
                None,
                String::new(),
            )
        } else {
            let selected = canonical_u64(selection)
                .and_then(|id| {
                    self.communities[index]
                        .rules()
                        .iter()
                        .find(|rule| rule.id == id)
                })
                .ok_or("Choose an available rule from this community.")?;
            let note = bounded(form, "reason", "Note to the author", REASON_LIMIT)?;
            (selected.title.clone(), Some(selected.clone()), note.into())
        };
        let episode = next_id(&mut self.next_removal_episode)?;
        self.record_content_removal(
            ContentRemoval {
                episode,
                kind,
                id,
                post_id,
                community,
                reason,
                rule,
                note,
                moderator: self.username.clone(),
                removed_at: unix_seconds(),
                automated: false,
            },
            &author,
            None,
        )?;
        Ok("Content removed.")
    }

    pub(super) fn remove_content_automatically(
        &mut self,
        kind: ContentKind,
        id: u64,
        community: &str,
        rule_name: &str,
        reason: &str,
    ) -> Result<(), String> {
        if self.content_removal(kind, id).is_some() {
            return Err("This post or comment is already removed.".into());
        }
        let (post_id, author) = self
            .content_moderation_target(kind, id, community)
            .ok_or("This post or comment is unavailable in this community.")?;
        let author = author.to_owned();
        let episode = next_id(&mut self.next_removal_episode)?;
        self.record_content_removal(
            ContentRemoval {
                episode,
                kind,
                id,
                post_id,
                community: community.into(),
                reason: reason.into(),
                rule: None,
                note: String::new(),
                moderator: String::new(),
                removed_at: unix_seconds(),
                automated: true,
            },
            &author,
            Some(rule_name),
        )
    }

    fn record_content_removal(
        &mut self,
        removal: ContentRemoval,
        author: &str,
        automod_rule_name: Option<&str>,
    ) -> Result<(), String> {
        let source = if removal.automated {
            "AutoMod"
        } else {
            "Moderators"
        };
        let notice = format!(
            "{source} removed your {} #{} in c/{}.\n\n{}",
            removal.kind.as_str(),
            removal.id,
            removal.community,
            removal.reason_summary(),
        );
        let actor = match automod_rule_name {
            Some(rule_name) => format!("AutoMod ({rule_name})"),
            None => format!("u/{}", removal.moderator),
        };
        let log = format!(
            "{} #{} removed by {actor}",
            removal.kind.label(),
            removal.id
        );
        self.clear_automod_flag(removal.kind, removal.id);
        self.push_removal_notification(author, notice, removal.episode)?;
        self.record_moderation_action(&removal.community, &log)?;
        self.content_removals.push(removal);
        Ok(())
    }

    pub(super) fn restore_content_removal(
        &mut self,
        index: usize,
    ) -> Result<ContentRemoval, String> {
        let removal = self.content_removals.remove(index);
        let log = format!(
            "{} #{} restored by u/{}",
            removal.kind.label(),
            removal.id,
            self.username
        );
        self.record_moderation_action(&removal.community, &log)?;
        Ok(removal)
    }

    pub(super) fn restoration_context(&self, removal: &ContentRemoval) -> &'static str {
        if removal.kind == ContentKind::Comment && self.is_post_removed(removal.post_id) {
            " The discussion is still removed."
        } else {
            ""
        }
    }

    pub(super) fn read_content_removal(&mut self, fields: &[&str]) -> Option<()> {
        let ["content-removal", kind, id, community, reason, moderator, removed_at, source @ ..] =
            fields
        else {
            return None;
        };
        let kind = ContentKind::parse(kind)?;
        let id = number(id)?;
        let community = decode(community)?;
        let (post_id, _) = self.content_moderation_target(kind, id, &community)?;
        let reason = decode(reason)?;
        let moderator = decode(moderator)?;
        let (episode, source) = match source {
            ["episode", id, source @ ..] => {
                let episode = canonical_u64(id).filter(|&id| id != 0)?;
                if self
                    .content_removals
                    .iter()
                    .any(|entry| entry.episode == episode)
                {
                    return None;
                }
                self.next_removal_episode = self.next_removal_episode.max(episode.checked_add(1)?);
                (episode, source)
            }
            _ => (next_id(&mut self.next_removal_episode).ok()?, source),
        };
        let (automated, rule, note) = match source {
            [] if checked_handle(&moderator).ok()? == moderator => (false, None, String::new()),
            ["automod"] if moderator.is_empty() => (true, None, String::new()),
            ["rule", id, description, note] if checked_handle(&moderator).ok()? == moderator => {
                let id = canonical_u64(id).filter(|&id| id != 0)?;
                let description = decode(description)?;
                let note = decode(note)?;
                if reason.chars().count() > COMMUNITY_RULE_TITLE_MAX
                    || description.chars().count() > COMMUNITY_RULE_DESCRIPTION_MAX
                    || note.chars().count() > REASON_LIMIT
                {
                    return None;
                }
                let rule = CommunityRule {
                    id,
                    title: reason.clone(),
                    description,
                };
                (false, Some(rule), note)
            }
            _ => return None,
        };
        if self.content_removal(kind, id).is_some()
            || reason.trim().is_empty()
            || reason.chars().count() > REASON_LIMIT
        {
            return None;
        }
        self.content_removals.push(ContentRemoval {
            episode,
            kind,
            id,
            post_id,
            community,
            reason,
            rule,
            note,
            moderator,
            removed_at: number(removed_at)?,
            automated,
        });
        Some(())
    }

    pub(super) fn write_content_removals(&self, file: &mut impl Write) -> io::Result<()> {
        for item in &self.content_removals {
            write!(
                file,
                "content-removal\t{}\t{}\t{}\t{}\t{}\t{}\tepisode\t{}",
                item.kind.as_str(),
                item.id,
                encode(&item.community),
                encode(&item.reason),
                encode(&item.moderator),
                item.removed_at,
                item.episode,
            )?;
            if item.automated {
                write!(file, "\tautomod")?;
            } else if let Some(rule) = &item.rule {
                write!(
                    file,
                    "\trule\t{}\t{}\t{}",
                    rule.id,
                    encode(&rule.description),
                    encode(&item.note),
                )?;
            }
            writeln!(file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PostMedia;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|&(key, val)| (key.into(), val.into()))
            .collect()
    }

    fn target(community: &str, kind: &str, id: u64, reason: &str) -> BTreeMap<String, String> {
        form(&[
            ("community", community),
            ("kind", kind),
            ("id", &id.to_string()),
            ("reason", reason),
        ])
    }

    fn community_fixture() -> State {
        let mut state = State::seeded();
        state.activate_profile(Some("board_owner"));
        for slug in ["test-board", "other-board"] {
            state
                .create_community(&form(&[("slug", slug), ("name", slug)]))
                .unwrap();
        }
        for (handle, role) in [
            ("editor_user", "moderator"),
            ("appeal_user", "appeals-reviewer"),
        ] {
            state
                .set_moderator_role(&form(&[
                    ("community", "test-board"),
                    ("handle", handle),
                    ("role", role),
                ]))
                .unwrap();
        }
        state
    }

    struct TempState(PathBuf);

    impl TempState {
        fn new() -> Self {
            let filename = format!(
                "commonline-content-{}-{}.state",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            );
            Self(std::env::temp_dir().join(filename))
        }
    }

    impl Drop for TempState {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn post_removal_is_scoped_reversible_and_persists_with_its_private_notice() {
        let fixture = TempState::new();
        let mut state = community_fixture();
        state.activate_profile(Some("post_author"));
        state
            .create_post(&form(&[
                ("community", "test-board"),
                ("title", "Original title"),
            ]))
            .unwrap();
        let post = state.posts.last_mut().unwrap();
        let id = post.id;
        post.media.push(PostMedia {
            key: "a".repeat(48),
            content_type: "image/png".into(),
        });
        let post_id = id.to_string();
        state
            .change("vote", &form(&[("id", &post_id), ("direction", "1")]))
            .unwrap();
        state
            .create_report(&form(&[
                ("post", &post_id),
                ("region", "Test region"),
                ("details", "Existing report"),
            ]))
            .unwrap();
        let report = state.reports.last().unwrap().id.to_string();
        let reason = "Rule example.\nSecond line & literal <markup>.";
        let removal = target("test-board", "post", id, reason);

        for handle in ["post_author", "appeal_user", "unrelated_user"] {
            state.activate_profile(Some(handle));
            assert!(state.apply("remove-content", &removal, &fixture.0).is_err());
        }
        state.activate_profile(Some("board_owner"));
        assert!(state
            .apply(
                "remove-content",
                &target("other-board", "post", id, reason),
                &fixture.0
            )
            .is_err());
        assert!(state
            .apply(
                "remove-content",
                &target("test-board", "comment", id, reason),
                &fixture.0
            )
            .is_err());
        state
            .pin_community(&form(&[("community", "test-board"), ("post", &post_id)]))
            .unwrap();
        state.activate_profile(Some("editor_user"));
        for invalid in [String::new(), "é".repeat(REASON_LIMIT + 1)] {
            assert!(state
                .apply(
                    "remove-content",
                    &target("test-board", "post", id, &invalid),
                    &fixture.0
                )
                .is_err());
        }
        state.apply("remove-content", &removal, &fixture.0).unwrap();
        assert!(state.apply("remove-content", &removal, &fixture.0).is_err());
        state
            .apply(
                "review-report",
                &form(&[("id", &report), ("reviewed", "yes")]),
                &fixture.0,
            )
            .unwrap();

        let mut state = State::load(&fixture.0).unwrap();
        assert!(state.is_post_removed(id));
        assert_eq!(state.reports.last().unwrap().reviewed, true);
        let stored = state.content_removal(ContentKind::Post, id).unwrap();
        assert_eq!(stored.moderator, "editor_user");
        assert!(stored.rule.is_none());
        assert!(stored.note.is_empty());
        let notices: Vec<_> = state
            .notifications
            .iter()
            .filter(|item| item.moderation)
            .collect();
        let [notice] = notices.as_slice() else {
            panic!("expected one direct author notice")
        };
        assert_eq!(notice.recipient, "post_author");
        assert_eq!(notice.post, None);
        assert!(!notice.appeal);
        assert!(notice.body.ends_with(reason));
        assert!(state.repost_source(id).is_none());

        state.activate_profile(Some("post_author"));
        let post_action = form(&[
            ("id", &post_id),
            ("post", &post_id),
            ("direction", "1"),
            ("body", "Known identifier"),
            ("title", "Edited title"),
        ]);
        for action in [
            "vote",
            "save",
            "hide",
            "comment",
            "report",
            "vote-poll",
            "edit",
            "delete",
            "profile-hide-post",
        ] {
            assert!(
                state.apply(action, &post_action, &fixture.0).is_err(),
                "{action}"
            );
        }
        state.activate_profile(Some("board_owner"));
        assert!(state
            .pin_community(&form(&[("community", "test-board"), ("post", &post_id)]))
            .is_err());
        state
            .apply("restore-content", &removal, &fixture.0)
            .unwrap();
        assert!(state
            .apply("restore-content", &removal, &fixture.0)
            .is_err());
        let mut state = State::load(&fixture.0).unwrap();
        let post = state.repost_source(id).unwrap();
        assert_eq!(post.title, "Original title");
        assert_eq!(post.media.len(), 1);
        assert_eq!(state.post_score(post), 2);
        assert_eq!(
            state.community_for_post(id).unwrap().highlighted_posts(),
            &[id]
        );
        assert!(state.notifications.iter().any(|item| item.moderation
            && item.recipient == "post_author"
            && item.body.starts_with("Moderators restored your post")));
        state.activate_profile(Some("post_author"));
        state.delete_post(&post_action).unwrap();
        state.activate_profile(Some("board_owner"));
        assert!(state.moderate_content(&removal, true).is_err());
    }

    #[test]
    fn rule_removals_preserve_the_reason_and_private_author_note() {
        let fixture = TempState::new();
        let mut state = community_fixture();
        let title = "Cite your sources";
        let description = "Link to the source.\nExplain its relevance.";
        state
            .apply(
                "save-community-rule",
                &form(&[
                    ("community", "test-board"),
                    ("title", title),
                    ("description", description),
                ]),
                &fixture.0,
            )
            .unwrap();
        state
            .apply(
                "save-community-rule",
                &form(&[
                    ("community", "other-board"),
                    ("title", "Different board rule"),
                ]),
                &fixture.0,
            )
            .unwrap();
        let rule_id = state
            .communities
            .iter()
            .find(|board| board.slug == "test-board")
            .unwrap()
            .rules()[0]
            .id;
        let foreign_id = state
            .communities
            .iter()
            .find(|board| board.slug == "other-board")
            .unwrap()
            .rules()[0]
            .id;

        state.activate_profile(Some("post_author"));
        for title in ["Original post", "Another post"] {
            state
                .create_post(&form(&[("community", "test-board"), ("title", title)]))
                .unwrap();
        }
        let post_id = state.posts[state.posts.len() - 2].id;
        let available_id = state.posts.last().unwrap().id;
        state.activate_profile(Some("reply_author"));
        state
            .create_comment(&form(&[
                ("post", &post_id.to_string()),
                ("body", "Original comment"),
            ]))
            .unwrap();
        let comment_id = state.comments.last().unwrap().id;
        let note = "Please replace <links>.\nInclude both sources.";
        let rule_target = |kind, id, note: &str, rule: u64| {
            let mut removal = target("test-board", kind, id, note);
            removal.insert("rule".into(), rule.to_string());
            removal
        };
        let post = rule_target("post", post_id, note, rule_id);
        let comment = rule_target("comment", comment_id, "", rule_id);
        for handle in ["post_author", "appeal_user"] {
            state.activate_profile(Some(handle));
            assert!(state.apply("remove-content", &post, &fixture.0).is_err());
        }
        state.activate_profile(Some("editor_user"));
        assert!(state
            .apply(
                "remove-content",
                &rule_target("comment", comment_id, note, foreign_id),
                &fixture.0
            )
            .is_err());
        let mut malformed = post.clone();
        malformed.insert("rule".into(), format!("0{rule_id}"));
        assert!(state
            .apply("remove-content", &malformed, &fixture.0)
            .is_err());
        let too_long = "é".repeat(REASON_LIMIT + 1);
        assert!(state
            .apply(
                "remove-content",
                &rule_target("post", post_id, &too_long, rule_id),
                &fixture.0
            )
            .is_err());
        assert!(state.content_removals.is_empty());
        assert!(!state.notifications.iter().any(|notice| notice.moderation));

        state.apply("remove-content", &comment, &fixture.0).unwrap();
        state.apply("remove-content", &post, &fixture.0).unwrap();
        state
            .apply(
                "save-community-rule",
                &form(&[
                    ("community", "test-board"),
                    ("id", &rule_id.to_string()),
                    ("title", "Updated title"),
                ]),
                &fixture.0,
            )
            .unwrap();
        state
            .apply(
                "delete-community-rule",
                &form(&[("community", "test-board"), ("id", &rule_id.to_string())]),
                &fixture.0,
            )
            .unwrap();

        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some("editor_user"));
        assert!(state
            .apply(
                "remove-content",
                &rule_target("post", available_id, note, rule_id),
                &fixture.0
            )
            .is_err());
        assert!(!state.is_post_removed(available_id));
        for (kind, id, author, expected_note) in [
            (ContentKind::Post, post_id, "post_author", note),
            (ContentKind::Comment, comment_id, "reply_author", ""),
        ] {
            let removal = state.content_removal(kind, id).unwrap();
            let rule = removal.rule.as_ref().unwrap();
            assert_eq!(
                (rule.id, rule.title.as_str(), rule.description.as_str()),
                (rule_id, title, description)
            );
            assert_eq!(removal.reason, title);
            assert_eq!(removal.note, expected_note);
            let header = format!(
                "Moderators removed your {} #{id} in c/test-board.",
                kind.as_str()
            );
            let notice = state
                .notifications
                .iter()
                .find(|notice| notice.body.starts_with(&header))
                .unwrap();
            assert_eq!(notice.recipient, author);
            assert!(notice.moderation);
            let mut expected = format!("{header}\n\nCommunity rule: {title}\n{description}");
            if !expected_note.is_empty() {
                expected.push_str(&format!("\n\nNote from the moderators: {expected_note}"));
            }
            assert_eq!(notice.body, expected);
        }
        assert_eq!(
            state
                .notifications
                .iter()
                .filter(|notice| notice.moderation)
                .count(),
            2
        );
        assert!(state
            .mod_log
            .iter()
            .all(|event| !event.action.contains(note)));
        state.apply("restore-content", &post, &fixture.0).unwrap();
        state
            .apply("restore-content", &comment, &fixture.0)
            .unwrap();
        assert!(state.content_removals.is_empty());
    }

    #[test]
    fn comment_removal_keeps_replies_and_author_deletion_clears_child_removal_records() {
        let mut state = community_fixture();
        state.activate_profile(Some("post_author"));
        state
            .create_post(&form(&[
                ("community", "test-board"),
                ("title", "Parent thread"),
            ]))
            .unwrap();
        let post_id = state.posts.last().unwrap().id.to_string();
        let parent_form = form(&[("post", &post_id), ("body", "Original comment")]);
        state.create_comment(&parent_form).unwrap();
        let parent = state.comments.last().unwrap().id;
        let parent_id = parent.to_string();
        let child_form = form(&[
            ("post", &post_id),
            ("parent", &parent_id),
            ("body", "Surviving reply"),
        ]);
        state.activate_profile(Some("reply_author"));
        state.create_comment(&child_form).unwrap();
        let child_id = state.comments.last().unwrap().id;
        let parent_action = form(&[
            ("id", &parent_id),
            ("direction", "1"),
            ("body", "Changed comment"),
        ]);
        state.change("comment-vote", &parent_action).unwrap();
        state.posts.last_mut().unwrap().reply_count += 4;
        let parent_removal = target("test-board", "comment", parent, "Moderation reason");
        state.activate_profile(Some("editor_user"));
        state.moderate_content(&parent_removal, false).unwrap();
        assert!(state.is_comment_removed(parent));
        assert_eq!(state.visible_reply_count(state.posts.last().unwrap()), 5);
        state.activate_profile(Some("post_author"));
        assert!(state.create_comment(&child_form).is_err());
        assert!(state
            .create_comment(&form(&[
                ("post", &post_id),
                ("parent", &child_id.to_string()),
                ("body", "Reply to a visible comment")
            ]))
            .is_ok());
        for action in [
            "comment-vote",
            "edit-comment",
            "delete-comment",
            "profile-hide-comment",
        ] {
            assert!(state.change(action, &parent_action).is_err(), "{action}");
        }
        assert!(state
            .comments
            .iter()
            .any(|item| item.id == child_id && item.parent == Some(parent)));
        state.activate_profile(Some("editor_user"));
        state.moderate_content(&parent_removal, true).unwrap();
        let restored = state
            .comments
            .iter()
            .find(|item| item.id == parent)
            .unwrap();
        assert_eq!(restored.body, "Original comment");
        assert_eq!(state.comment_score(restored), 2);
        state.moderate_content(&parent_removal, false).unwrap();
        state.activate_profile(Some("post_author"));
        state.delete_post(&form(&[("id", &post_id)])).unwrap();
        assert!(!state.is_comment_removed(parent));
        state.activate_profile(Some("editor_user"));
        assert!(state.moderate_content(&parent_removal, true).is_err());
    }
}

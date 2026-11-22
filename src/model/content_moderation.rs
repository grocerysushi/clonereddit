use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    canonical_u64, checked_handle, decode, encode, number, required, unix_seconds, value,
    ModPermission, Post, State,
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

    fn parse(kind: &str) -> Option<Self> {
        match kind {
            "post" => Some(Self::Post),
            "comment" => Some(Self::Comment),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct ContentRemoval {
    pub kind: ContentKind,
    pub id: u64,
    pub post_id: u64,
    pub community: String,
    pub reason: String,
    pub moderator: String,
    pub removed_at: u64,
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

    fn content_moderation_target(
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
        let (action, notice, result) = if restore {
            let index = existing.ok_or("This post or comment is already visible.")?;
            self.content_removals.remove(index);
            let context = if kind == ContentKind::Comment && self.is_post_removed(post_id) {
                " The discussion is still removed."
            } else {
                ""
            };
            (
                "restored",
                format!("Moderators restored your {label} #{id} in c/{community}.{context}"),
                "Content restored.",
            )
        } else {
            if existing.is_some() {
                return Err("This post or comment is already removed.".into());
            }
            let reason = required(form, "reason", "Removal reason", REASON_LIMIT)?;
            let notice =
                format!("Moderators removed your {label} #{id} in c/{community}.\n\n{reason}");
            self.content_removals.push(ContentRemoval {
                kind,
                id,
                post_id,
                community: community.clone(),
                reason: reason.into(),
                moderator: self.username.clone(),
                removed_at: unix_seconds(),
            });
            ("removed", notice, "Content removed.")
        };
        self.push_moderation_notification(&author, notice)?;
        let log = format!("{} #{id} {action} by u/{}", kind.label(), self.username);
        self.record_moderation_action(&community, &log)?;
        Ok(result)
    }

    pub(super) fn read_content_removal(&mut self, fields: &[&str]) -> Option<()> {
        let ["content-removal", kind, id, community, reason, moderator, removed_at] = fields else {
            return None;
        };
        let kind = ContentKind::parse(kind)?;
        let id = number(id)?;
        let community = decode(community)?;
        let (post_id, _) = self.content_moderation_target(kind, id, &community)?;
        let reason = decode(reason)?;
        let moderator = decode(moderator)?;
        if self.content_removal(kind, id).is_some()
            || reason.trim().is_empty()
            || reason.chars().count() > REASON_LIMIT
            || checked_handle(&moderator).ok()? != moderator
        {
            return None;
        }
        self.content_removals.push(ContentRemoval {
            kind,
            id,
            post_id,
            community,
            reason,
            moderator,
            removed_at: number(removed_at)?,
        });
        Some(())
    }

    pub(super) fn write_content_removals(&self, file: &mut impl Write) -> io::Result<()> {
        for item in &self.content_removals {
            writeln!(
                file,
                "content-removal\t{}\t{}\t{}\t{}\t{}\t{}",
                item.kind.as_str(),
                item.id,
                encode(&item.community),
                encode(&item.reason),
                encode(&item.moderator),
                item.removed_at,
            )?;
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
        assert_eq!(
            state
                .content_removal(ContentKind::Post, id)
                .unwrap()
                .moderator,
            "editor_user"
        );
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
        assert_eq!(state.community_for_post(id).unwrap().pinned_post, Some(id));
        assert!(state.notifications.iter().any(|item| item.moderation
            && item.recipient == "post_author"
            && item.body.starts_with("Moderators restored your post")));
        state.activate_profile(Some("post_author"));
        state.delete_post(&post_action).unwrap();
        state.activate_profile(Some("board_owner"));
        assert!(state.moderate_content(&removal, true).is_err());
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

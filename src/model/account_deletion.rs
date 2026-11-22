use super::State;

pub const DELETED_ACCOUNT: &str = "[deleted]";

impl State {
    pub fn delete_account_profile(&mut self, handle: &str) {
        if self.username == handle {
            self.activate_profile(None);
        }
        self.account_profiles.remove(handle);
        if self.legacy_profile.as_deref() == Some(handle) {
            self.legacy_profile = None;
        }

        self.pending_artwork.retain(|entry| entry.creator != handle);
        self.notifications.retain(|entry| entry.recipient != handle);
        self.moderator_roles.retain(|entry| entry.handle != handle);
        self.approved_members.retain(|entry| entry.handle != handle);
        self.delete_account_chat_requests(handle);
        self.content_appeals.retain(|case| case.appellant != handle);
        for case in &mut self.content_appeals {
            if case.reviewer.as_deref() == Some(handle) {
                case.reviewer = Some(DELETED_ACCOUNT.into());
            }
        }

        for post in &mut self.posts {
            if post.author == handle {
                post.author = DELETED_ACCOUNT.into();
            }
            if let Some(poll) = post.poll.as_mut() {
                poll.votes.remove(handle);
            }
        }
        for comment in &mut self.comments {
            if comment.author == handle {
                comment.author = DELETED_ACCOUNT.into();
            }
        }

        // Old schedules without an author were attributed to the community owner.
        for entry in &mut self.scheduled {
            let author = entry.author.as_deref().or_else(|| {
                self.communities
                    .iter()
                    .find(|item| item.slug == entry.community)
                    .and_then(|item| item.owner.as_deref())
            });
            if author == Some(handle) {
                if entry.status == "pending" {
                    entry.status = "canceled".into();
                }
                entry.author = Some(DELETED_ACCOUNT.into());
            }
        }
        for community in &mut self.communities {
            if community.owner.as_deref() == Some(handle) {
                community.owner = Some(DELETED_ACCOUNT.into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf, time::SystemTime};

    use crate::model::{unix_seconds, ModPermission};

    use super::*;

    const OWNER: &str = "departing_owner";
    const MODERATOR: &str = "staying_mod";
    const MEMBER: &str = "staying_member";
    const ROOM: &str = "private-room";

    struct StateFile(PathBuf);

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
    fn account_cleanup_keeps_threads_and_existing_community_permissions_after_reload() {
        let path = StateFile(std::env::temp_dir().join(format!(
            "commonline-account-cleanup-{}-{:?}.state",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
        )));
        let mut state = State::seeded();
        state.activate_profile(Some(OWNER));
        state.legacy_profile = Some(OWNER.into());
        state.display_name = "Name to remove".into();
        state
            .create_community(&form(&[
                ("slug", ROOM),
                ("name", "Private room"),
                ("visibility", "private"),
            ]))
            .unwrap();
        state
            .set_moderator_role(&form(&[
                ("community", ROOM),
                ("handle", MODERATOR),
                ("role", "moderator"),
            ]))
            .unwrap();
        state
            .approve_member(&form(&[("community", ROOM), ("handle", MEMBER)]))
            .unwrap();
        state
            .create_poll(&form(&[
                ("community", ROOM),
                ("title", "Discussion to keep"),
                ("option_1", "First"),
                ("option_2", "Second"),
                ("duration", "1"),
            ]))
            .unwrap();
        let post_id = state.posts.last().unwrap().id;
        let post = post_id.to_string();
        state
            .vote_poll(&form(&[("id", &post), ("option", "0")]))
            .unwrap();
        state
            .create_comment(&form(&[("post", &post), ("body", "Comment to keep")]))
            .unwrap();
        let comment_id = state.comments.last().unwrap().id;
        state
            .save_draft(&form(&[("community", ROOM), ("title", "Draft to remove")]))
            .unwrap();
        state
            .request_chat(
                &form(&[("recipient", MODERATOR), ("body", "Kept by recipient")]),
                false,
                &path.0,
            )
            .unwrap();
        let publish_at = (unix_seconds() + 3_600).to_string();
        state
            .schedule_item(&form(&[
                ("community", ROOM),
                ("kind", "post"),
                ("title", "Pending owner's post"),
                ("publish_at", &publish_at),
            ]))
            .unwrap();
        state.scheduled.last_mut().unwrap().author = None;

        state.activate_profile(Some(MODERATOR));
        let request_id = state
            .incoming_chat_requests()
            .next()
            .unwrap()
            .id
            .to_string();
        state
            .apply(
                "accept-chat-request",
                &form(&[("id", &request_id)]),
                &path.0,
            )
            .unwrap();
        state
            .vote_poll(&form(&[("id", &post), ("option", "1")]))
            .unwrap();
        state
            .create_comment(&form(&[
                ("post", &post),
                ("parent", &comment_id.to_string()),
                ("body", "Child reply to keep"),
            ]))
            .unwrap();
        state
            .schedule_item(&form(&[
                ("community", ROOM),
                ("kind", "post"),
                ("title", "Pending moderator's post"),
                ("publish_at", &publish_at),
            ]))
            .unwrap();
        assert!(state
            .notifications
            .iter()
            .any(|item| item.recipient == OWNER));

        state.activate_profile(Some(OWNER));
        state.delete_account_profile(OWNER);
        state.delete_account_profile(OWNER);
        assert!(state.username.is_empty());
        assert!(state.draft.is_none());
        state.save(&path.0).unwrap();
        let mut state = State::load(&path.0).unwrap();
        assert!(!state.has_profile(OWNER));
        assert_eq!(state.legacy_username(), None);
        assert!(state
            .notifications
            .iter()
            .all(|item| item.recipient != OWNER));
        assert!(state
            .messages
            .iter()
            .any(|item| item.from == OWNER && item.to == MODERATOR));
        let post = state.posts.iter().find(|item| item.id == post_id).unwrap();
        assert_eq!(post.author, DELETED_ACCOUNT);
        assert_eq!(post.title, "Discussion to keep");
        assert_eq!(post.poll.as_ref().unwrap().votes.len(), 1);
        assert!(post.poll.as_ref().unwrap().votes.contains_key(MODERATOR));
        let comment = state
            .comments
            .iter()
            .find(|item| item.id == comment_id)
            .unwrap();
        assert_eq!(comment.author, DELETED_ACCOUNT);
        assert_eq!(comment.body, "Comment to keep");
        assert!(state
            .comments
            .iter()
            .any(|item| item.parent == Some(comment_id) && item.author == MODERATOR));
        assert_eq!(state.scheduled[0].status, "canceled");
        assert_eq!(state.scheduled[0].author.as_deref(), Some(DELETED_ACCOUNT));
        assert_eq!(state.scheduled[1].status, "pending");
        assert_eq!(state.scheduled[1].author.as_deref(), Some(MODERATOR));

        let community = state
            .communities
            .iter()
            .find(|item| item.slug == ROOM)
            .unwrap()
            .clone();
        assert_eq!(community.owner.as_deref(), Some(DELETED_ACCOUNT));
        assert!(!state.can_view_community(&community));
        state.activate_profile(Some(MEMBER));
        assert!(state.can_contribute(&community));
        state.activate_profile(Some(MODERATOR));
        assert!(state.can_moderate(&community, ModPermission::Members));
        assert!(state
            .owned_community_index(&form(&[("community", ROOM)]))
            .is_err());
    }
}

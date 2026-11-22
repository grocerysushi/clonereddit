use std::collections::BTreeMap;

use super::{id_field, ContentKind, ModPermission, State};

impl State {
    pub fn is_discussion_locked(&self, post_id: u64) -> bool {
        self.locked_discussions.contains(&post_id)
    }

    pub(super) fn set_discussion_lock(
        &mut self,
        form: &BTreeMap<String, String>,
        locked: bool,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let community = self.communities[index].slug.clone();
        let id = id_field(form, "id")?;
        self.content_moderation_target(ContentKind::Post, id, &community)
            .ok_or("This discussion is unavailable in this community.")?;
        let (changed, action, notice) = if locked {
            (
                self.locked_discussions.insert(id),
                "locked",
                "Discussion locked.",
            )
        } else {
            (
                self.locked_discussions.remove(&id),
                "unlocked",
                "Discussion unlocked.",
            )
        };
        if changed {
            self.record_moderation_action(
                &community,
                &format!("Discussion #{id} {action} by u/{}", self.username),
            )?;
        }
        Ok(notice)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const ROOM: &str = "discussion-lock-room";
    const OTHER_ROOM: &str = "unrelated-lock-room";
    const OWNER: &str = "discussion_owner";
    const MODERATOR: &str = "discussion_mod";
    const REVIEWER: &str = "discussion_reviewer";
    const MEMBER: &str = "discussion_member";
    const OTHER_OWNER: &str = "discussion_other";

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "commonline-discussion-lock-{}-{}.state",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
            )))
        }
    }

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
    fn locking_enforces_discussion_and_community_scope_across_the_post_lifecycle() {
        let fixture = Fixture::new();
        let mut state = State::seeded();
        state.activate_profile(Some(OTHER_OWNER));
        state
            .create_community(&form(&[
                ("slug", OTHER_ROOM),
                ("name", "Unrelated community"),
            ]))
            .unwrap();
        state.activate_profile(Some(OWNER));
        state
            .create_community(&form(&[
                ("slug", ROOM),
                ("name", "Discussion lock community"),
            ]))
            .unwrap();
        for (handle, role) in [(MODERATOR, "moderator"), (REVIEWER, "appeals-reviewer")] {
            state
                .set_moderator_role(&form(&[
                    ("community", ROOM),
                    ("handle", handle),
                    ("role", role),
                ]))
                .unwrap();
        }
        state
            .create_post(&form(&[
                ("community", ROOM),
                ("title", "Discussion to lock"),
            ]))
            .unwrap();
        let post_id = state.posts.last().unwrap().id;
        let post = post_id.to_string();
        state.activate_profile(Some(MEMBER));
        let topic = form(&[("post", &post), ("body", "An ordinary comment")]);
        state.apply("comment", &topic, &fixture.0).unwrap();
        let parent_id = state.comments.last().unwrap().id;
        let parent = parent_id.to_string();
        let reply = form(&[
            ("post", &post),
            ("parent", &parent),
            ("body", "A nested reply"),
        ]);
        state.apply("comment", &reply, &fixture.0).unwrap();
        let child = state.comments.last().unwrap().id.to_string();

        let mut state = State::load(&fixture.0).unwrap();
        assert!(!state.is_discussion_locked(post_id));
        let target = form(&[("community", ROOM), ("id", &post)]);
        for outsider in [MEMBER, REVIEWER, OTHER_OWNER] {
            state.activate_profile(Some(outsider));
            assert!(state.apply("lock-discussion", &target, &fixture.0).is_err());
        }
        let mismatched = form(&[("community", OTHER_ROOM), ("id", &post)]);
        assert!(state
            .apply("lock-discussion", &mismatched, &fixture.0)
            .is_err());
        state.activate_profile(Some(MODERATOR));
        let locked_log = format!("Discussion #{post_id} locked by u/{MODERATOR}");
        state.apply("lock-discussion", &target, &fixture.0).unwrap();
        state.apply("lock-discussion", &target, &fixture.0).unwrap();
        assert_eq!(
            state
                .mod_log
                .iter()
                .filter(|event| event.action == locked_log)
                .count(),
            1
        );

        let mut state = State::load(&fixture.0).unwrap();
        assert!(state.is_discussion_locked(post_id));
        for viewer in [OWNER, MODERATOR, MEMBER] {
            state.activate_profile(Some(viewer));
            assert_eq!(
                state.apply("comment", &topic, &fixture.0).unwrap_err(),
                "This discussion is locked."
            );
            assert_eq!(
                state.apply("comment", &reply, &fixture.0).unwrap_err(),
                "This discussion is locked."
            );
        }
        assert!(state
            .apply("unlock-discussion", &target, &fixture.0)
            .is_err());
        state
            .apply(
                "edit-comment",
                &form(&[("id", &parent), ("body", "Existing comment edited")]),
                &fixture.0,
            )
            .unwrap();
        state
            .apply("delete-comment", &form(&[("id", &child)]), &fixture.0)
            .unwrap();
        assert_eq!(
            state
                .comments
                .iter()
                .find(|comment| comment.id == parent_id)
                .unwrap()
                .body,
            "Existing comment edited"
        );

        state.activate_profile(Some(MODERATOR));
        let moderation = form(&[
            ("community", ROOM),
            ("kind", "post"),
            ("id", &post),
            ("reason", "Removal test"),
        ]);
        state
            .apply("remove-content", &moderation, &fixture.0)
            .unwrap();
        let mut state = State::load(&fixture.0).unwrap();
        assert!(state.is_post_removed(post_id));
        assert!(state.is_discussion_locked(post_id));
        state.activate_profile(Some(MODERATOR));
        state
            .apply("restore-content", &moderation, &fixture.0)
            .unwrap();
        assert!(state.is_discussion_locked(post_id));
        assert_eq!(
            state.apply("comment", &reply, &fixture.0).unwrap_err(),
            "This discussion is locked."
        );
        state
            .apply("unlock-discussion", &target, &fixture.0)
            .unwrap();
        state
            .apply("unlock-discussion", &target, &fixture.0)
            .unwrap();
        let unlocked_log = format!("Discussion #{post_id} unlocked by u/{MODERATOR}");
        assert_eq!(
            state
                .mod_log
                .iter()
                .filter(|event| event.action == unlocked_log)
                .count(),
            1
        );
        state.activate_profile(Some(MEMBER));
        state.apply("comment", &reply, &fixture.0).unwrap();

        state.activate_profile(Some(OWNER));
        state.apply("lock-discussion", &target, &fixture.0).unwrap();
        state
            .apply("delete", &form(&[("id", &post)]), &fixture.0)
            .unwrap();
        assert!(!state.is_discussion_locked(post_id));
        let snapshot = fs::read_to_string(&fixture.0).unwrap();
        assert!(!snapshot
            .lines()
            .any(|line| line.starts_with("discussion-lock\t")));
        let state = State::load(&fixture.0).unwrap();
        assert!(!state.is_discussion_locked(post_id));
    }
}

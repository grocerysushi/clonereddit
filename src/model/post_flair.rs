use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    bounded, canonical_u64, decode, encode, next_id, required, value, Community, ModPermission,
    ScheduledItem, State,
};

pub const COMMUNITY_POST_FLAIR_LABEL_MAX: usize = 32;

#[derive(Clone)]
pub struct CommunityPostFlair {
    pub id: u64,
    pub label: String,
}

pub(super) fn parse_flair_id(raw: &str) -> Option<u64> {
    canonical_u64(raw).filter(|&id| id > 0)
}

pub(super) fn requested_flair_id(form: &BTreeMap<String, String>) -> Result<Option<u64>, String> {
    match value(form, "flair_id") {
        "" => Ok(None),
        raw => parse_flair_id(raw)
            .map(Some)
            .ok_or_else(|| "Choose a valid post flair.".into()),
    }
}

impl Community {
    pub fn post_flairs(&self) -> &[CommunityPostFlair] {
        &self.post_flairs
    }

    pub fn requires_post_flair(&self) -> bool {
        self.require_post_flair
    }

    pub(super) fn flair_for_id(&self, id: u64) -> Option<&CommunityPostFlair> {
        self.post_flairs.iter().find(|flair| flair.id == id)
    }

    pub(super) fn selected_post_flair(
        &self,
        form: &BTreeMap<String, String>,
    ) -> Result<Option<&CommunityPostFlair>, String> {
        requested_flair_id(form)?
            .map(|id| {
                self.flair_for_id(id)
                    .ok_or_else(|| "This post flair is unavailable in the chosen community.".into())
            })
            .transpose()
    }

    pub(super) fn publication_flair<'a>(
        &'a self,
        form: &'a BTreeMap<String, String>,
        fallback: &'a str,
        legacy_fallback: &'a str,
    ) -> Result<(&'a str, Option<u64>), String> {
        if let Some(flair) = self.selected_post_flair(form)? {
            return Ok((&flair.label, Some(flair.id)));
        }
        if self.requires_post_flair() {
            return Err("Choose a post flair for this community before publishing.".into());
        }
        if self.post_flairs.is_empty() {
            let legacy = legacy_post_flair(form)?;
            let label = if legacy.is_empty() {
                legacy_fallback
            } else {
                legacy
            };
            return Ok((label, None));
        }
        Ok((fallback, None))
    }

    pub(super) fn write_post_flairs(&self, file: &mut impl Write) -> io::Result<()> {
        for flair in &self.post_flairs {
            writeln!(
                file,
                "community-post-flair\t{}\t{}\t{}",
                encode(&self.slug),
                flair.id,
                encode(&flair.label)
            )?;
        }
        if self.requires_post_flair() {
            writeln!(file, "community-post-flair-policy\t{}", encode(&self.slug))?;
        }
        Ok(())
    }
}

impl State {
    pub fn scheduled_flair_blocked(&self, item: &ScheduledItem) -> bool {
        self.communities.iter().any(|community| {
            community.slug == item.community
                && community.requires_post_flair()
                && item
                    .flair_id
                    .and_then(|id| community.flair_for_id(id))
                    .is_none()
        })
    }

    pub(super) fn save_post_flair(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let community = &mut self.communities[index];
        let edit = if value(form, "id").is_empty() {
            None
        } else {
            Some(flair_position(&community.post_flairs, form)?)
        };
        let label = required(form, "label", "Flair label", COMMUNITY_POST_FLAIR_LABEL_MAX)?;
        if label.chars().any(char::is_control) {
            return Err("Enter the flair label on one line.".into());
        }
        let folded = label.to_lowercase();
        if community
            .post_flairs
            .iter()
            .enumerate()
            .any(|(position, flair)| Some(position) != edit && flair.label.to_lowercase() == folded)
        {
            return Err("A post flair with that label already exists in this community.".into());
        }
        let (id, action, notice) = if let Some(position) = edit {
            let flair = &mut community.post_flairs[position];
            if flair.label == label {
                return Ok("Post flair updated.");
            }
            flair.label = label.into();
            (flair.id, "updated", "Post flair updated.")
        } else {
            let id = next_id(&mut self.next_post_flair)?;
            community.post_flairs.push(CommunityPostFlair {
                id,
                label: label.into(),
            });
            (id, "added", "Post flair added.")
        };
        let slug = community.slug.clone();
        self.record_moderation_action(
            &slug,
            &format!("Post flair #{id} {action} by u/{}", self.username),
        )?;
        Ok(notice)
    }

    pub(super) fn delete_post_flair(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let community = &mut self.communities[index];
        let position = flair_position(&community.post_flairs, form)?;
        if community.requires_post_flair() && community.post_flairs.len() == 1 {
            return Err("Make post flair optional before deleting the only label.".into());
        }
        let id = community.post_flairs.remove(position).id;
        let slug = community.slug.clone();
        self.record_moderation_action(
            &slug,
            &format!("Post flair #{id} deleted by u/{}", self.username),
        )?;
        Ok("Post flair deleted.")
    }

    pub(super) fn set_post_flair_policy(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let community = &mut self.communities[index];
        let required = match value(form, "required") {
            "" => false,
            "on" => true,
            _ => return Err("Choose whether this community requires post flair.".into()),
        };
        let setting = if required { "required" } else { "optional" };
        if required && community.post_flairs.is_empty() {
            return Err("Add at least one post flair before making it required.".into());
        }
        if community.require_post_flair != required {
            community.require_post_flair = required;
            let slug = community.slug.clone();
            self.record_moderation_action(
                &slug,
                &format!("Post flair made {setting} by u/{}", self.username),
            )?;
        }
        Ok(if required {
            "Post flair is required."
        } else {
            "Post flair is optional."
        })
    }

    pub(super) fn read_post_flair(&mut self, slug: &str, id: &str, label: &str) -> Option<()> {
        let slug = decode(slug)?;
        let id = parse_flair_id(id)?;
        let label = decode(label)?;
        if label.is_empty()
            || label != label.trim()
            || label.chars().count() > COMMUNITY_POST_FLAIR_LABEL_MAX
            || label.chars().any(char::is_control)
            || self
                .communities
                .iter()
                .any(|community| community.flair_for_id(id).is_some())
        {
            return None;
        }
        let community = self
            .communities
            .iter_mut()
            .find(|community| community.slug == slug && community.owner.is_some())?;
        let folded = label.to_lowercase();
        if community
            .post_flairs
            .iter()
            .any(|flair| flair.label.to_lowercase() == folded)
        {
            return None;
        }
        self.next_post_flair = self.next_post_flair.max(id.checked_add(1)?);
        community.post_flairs.push(CommunityPostFlair { id, label });
        Some(())
    }

    pub(super) fn read_post_flair_policy(&mut self, slug: &str) -> Option<()> {
        let slug = decode(slug)?;
        let community = self.communities.iter_mut().find(|community| {
            community.slug == slug && community.owner.is_some() && !community.post_flairs.is_empty()
        })?;
        community.require_post_flair = true;
        Some(())
    }
}

pub(super) fn legacy_post_flair(form: &BTreeMap<String, String>) -> Result<&str, String> {
    bounded(form, "flair", "Flair", COMMUNITY_POST_FLAIR_LABEL_MAX)
}

fn flair_position(
    flairs: &[CommunityPostFlair],
    form: &BTreeMap<String, String>,
) -> Result<usize, String> {
    let id = parse_flair_id(value(form, "id")).ok_or("Choose a valid post flair.")?;
    flairs
        .iter()
        .position(|flair| flair.id == id)
        .ok_or_else(|| "This post flair is unavailable in this community.".into())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::{unix_seconds, PostMedia};
    use super::*;

    const ROOM: &str = "flair-workshop";
    const OTHER: &str = "flair-archive";
    const OWNER: &str = "flair_owner";

    struct TestState {
        state: State,
        file: PathBuf,
    }

    impl TestState {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let file = std::env::temp_dir().join(format!(
                "commonline-flair-{}-{nonce}.state",
                std::process::id()
            ));
            let mut state = State::seeded();
            state.activate_profile(Some(OWNER));
            let mut test = Self { state, file };
            for slug in [ROOM, OTHER] {
                test.apply("create-community", &[("slug", slug), ("name", slug)])
                    .unwrap();
            }
            test
        }

        fn apply(&mut self, action: &str, fields: &[(&str, &str)]) -> Result<&'static str, String> {
            self.state.apply(action, &form(fields), &self.file)
        }

        fn add_flair(&mut self, community: &str, label: &str) -> String {
            self.apply(
                "save-post-flair",
                &[("community", community), ("label", label)],
            )
            .unwrap();
            self.community(community)
                .post_flairs()
                .last()
                .unwrap()
                .id
                .to_string()
        }

        fn set_required(&mut self, required: &str) -> Result<&'static str, String> {
            self.apply(
                "set-post-flair-policy",
                &[("community", ROOM), ("required", required)],
            )
        }

        fn community(&self, slug: &str) -> &Community {
            self.state
                .communities
                .iter()
                .find(|community| community.slug == slug)
                .unwrap()
        }

        fn reload(&mut self) {
            self.state = State::load(&self.file).unwrap();
        }
    }

    impl Drop for TestState {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.file);
        }
    }

    fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|&(key, value)| (key.into(), value.into()))
            .collect()
    }

    #[test]
    fn flair_catalog_enforces_permissions_and_preserves_identity() {
        let mut test = TestState::new();
        assert!(test.set_required("on").is_err());
        for (handle, role) in [
            ("flair_moderator", "moderator"),
            ("flair_reviewer", "appeals-reviewer"),
        ] {
            test.apply(
                "set-moderator-role",
                &[("community", ROOM), ("handle", handle), ("role", role)],
            )
            .unwrap();
        }
        let create = [("community", ROOM), ("label", "Announcements")];
        for handle in ["flair_member", "flair_reviewer"] {
            test.state.activate_profile(Some(handle));
            assert!(test.apply("save-post-flair", &create).is_err());
            assert!(test.set_required("on").is_err());
        }
        test.state.activate_profile(Some("flair_moderator"));
        let first = test.add_flair(ROOM, "Announcements");
        assert!(test
            .apply(
                "save-post-flair",
                &[("community", ROOM), ("label", " announcements ")]
            )
            .is_err());
        assert!(test
            .apply(
                "save-post-flair",
                &[("community", ROOM), ("label", "Multi\nline")]
            )
            .is_err());
        assert!(test
            .apply(
                "save-post-flair",
                &[("community", ROOM), ("label", &"a".repeat(33))]
            )
            .is_err());
        assert!(test
            .apply(
                "save-post-flair",
                &[("community", OTHER), ("label", "Announcements")]
            )
            .is_err());
        let delete = [("community", ROOM), ("id", &first)];
        test.set_required("on").unwrap();
        assert!(test.apply("delete-post-flair", &delete).is_err());
        test.reload();
        assert!(test.community(ROOM).requires_post_flair());
        assert_eq!(test.community(ROOM).post_flairs()[0].label, "Announcements");
        test.state.activate_profile(Some(OWNER));
        let archive = test.add_flair(OTHER, "Announcements");
        assert!(test
            .apply(
                "save-post-flair",
                &[("community", ROOM), ("id", &archive), ("label", "Retitled")]
            )
            .is_err());
        test.set_required("").unwrap();
        test.apply("delete-post-flair", &delete).unwrap();
        test.apply(
            "delete-post-flair",
            &[("community", OTHER), ("id", &archive)],
        )
        .unwrap();
        test.reload();
        test.state.activate_profile(Some(OWNER));
        assert!(test.community(ROOM).post_flairs().is_empty());
        let next = test.add_flair(ROOM, "Announcements");
        assert!(parse_flair_id(&next) > parse_flair_id(&archive));
        assert!(test
            .state
            .mod_log
            .iter()
            .any(|event| event.action.contains("Post flair made required")));
    }

    #[test]
    fn publishing_validates_every_destination_and_keeps_historical_labels() {
        let mut test = TestState::new();
        test.apply(
            "create",
            &[
                ("community", ROOM),
                ("title", "Legacy"),
                ("flair", "Legacy label"),
            ],
        )
        .unwrap();
        let legacy = test.state.posts.last().unwrap().id.to_string();
        let selected = test.add_flair(ROOM, "Question");
        let replacement = test.add_flair(ROOM, "Guide");
        let foreign = test.add_flair(OTHER, "Question");
        test.apply(
            "create",
            &[
                ("community", ROOM),
                ("title", "Optional"),
                ("flair", "Pretend flair"),
            ],
        )
        .unwrap();
        assert_eq!(test.state.posts.last().unwrap().flair, "Discussion");
        test.set_required("on").unwrap();
        let fields = [
            ("community", ROOM),
            ("title", "New post"),
            ("url", "https://example.org/topic"),
            ("flair", "Question"),
            ("option_1", "First"),
            ("option_2", "Second"),
            ("duration", "1"),
            ("source", "1"),
        ];
        let media = [PostMedia {
            key: "a".repeat(48),
            content_type: "image/png".into(),
        }];
        for action in ["create", "create-poll", "create-repost"] {
            assert!(test
                .apply(action, &fields)
                .unwrap_err()
                .contains("before publishing"));
        }
        assert!(test
            .state
            .create_media_post(&form(&fields), media.to_vec(), &test.file)
            .unwrap_err()
            .contains("before publishing"));
        for invalid in [&foreign, "bad", "0"] {
            let mut attempted = fields.to_vec();
            attempted.push(("flair_id", invalid));
            assert!(test.apply("create", &attempted).is_err());
        }
        let mut fields = fields.to_vec();
        fields.push(("flair_id", &selected));
        for action in ["create", "create-poll", "create-repost"] {
            test.apply(action, &fields).unwrap();
            assert_eq!(test.state.posts.last().unwrap().flair, "Question");
        }
        test.state
            .create_media_post(&form(&fields), media.to_vec(), &test.file)
            .unwrap();
        let published = test.state.posts.last().unwrap().id.to_string();
        test.apply(
            "save-post-flair",
            &[("community", ROOM), ("id", &selected), ("label", "Renamed")],
        )
        .unwrap();
        test.apply(
            "delete-post-flair",
            &[("community", ROOM), ("id", &selected)],
        )
        .unwrap();
        for (id, label) in [(&legacy, "Legacy label"), (&published, "Question")] {
            test.apply(
                "edit",
                &[
                    ("id", id),
                    ("title", "Updated title"),
                    ("flair", "Pretend flair"),
                ],
            )
            .unwrap();
            assert_eq!(
                test.state
                    .posts
                    .iter()
                    .find(|post| post.id.to_string() == *id)
                    .unwrap()
                    .flair,
                label
            );
        }
        fields.pop();
        fields.push(("flair_id", &replacement));
        test.apply("create", &fields).unwrap();
        assert_eq!(test.state.posts.last().unwrap().flair, "Guide");
    }

    #[test]
    fn drafts_keep_flair_selection_without_requiring_a_finished_choice() {
        let mut test = TestState::new();
        let selected = test.add_flair(ROOM, "Question");
        test.set_required("on").unwrap();
        let fields = [("community", ROOM), ("title", "In progress")];
        test.apply("save-draft", &fields).unwrap();
        assert!(test.state.drafts[0].flair_id.is_none());
        test.apply(
            "save-draft",
            &[
                ("community", ROOM),
                ("title", "With flair"),
                ("flair_id", &selected),
            ],
        )
        .unwrap();
        test.state.activate_profile(Some("flair_other_member"));
        test.apply("save-draft", &fields).unwrap();
        test.reload();
        test.state.activate_profile(Some(OWNER));
        assert_eq!(test.state.drafts.len(), 2);
        assert!(test.state.drafts[0].flair_id.is_none());
        assert_eq!(test.state.drafts[1].flair_id, parse_flair_id(&selected));
        assert_eq!(test.state.drafts[1].flair, "Question");
    }

    #[test]
    fn due_schedules_hold_only_items_that_lack_required_flair() {
        let mut test = TestState::new();
        let retired = test.add_flair(ROOM, "Retired");
        let active = test.add_flair(ROOM, "Active");
        let publish_at = (unix_seconds() + 90).to_string();
        let pending = [
            ("community", ROOM),
            ("kind", "post"),
            ("title", "Scheduled"),
            ("publish_at", &publish_at),
        ];
        test.apply("schedule-item", &pending).unwrap();
        for (id, kind) in [(&retired, "post"), (&active, "event")] {
            test.apply(
                "schedule-item",
                &[
                    ("community", ROOM),
                    ("kind", kind),
                    ("title", "Selected"),
                    ("publish_at", &publish_at),
                    ("event_start", &publish_at),
                    ("flair_id", id),
                ],
            )
            .unwrap();
        }
        test.set_required("on").unwrap();
        assert!(test.apply("schedule-item", &pending).is_err());
        test.apply(
            "save-post-flair",
            &[("community", ROOM), ("id", &active), ("label", "Current")],
        )
        .unwrap();
        test.apply(
            "delete-post-flair",
            &[("community", ROOM), ("id", &retired)],
        )
        .unwrap();
        test.reload();
        assert!(test.state.scheduled_flair_blocked(&test.state.scheduled[0]));
        assert!(test.state.scheduled_flair_blocked(&test.state.scheduled[1]));
        assert!(!test.state.scheduled_flair_blocked(&test.state.scheduled[2]));
        assert_eq!(
            test.state
                .publish_due(&test.file, publish_at.parse().unwrap())
                .unwrap(),
            1
        );
        assert_eq!(test.state.scheduled[0].status, "pending");
        assert_eq!(test.state.scheduled[1].status, "pending");
        assert_eq!(test.state.scheduled[2].status, "published");
        assert_eq!(test.state.posts.last().unwrap().flair, "Current");
        test.state.activate_profile(Some(OWNER));
        test.state.scheduled[0].publish_at = 1;
        test.state.scheduled[1].publish_at = 1;
        let id = test.state.scheduled[0].id.to_string();
        let retired_id = test.state.scheduled[1].id.to_string();
        test.apply("cancel-scheduled-item", &[("community", ROOM), ("id", &id)])
            .unwrap();
        let mut reschedule = pending.to_vec();
        reschedule.extend([("id", retired_id.as_str()), ("flair_id", active.as_str())]);
        test.apply("edit-scheduled-item", &reschedule).unwrap();
        assert_eq!(
            test.state
                .publish_due(&test.file, publish_at.parse().unwrap())
                .unwrap(),
            1
        );
        assert_eq!(test.state.posts.last().unwrap().flair, "Current");
    }
}

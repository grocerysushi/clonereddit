use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    canonical_u64, decode, encode, next_id, number, optional_number, required, unix_seconds, value,
    AppealStatus, ContentKind, ContentRemoval, ModPermission, State,
};

#[derive(Clone)]
pub struct ContentAppeal {
    pub id: u64,
    pub removal: u64,
    pub kind: ContentKind,
    pub content_id: u64,
    pub post_id: u64,
    pub community: String,
    pub appellant: String,
    pub body: String,
    pub status: AppealStatus,
    pub response: String,
    pub reviewer: Option<String>,
    pub created_at: u64,
    pub decided_at: Option<u64>,
    pub reason: String,
}

impl State {
    pub fn removal_for_author(&self, episode: u64) -> Option<&ContentRemoval> {
        let removal = self
            .content_removals
            .iter()
            .find(|item| item.episode == episode)?;
        self.is_removal_author(removal).then_some(removal)
    }

    pub fn can_appeal_removal(&self, removal: &ContentRemoval) -> bool {
        self.is_removal_author(removal)
            && !self
                .content_appeals
                .iter()
                .any(|case| case.removal == removal.episode)
    }

    fn is_removal_author(&self, removal: &ContentRemoval) -> bool {
        !self.username.is_empty()
            && self
                .content_moderation_target(removal.kind, removal.id, &removal.community)
                .is_some_and(|(_, author)| author == self.username)
    }

    pub(super) fn submit_content_appeal(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let episode = canonical_u64(value(form, "removal")).ok_or("Choose an active removal.")?;
        let removal = self
            .removal_for_author(episode)
            .ok_or("Choose one of your active removals.")?;
        if !self.can_appeal_removal(removal) {
            return Err("You have already appealed this removal.".into());
        }
        let body = required(form, "body", "Appeal", 4_000)?;
        let reason = removal.reason_summary();
        let (kind, content_id, post_id, community) = (
            removal.kind,
            removal.id,
            removal.post_id,
            removal.community.clone(),
        );
        self.content_appeals.push(ContentAppeal {
            id: next_id(&mut self.next_appeal)?,
            removal: episode,
            kind,
            content_id,
            post_id,
            community,
            appellant: self.username.clone(),
            body: body.into(),
            status: AppealStatus::Pending,
            response: String::new(),
            reviewer: None,
            created_at: unix_seconds(),
            decided_at: None,
            reason,
        });
        Ok("Appeal submitted.")
    }

    pub(super) fn decide_content_appeal(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let scope = self.moderated_community_index(form, ModPermission::Appeals)?;
        let community = &self.communities[scope].slug;
        let id = canonical_u64(value(form, "appeal")).ok_or("Choose a valid appeal.")?;
        let index = self
            .content_appeals
            .iter()
            .position(|case| case.id == id && &case.community == community)
            .ok_or("This appeal is unavailable in this community.")?;
        let case = &self.content_appeals[index];
        if case.status != AppealStatus::Pending {
            return Err("This appeal has already been resolved.".into());
        }
        let (status, notice, outcome) = match value(form, "decision") {
            "approve" => (AppealStatus::Approved, "Appeal approved.", "approved"),
            "deny" => (AppealStatus::Denied, "Appeal denied.", "denied"),
            _ => return Err("Choose whether to approve or deny the appeal.".into()),
        };
        let response = required(form, "response", "Decision explanation", 4_000)?;
        let removal_index = self
            .content_removals
            .iter()
            .position(|item| {
                item.episode == case.removal
                    && item.kind == case.kind
                    && item.id == case.content_id
                    && item.post_id == case.post_id
                    && item.community == case.community
            })
            .ok_or("The removal attached to this appeal is no longer active.")?;
        let (post_id, author) = self
            .content_moderation_target(case.kind, case.content_id, &case.community)
            .ok_or("The content attached to this appeal is unavailable.")?;
        if post_id != case.post_id || author != case.appellant {
            return Err("The content attached to this appeal is unavailable.".into());
        }

        let context = if status == AppealStatus::Approved {
            let removal = self.restore_content_removal(removal_index)?;
            self.restoration_context(&removal)
        } else {
            ""
        };
        let case = &mut self.content_appeals[index];
        case.status = status;
        case.response = response.into();
        case.reviewer = Some(self.username.clone());
        case.decided_at = Some(unix_seconds());

        let community = case.community.clone();
        let appellant = case.appellant.clone();
        let kind = case.kind.as_str();
        let target = case.content_id;
        let episode = case.removal;
        self.push_removal_notification(
            &appellant,
            format!("Your appeal for {kind} #{target} in c/{community} was {outcome}.{context}"),
            episode,
        )?;
        self.record_moderation_action(
            &community,
            &format!("Appeal #{id} {outcome} by u/{}", self.username),
        )?;
        Ok(notice)
    }

    pub(super) fn resolve_manually_restored_content_appeal(&mut self, episode: u64) {
        let Some(case) = self
            .content_appeals
            .iter_mut()
            .find(|case| case.removal == episode && case.status == AppealStatus::Pending)
        else {
            return;
        };
        case.status = AppealStatus::Restored;
        case.response = "The moderation team restored this content.".into();
        case.reviewer = Some(self.username.clone());
        case.decided_at = Some(unix_seconds());
    }

    pub(super) fn resolve_deleted_content_appeals(&mut self, post_id: u64) {
        let now = unix_seconds();
        for case in self
            .content_appeals
            .iter_mut()
            .filter(|case| case.post_id == post_id && case.status == AppealStatus::Pending)
        {
            case.status = AppealStatus::Unavailable;
            case.response = "The discussion was deleted.".into();
            case.decided_at = Some(now);
        }
    }

    pub(super) fn read_content_appeal_record(&mut self, fields: &[&str]) -> Option<()> {
        let ["content-appeal", id, removal, kind, content_id, post_id, community, appellant, body, status, response, reviewer, created, decided, reason] =
            fields
        else {
            return None;
        };
        let id = canonical_u64(id).filter(|&id| id > 0)?;
        let removal = canonical_u64(removal).filter(|&id| id > 0)?;
        let status = AppealStatus::from_record(status)?;
        if !matches!(
            status,
            AppealStatus::Pending
                | AppealStatus::Approved
                | AppealStatus::Denied
                | AppealStatus::Restored
                | AppealStatus::Unavailable
        ) || self
            .content_appeals
            .iter()
            .any(|case| case.id == id || case.removal == removal)
            || self.appeals.iter().any(|case| case.id == id)
        {
            return None;
        }
        let reviewer = decode(reviewer)?;
        self.content_appeals.push(ContentAppeal {
            id,
            removal,
            kind: ContentKind::parse(kind)?,
            content_id: number(content_id)?,
            post_id: number(post_id)?,
            community: decode(community)?,
            appellant: decode(appellant)?,
            body: decode(body)?,
            status,
            response: decode(response)?,
            reviewer: (!reviewer.is_empty()).then_some(reviewer),
            created_at: number(created)?,
            decided_at: optional_number(decided)?,
            reason: decode(reason)?,
        });
        self.next_appeal = self.next_appeal.max(id.checked_add(1)?);
        self.next_removal_episode = self.next_removal_episode.max(removal.checked_add(1)?);
        Some(())
    }

    pub(super) fn write_content_appeal_records(&self, file: &mut impl Write) -> io::Result<()> {
        for case in &self.content_appeals {
            let decided = case
                .decided_at
                .map_or_else(String::new, |at| at.to_string());
            writeln!(
                file,
                "content-appeal\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{decided}\t{}",
                case.id,
                case.removal,
                case.kind.as_str(),
                case.content_id,
                case.post_id,
                encode(&case.community),
                encode(&case.appellant),
                encode(&case.body),
                case.status.as_record(),
                encode(&case.response),
                encode(case.reviewer.as_deref().unwrap_or_default()),
                case.created_at,
                encode(&case.reason),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf, time::SystemTime};

    use super::*;

    const ROOM: &str = "appeal-removal-workroom";
    const OTHER: &str = "appeal-other-workroom";
    const OWNER: &str = "appeal_mod_owner";
    const REVIEWER: &str = "appeal_mod_reviewer";
    const AUTHOR: &str = "appeal_content_author";
    const OUTSIDER: &str = "appeal_outsider";

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
    fn author_removal_appeals_are_scoped_to_one_episode_and_survive_restart() {
        let fixture = StateFile(std::env::temp_dir().join(format!(
            "commonline-content-appeals-{}-{}.state",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos(),
        )));
        let path = &fixture.0;
        let mut state = State::seeded();
        state.activate_profile(Some(OUTSIDER));
        state
            .create_community(&form(&[("slug", OTHER), ("name", "Other room")]))
            .unwrap();
        state.activate_profile(Some(OWNER));
        state
            .create_community(&form(&[
                ("slug", ROOM),
                ("name", "Review room"),
                ("visibility", "private"),
            ]))
            .unwrap();
        state
            .set_moderator_role(&form(&[
                ("community", ROOM),
                ("handle", REVIEWER),
                ("role", "appeals-reviewer"),
            ]))
            .unwrap();
        let membership = form(&[("community", ROOM), ("handle", AUTHOR)]);
        state.approve_member(&membership).unwrap();
        state
            .create_post(&form(&[
                ("community", ROOM),
                ("title", "Parent discussion"),
            ]))
            .unwrap();
        let parent = state.posts.last().unwrap().id;
        let parent_id = parent.to_string();
        state.activate_profile(Some(AUTHOR));
        state
            .create_post(&form(&[("community", ROOM), ("title", "Post to appeal")]))
            .unwrap();
        let post = state.posts.last().unwrap().id;
        let post_id = post.to_string();
        state
            .create_comment(&form(&[
                ("post", &parent_id),
                ("body", "Comment to appeal"),
            ]))
            .unwrap();
        let comment = state.comments.last().unwrap().id;
        let comment_id = comment.to_string();
        state.activate_profile(Some(OWNER));
        let post_target = form(&[
            ("community", ROOM),
            ("kind", "post"),
            ("id", &post_id),
            ("reason", "Original post reason"),
        ]);
        let comment_target = form(&[
            ("community", ROOM),
            ("kind", "comment"),
            ("id", &comment_id),
            ("reason", "Original comment reason"),
        ]);
        state.apply("remove-content", &post_target, path).unwrap();
        state
            .apply("remove-content", &comment_target, path)
            .unwrap();
        state.revoke_approved_member(&membership).unwrap();
        let post_episode = state
            .content_removal(ContentKind::Post, post)
            .unwrap()
            .episode;
        let comment_episode = state
            .content_removal(ContentKind::Comment, comment)
            .unwrap()
            .episode;
        let post_appeal = form(&[
            ("removal", &post_episode.to_string()),
            ("body", "Please restore my post"),
        ]);
        let comment_appeal = form(&[
            ("removal", &comment_episode.to_string()),
            ("body", "Please restore my comment"),
        ]);
        assert_ne!(post_episode, comment_episode);

        for viewer in [OWNER, REVIEWER, OUTSIDER] {
            state.activate_profile(Some(viewer));
            assert!(state.removal_for_author(post_episode).is_none());
            assert!(state
                .apply("submit-content-appeal", &post_appeal, path)
                .is_err());
        }
        state.activate_profile(Some(AUTHOR));
        let removal = state.removal_for_author(post_episode).unwrap();
        assert!(state.can_appeal_removal(removal));
        state
            .apply("submit-content-appeal", &post_appeal, path)
            .unwrap();
        assert!(state
            .apply("submit-content-appeal", &post_appeal, path)
            .is_err());
        state
            .apply("submit-content-appeal", &comment_appeal, path)
            .unwrap();

        let mut state = State::load(path).unwrap();
        let post_case = state.content_appeals[0].id.to_string();
        let comment_case = state.content_appeals[1].id.to_string();
        let decision = |community: &str, appeal: &str, option: &str, response: &str| {
            form(&[
                ("community", community),
                ("appeal", appeal),
                ("decision", option),
                ("response", response),
            ])
        };
        let approval = decision(
            ROOM,
            &post_case,
            "approve",
            "Reviewed and restored the post",
        );
        state.activate_profile(Some(OUTSIDER));
        assert!(state
            .apply("decide-content-appeal", &approval, path)
            .is_err());
        assert!(state
            .apply(
                "decide-content-appeal",
                &decision(OTHER, &post_case, "approve", "Wrong room"),
                path
            )
            .is_err());
        state.activate_profile(Some(REVIEWER));
        assert!(state.apply("restore-content", &post_target, path).is_err());
        assert!(state
            .apply(
                "decide-content-appeal",
                &decision(ROOM, &post_case, "approve", ""),
                path
            )
            .is_err());
        state
            .apply("decide-content-appeal", &approval, path)
            .unwrap();
        assert!(state
            .apply("decide-content-appeal", &approval, path)
            .is_err());
        state
            .apply(
                "decide-content-appeal",
                &decision(ROOM, &comment_case, "deny", "Comment removal upheld"),
                path,
            )
            .unwrap();
        assert!(!state.is_post_removed(post));
        assert!(state.is_comment_removed(comment));
        assert_eq!(state.content_appeals[0].status, AppealStatus::Approved);
        assert_eq!(
            state.content_appeals[0].response,
            "Reviewed and restored the post"
        );
        assert_eq!(state.content_appeals[1].status, AppealStatus::Denied);
        assert_eq!(state.content_appeals[1].reason, "Original comment reason");
        assert_eq!(
            state
                .notifications
                .iter()
                .filter(|entry| entry.recipient == AUTHOR && entry.body.contains("Your appeal for"))
                .count(),
            2
        );

        state.activate_profile(Some(OWNER));
        state.apply("remove-content", &post_target, path).unwrap();
        state
            .apply("restore-content", &comment_target, path)
            .unwrap();
        state
            .apply("remove-content", &comment_target, path)
            .unwrap();
        state.activate_profile(Some(AUTHOR));
        for (kind, target, old) in [
            (ContentKind::Post, post, post_episode),
            (ContentKind::Comment, comment, comment_episode),
        ] {
            let episode = state.content_removal(kind, target).unwrap().episode;
            assert!(episode > old);
            state
                .apply(
                    "submit-content-appeal",
                    &form(&[
                        ("removal", &episode.to_string()),
                        ("body", "Appeal of the new removal"),
                    ]),
                    path,
                )
                .unwrap();
        }
        state.activate_profile(Some(OWNER));
        state.apply("restore-content", &post_target, path).unwrap();
        state
            .apply("delete", &form(&[("id", &parent_id)]), path)
            .unwrap();
        let mut state = State::load(path).unwrap();
        assert_eq!(state.content_appeals[0].status, AppealStatus::Approved);
        assert_eq!(state.content_appeals[1].status, AppealStatus::Denied);
        assert_eq!(state.content_appeals[2].status, AppealStatus::Restored);
        assert_eq!(state.content_appeals[3].status, AppealStatus::Unavailable);
        state.delete_account_profile(REVIEWER);
        assert_eq!(
            state.content_appeals[0].reviewer.as_deref(),
            Some(super::super::DELETED_ACCOUNT)
        );
        state.delete_account_profile(AUTHOR);
        assert!(state.content_appeals.is_empty());
    }
}

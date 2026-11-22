use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{decode, encode, restriction_handle, value, ModPermission, ModeratorRole, State};

#[derive(Clone)]
pub struct ApprovedMember {
    pub community: String,
    pub handle: String,
}

impl State {
    pub fn is_approved_member(&self, slug: &str, handle: &str) -> bool {
        restriction_handle(handle).is_ok_and(|handle| {
            self.approved_members
                .iter()
                .any(|member| member.community == slug && member.handle == handle)
        })
    }

    pub(super) fn approve_member(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Members)?;
        let item = &self.communities[index];
        let handle = restriction_handle(value(form, "handle"))?;
        if handle == self.username && self.moderator_role(item, &handle).is_some() {
            return Err("Ask another moderator to approve your account.".into());
        }
        if item.owner.as_deref() == Some(handle.as_str())
            || self.moderator_role(item, &handle) == Some(ModeratorRole::Moderator)
        {
            return Err("This account already has access through its community role.".into());
        }
        if self.is_approved_member(&item.slug, &handle) {
            return Ok("This account is already approved.");
        }
        let community = item.slug.clone();
        let action = format!("Approved u/{handle} as a community member");
        self.approved_members.push(ApprovedMember {
            community: community.clone(),
            handle,
        });
        self.record_moderation_action(&community, &action)?;
        Ok("Member approved.")
    }

    pub(super) fn revoke_approved_member(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Members)?;
        let community = &self.communities[index].slug;
        let handle = restriction_handle(value(form, "handle"))?;
        let index = self
            .approved_members
            .iter()
            .position(|member| member.community == *community && member.handle == handle)
            .ok_or("This account is not an approved member here.")?;
        let member = self.approved_members.remove(index);
        let action = format!("Revoked approved-member access for u/{handle}");
        self.record_moderation_action(&member.community, &action)?;
        Ok("Member approval revoked.")
    }

    pub(super) fn read_approved_member(&mut self, slug: &str, handle: &str) -> Option<()> {
        let slug = decode(slug)?;
        let handle = decode(handle)?;
        if restriction_handle(&handle).ok()? != handle
            || self.is_approved_member(&slug, &handle)
            || !self
                .communities
                .iter()
                .any(|item| item.slug == slug && item.owner.is_some())
        {
            return None;
        }
        self.approved_members.push(ApprovedMember {
            community: slug,
            handle,
        });
        Some(())
    }

    pub(super) fn write_approved_members(&self, file: &mut impl Write) -> io::Result<()> {
        for member in &self.approved_members {
            writeln!(
                file,
                "approved-member\t{}\t{}",
                encode(&member.community),
                encode(&member.handle)
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Community;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|&(key, val)| (key.into(), val.into()))
            .collect()
    }

    fn community<'a>(state: &'a State, slug: &str) -> &'a Community {
        state
            .communities
            .iter()
            .find(|item| item.slug == slug)
            .unwrap()
    }

    fn member(slug: &str, handle: &str) -> BTreeMap<String, String> {
        form(&[("community", slug), ("handle", handle)])
    }

    fn room(state: &mut State, slug: &str, visibility: &str) {
        state
            .create_community(&form(&[
                ("slug", slug),
                ("name", slug),
                ("visibility", visibility),
            ]))
            .unwrap();
    }

    #[test]
    fn approvals_persist_and_revocation_respects_community_visibility() {
        let filename = format!(
            "commonline-approval-{}-{}.state",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        );
        struct TempState(PathBuf);
        impl Drop for TempState {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
        let fixture = TempState(std::env::temp_dir().join(filename));
        let mut state = State::seeded();
        state.activate_profile(Some("room_owner"));
        room(&mut state, "private-room", "private");
        room(&mut state, "restricted-room", "restricted");
        state
            .apply(
                "approve-member",
                &member("private-room", "U/Member_A"),
                &fixture.0,
            )
            .unwrap();
        state
            .apply(
                "approve-member",
                &member("restricted-room", "member_a"),
                &fixture.0,
            )
            .unwrap();
        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some("member_a"));
        for slug in ["private-room", "restricted-room"] {
            assert!(state.is_approved_member(slug, "member_a"));
            assert!(!state.joined.contains(slug));
            assert!(state.can_view_community(community(&state, slug)));
            assert!(state.can_contribute(community(&state, slug)));
            assert!(!state.can_open_mod_tools(community(&state, slug)));
        }
        let mut owned_ids = Vec::new();
        for slug in ["private-room", "restricted-room"] {
            state
                .create_post(&form(&[
                    ("community", slug),
                    ("title", "Approved contribution"),
                ]))
                .unwrap();
            owned_ids.push(state.posts.last().unwrap().id.to_string());
        }
        state
            .create_feed(&form(&[
                ("name", "Existing feed"),
                ("community-private-room", "on"),
            ]))
            .unwrap();
        state
            .save_draft(&form(&[
                ("community", "private-room"),
                ("title", "Keep my local draft"),
            ]))
            .unwrap();

        state.activate_profile(Some("room_owner"));
        for slug in ["private-room", "restricted-room"] {
            state
                .apply(
                    "revoke-approved-member",
                    &member(slug, "member_a"),
                    &fixture.0,
                )
                .unwrap();
        }
        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some("member_a"));
        assert!(!state.can_view_community(community(&state, "private-room")));
        assert!(state.can_view_community(community(&state, "restricted-room")));
        assert!(!state.can_contribute(community(&state, "restricted-room")));
        for id in &owned_ids {
            assert!(state
                .edit_post(&form(&[("id", id), ("title", "Bypass")]))
                .is_err());
        }
        assert!(state.delete_post(&form(&[("id", &owned_ids[0])])).is_err());
        state.delete_post(&form(&[("id", &owned_ids[1])])).unwrap();
        assert!(state
            .change("mute", &form(&[("community", "private-room")]))
            .is_err());
        assert!(state
            .save_draft(&form(&[("community", "private-room"), ("title", "Bypass")]))
            .is_err());
        assert_eq!(state.drafts[0].title, "Keep my local draft");
        let feed_id = state.custom_feeds[0].id.to_string();
        state
            .edit_feed(&form(&[("id", &feed_id), ("name", "Renamed feed")]))
            .unwrap();
        assert!(state.custom_feeds[0].communities.contains("private-room"));
        state
            .create_feed(&form(&[
                ("name", "New feed"),
                ("community-private-room", "on"),
            ]))
            .unwrap();
        assert!(!state.custom_feeds[1].communities.contains("private-room"));
    }

    #[test]
    fn member_management_respects_roles_scope_and_independent_restrictions() {
        let mut state = State::seeded();
        state.activate_profile(Some("room_owner"));
        room(&mut state, "private-room", "private");
        room(&mut state, "other-room", "private");
        let role = |handle, role| {
            form(&[
                ("community", "private-room"),
                ("handle", handle),
                ("role", role),
            ])
        };
        state
            .set_moderator_role(&role("delegate", "moderator"))
            .unwrap();
        state
            .set_moderator_role(&role("reviewer", "appeals-reviewer"))
            .unwrap();

        state.activate_profile(Some("delegate"));
        assert!(state
            .approve_member(&member("private-room", "delegate"))
            .is_err());
        assert!(state
            .approve_member(&member("private-room", "room_owner"))
            .is_err());
        assert!(state
            .approve_member(&member("other-room", "member_a"))
            .is_err());
        state
            .approve_member(&member("private-room", "reviewer"))
            .unwrap();
        state
            .approve_member(&member("private-room", "member_a"))
            .unwrap();
        for kind in ["ban", "mute"] {
            state
                .restrict_user(&form(&[
                    ("community", "private-room"),
                    ("username", "member_a"),
                    ("kind", kind),
                ]))
                .unwrap();
        }
        state.activate_profile(Some("reviewer"));
        assert!(state.can_contribute(community(&state, "private-room")));
        assert!(state
            .revoke_approved_member(&member("private-room", "member_a"))
            .is_err());
        state.activate_profile(Some("member_a"));
        assert!(state.can_view_community(community(&state, "private-room")));
        assert!(!state.can_contribute(community(&state, "private-room")));
        assert!(state.is_mod_mail_muted("private-room", "member_a"));
        assert!(!state.can_view_community(community(&state, "other-room")));

        state.activate_profile(Some("room_owner"));
        state
            .set_moderator_role(&role("reviewer", "moderator"))
            .unwrap();
        state
            .revoke_approved_member(&member("private-room", "reviewer"))
            .unwrap();
        state.activate_profile(Some("reviewer"));
        assert!(!state.is_approved_member("private-room", "reviewer"));
        assert!(state.can_contribute(community(&state, "private-room")));
    }
}

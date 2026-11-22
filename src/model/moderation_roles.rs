use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{decode, encode, restriction_handle, value, Community, State};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModPermission {
    Reports,
    Mail,
    Restrictions,
    Schedule,
    Appeals,
    Members,
    Content,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeratorRole {
    Moderator,
    AppealsReviewer,
}

impl ModeratorRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Moderator => "moderator",
            Self::AppealsReviewer => "appeals-reviewer",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Moderator => "Moderator",
            Self::AppealsReviewer => "Appeals reviewer",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "moderator" => Some(Self::Moderator),
            "appeals-reviewer" => Some(Self::AppealsReviewer),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct ModeratorAssignment {
    pub community: String,
    pub handle: String,
    pub role: ModeratorRole,
}

impl State {
    pub fn moderator_role(&self, item: &Community, handle: &str) -> Option<ModeratorRole> {
        self.moderator_roles
            .iter()
            .find(|assignment| assignment.community == item.slug && assignment.handle == handle)
            .map(|assignment| assignment.role)
    }

    pub fn can_moderate(&self, item: &Community, permission: ModPermission) -> bool {
        if self.username.is_empty() || item.owner.is_none() {
            return false;
        }
        if item.owner.as_deref() == Some(self.username.as_str()) {
            return true;
        }
        match self.moderator_role(item, &self.username) {
            Some(ModeratorRole::Moderator) => true,
            Some(ModeratorRole::AppealsReviewer) => permission == ModPermission::Appeals,
            None => false,
        }
    }

    pub fn can_open_mod_tools(&self, item: &Community) -> bool {
        self.can_moderate(item, ModPermission::Appeals)
    }

    pub(super) fn moderated_community_index(
        &self,
        form: &BTreeMap<String, String>,
        permission: ModPermission,
    ) -> Result<usize, String> {
        let index = self.community_index(form)?;
        if self.can_moderate(&self.communities[index], permission) {
            Ok(index)
        } else {
            Err("Your role cannot use this community’s moderation tool.".into())
        }
    }

    pub(super) fn protect_community_staff(
        &self,
        community: &Community,
        handle: &str,
    ) -> Result<(), String> {
        if community.owner.as_deref() == Some(handle) {
            return Err("The community owner cannot be restricted.".into());
        }
        if self.moderator_role(community, handle).is_some() {
            return Err("The owner must remove this account’s moderator role first.".into());
        }
        Ok(())
    }

    pub(super) fn set_moderator_role(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.owned_community_index(form)?;
        let community = self.communities[index].slug.clone();
        let handle = restriction_handle(value(form, "handle"))?;
        if handle == self.username {
            return Err("The owner already has full access to this community.".into());
        }
        if self.is_banned(&community, &handle) || self.is_mod_mail_muted(&community, &handle) {
            return Err(
                "Lift this account’s restrictions before assigning a moderator role.".into(),
            );
        }
        let role = ModeratorRole::parse(value(form, "role")).ok_or("Choose a moderator role.")?;
        if let Some(assignment) = self
            .moderator_roles
            .iter_mut()
            .find(|assignment| assignment.community == community && assignment.handle == handle)
        {
            if assignment.role == role {
                return Ok("This account already has that role.");
            }
            assignment.role = role;
        } else {
            self.moderator_roles.push(ModeratorAssignment {
                community: community.clone(),
                handle: handle.clone(),
                role,
            });
        }
        let action = format!("Assigned {} to u/{handle}", role.label());
        self.record_moderation_action(&community, &action)?;
        Ok("Moderator role saved.")
    }

    pub(super) fn remove_moderator_role(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.owned_community_index(form)?;
        let community = self.communities[index].slug.clone();
        let handle = restriction_handle(value(form, "handle"))?;
        let index = self
            .moderator_roles
            .iter()
            .position(|assignment| assignment.community == community && assignment.handle == handle)
            .ok_or("This account has no delegated role here.")?;
        self.moderator_roles.remove(index);
        let action = format!("Removed moderator role from u/{handle}");
        self.record_moderation_action(&community, &action)?;
        Ok("Moderator role removed.")
    }

    pub(super) fn read_moderator_role(
        &mut self,
        slug: &str,
        handle: &str,
        role: &str,
    ) -> Option<()> {
        let slug = decode(slug)?;
        let handle = decode(handle)?;
        let role = ModeratorRole::parse(role)?;
        let community = self.communities.iter().find(|item| item.slug == slug)?;
        let owner = community.owner.as_deref()?;
        if handle == owner
            || restriction_handle(&handle).ok()? != handle
            || self.moderator_role(community, &handle).is_some()
        {
            return None;
        }
        self.moderator_roles.push(ModeratorAssignment {
            community: slug,
            handle,
            role,
        });
        Some(())
    }

    pub(super) fn write_moderator_roles(&self, file: &mut impl Write) -> io::Result<()> {
        for assignment in &self.moderator_roles {
            writeln!(
                file,
                "moderator-role\t{}\t{}\t{}",
                encode(&assignment.community),
                encode(&assignment.handle),
                assignment.role.as_str()
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|&(key, val)| (key.into(), val.into()))
            .collect()
    }

    #[test]
    fn delegated_access_stays_in_the_community_and_changes_with_the_role() {
        let mut state = State::seeded();
        state.activate_profile(Some("owner_test"));
        for slug in ["backstage", "second-room"] {
            state
                .create_community(&form(&[
                    ("slug", slug),
                    ("name", "Test community"),
                    ("description", "Private community for permission checks."),
                    ("visibility", "private"),
                ]))
                .unwrap();
        }
        let assign = |role| {
            form(&[
                ("community", "backstage"),
                ("handle", "mod_test"),
                ("role", role),
            ])
        };
        state.set_moderator_role(&assign("moderator")).unwrap();
        let backstage = state
            .communities
            .iter()
            .find(|item| item.slug == "backstage")
            .unwrap()
            .clone();
        let other = state
            .communities
            .iter()
            .find(|item| item.slug == "second-room")
            .unwrap()
            .clone();

        state.activate_profile(Some("mod_test"));
        assert!(state.can_view_community(&backstage));
        assert!(state.can_contribute(&backstage));
        assert!(state.can_moderate(&backstage, ModPermission::Restrictions));
        assert!(!state.can_open_mod_tools(&other));
        assert!(state
            .set_moderator_role(&assign("appeals-reviewer"))
            .is_err());
        assert!(state
            .restrict_user(&form(&[
                ("community", "backstage"),
                ("username", "owner_test"),
                ("kind", "ban"),
            ]))
            .is_err());

        state.activate_profile(Some("owner_test"));
        state
            .set_moderator_role(&assign("appeals-reviewer"))
            .unwrap();
        state.activate_profile(Some("mod_test"));
        assert!(state.can_open_mod_tools(&backstage));
        assert!(state.can_moderate(&backstage, ModPermission::Appeals));
        assert!(!state.can_moderate(&backstage, ModPermission::Reports));
        assert!(!state.can_view_community(&backstage));

        state.activate_profile(Some("owner_test"));
        state
            .remove_moderator_role(&assign("appeals-reviewer"))
            .unwrap();
        state.activate_profile(Some("mod_test"));
        assert!(!state.can_open_mod_tools(&backstage));
    }

    #[test]
    fn scheduled_edits_are_attributed_to_the_moderator_who_made_them() {
        let mut state = State::seeded();
        state.activate_profile(Some("owner_test"));
        state
            .create_community(&form(&[
                ("slug", "backstage"),
                ("name", "Backstage"),
                ("description", "Test schedule ownership."),
            ]))
            .unwrap();
        state
            .set_moderator_role(&form(&[
                ("community", "backstage"),
                ("handle", "mod_test"),
                ("role", "moderator"),
            ]))
            .unwrap();
        let publish_at = (super::super::unix_seconds() + 3_600).to_string();
        let mut scheduled = form(&[
            ("community", "backstage"),
            ("kind", "post"),
            ("title", "Entered by owner"),
            ("publish_at", &publish_at),
        ]);
        state.schedule_item(&scheduled).unwrap();
        scheduled.insert("id".into(), state.scheduled[0].id.to_string());
        scheduled.insert("title".into(), "Edited by moderator".into());

        state.activate_profile(Some("mod_test"));
        state.edit_scheduled_item(&scheduled).unwrap();
        let published_at = state.scheduled[0].publish_at + 30;
        state.publish_scheduled_item(0, published_at).unwrap();
        let post = state.posts.last().unwrap();
        assert_eq!(post.title, "Edited by moderator");
        assert_eq!(post.author, "mod_test");
        assert_eq!(post.created_at, Some(published_at));
    }
}

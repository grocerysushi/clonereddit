use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    decode, encode, id_field, next_id, restriction_handle, value, Community, State, DELETED_ACCOUNT,
};

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

impl ModPermission {
    pub const ALL: [Self; 7] = [
        Self::Reports,
        Self::Content,
        Self::Mail,
        Self::Members,
        Self::Restrictions,
        Self::Appeals,
        Self::Schedule,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Reports => "reports",
            Self::Mail => "mail",
            Self::Restrictions => "restrictions",
            Self::Schedule => "schedule",
            Self::Appeals => "appeals",
            Self::Members => "members",
            Self::Content => "content",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Reports => "Review reports and activity",
            Self::Mail => "Manage mod mail",
            Self::Restrictions => "Manage bans and mutes",
            Self::Schedule => "Schedule posts and events",
            Self::Appeals => "Review appeals",
            Self::Members => "Manage approved members",
            Self::Content => "Moderate posts, comments, and wiki pages",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeratorPermissions(u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeratorRole {
    Moderator,
    AppealsReviewer,
    Custom(ModeratorPermissions),
}

impl ModeratorRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Moderator => "moderator",
            Self::AppealsReviewer => "appeals-reviewer",
            Self::Custom(_) => "custom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Moderator => "Moderator",
            Self::AppealsReviewer => "Appeals reviewer",
            Self::Custom(_) => "Custom permissions",
        }
    }

    pub fn allows(self, permission: ModPermission) -> bool {
        match self {
            Self::Moderator => true,
            Self::AppealsReviewer => permission == ModPermission::Appeals,
            Self::Custom(permissions) => permissions.0 & permission.bit() != 0,
        }
    }

    pub fn permissions(self) -> impl Iterator<Item = ModPermission> {
        ModPermission::ALL
            .into_iter()
            .filter(move |&permission| self.allows(permission))
    }

    fn from_mask(mask: u8) -> Option<Self> {
        let all = ModPermission::ALL
            .into_iter()
            .fold(0, |bits, permission| bits | permission.bit());
        match mask {
            0 => None,
            bits if bits == all => Some(Self::Moderator),
            bits if bits == ModPermission::Appeals.bit() => Some(Self::AppealsReviewer),
            bits => Some(Self::Custom(ModeratorPermissions(bits))),
        }
    }

    fn from_form(form: &BTreeMap<String, String>) -> Result<Self, String> {
        let mut bits = 0;
        for permission in ModPermission::ALL {
            match form
                .get(&format!("permission-{}", permission.code()))
                .map(String::as_str)
            {
                Some("on") => bits |= permission.bit(),
                None => {}
                _ => return Err("Choose valid moderator permissions.".into()),
            }
        }
        Self::from_mask(bits).ok_or_else(|| "Choose at least one moderator permission.".into())
    }

    fn from_record(role: &str, scopes: &[&str]) -> Option<Self> {
        match (role, scopes) {
            ("moderator", []) => Some(Self::Moderator),
            ("appeals-reviewer", []) => Some(Self::AppealsReviewer),
            ("custom", [scopes]) => {
                let mut bits = 0;
                for code in scopes.split(',') {
                    let permission = ModPermission::ALL
                        .into_iter()
                        .find(|scope| scope.code() == code)?;
                    if bits & permission.bit() != 0 {
                        return None;
                    }
                    bits |= permission.bit();
                }
                Self::from_mask(bits)
            }
            _ => None,
        }
    }

    fn write(self, file: &mut impl Write) -> io::Result<()> {
        write!(file, "{}", self.as_str())?;
        if matches!(self, Self::Custom(_)) {
            let scopes = self
                .permissions()
                .map(ModPermission::code)
                .collect::<Vec<_>>()
                .join(",");
            write!(file, "\t{scopes}")?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ModeratorAssignment {
    pub community: String,
    pub handle: String,
    pub role: ModeratorRole,
}

#[derive(Clone)]
pub struct ModeratorInvitation {
    pub id: u64,
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
        self.can_moderate_as(item, &self.username, permission)
    }

    pub fn can_moderate_as(
        &self,
        item: &Community,
        handle: &str,
        permission: ModPermission,
    ) -> bool {
        if handle.is_empty() || handle == DELETED_ACCOUNT || item.owner.is_none() {
            return false;
        }
        item.owner.as_deref() == Some(handle)
            || self
                .moderator_role(item, handle)
                .is_some_and(|role| role.allows(permission))
    }

    pub fn can_open_mod_tools(&self, item: &Community) -> bool {
        !self.username.is_empty()
            && (item.owner.as_deref() == Some(self.username.as_str())
                || self.moderator_role(item, &self.username).is_some())
    }

    pub fn incoming_moderator_invitations(&self) -> impl Iterator<Item = &ModeratorInvitation> {
        self.moderator_invitations
            .iter()
            .filter(|invite| invite.handle == self.username)
    }

    pub fn outgoing_moderator_invitations<'a>(
        &'a self,
        item: &'a Community,
    ) -> impl Iterator<Item = &'a ModeratorInvitation> {
        self.moderator_invitations.iter().filter(move |invite| {
            item.owner.as_deref() == Some(self.username.as_str()) && invite.community == item.slug
        })
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
        let role = ModeratorRole::from_record(value(form, "role"), &[])
            .ok_or("Choose a moderator role.")?;
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
        self.moderator_invitations
            .retain(|invite| invite.community != community || invite.handle != handle);
        let action = format!("Assigned {} to u/{handle}", role.label());
        self.record_moderation_action(&community, &action)?;
        Ok("Moderator role saved.")
    }

    pub(super) fn invite_moderator(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.owned_community_index(form)?;
        let item = &self.communities[index];
        let handle = restriction_handle(value(form, "handle"))?;
        self.validate_invitation_target(item, &handle)?;
        if self
            .moderator_invitations
            .iter()
            .any(|invite| invite.community == item.slug && invite.handle == handle)
        {
            return Err("This account already has a pending invitation here.".into());
        }
        let role = ModeratorRole::from_form(form)?;
        let community = item.slug.clone();
        let id = next_id(&mut self.next_moderator_invitation)?;
        self.moderator_invitations.push(ModeratorInvitation {
            id,
            community: community.clone(),
            handle: handle.clone(),
            role,
        });
        self.record_moderation_action(&community, &format!("Invited u/{handle} to moderate"))?;
        Ok("Moderator invitation sent.")
    }

    fn validate_invitation_target(&self, item: &Community, handle: &str) -> Result<(), String> {
        if item.owner.as_deref() == Some(handle) || self.moderator_role(item, handle).is_some() {
            return Err("This account already moderates this community.".into());
        }
        if self.is_banned(&item.slug, handle) || self.is_mod_mail_muted(&item.slug, handle) {
            return Err(
                "Lift this account’s restrictions before inviting them to moderate.".into(),
            );
        }
        Ok(())
    }

    pub(super) fn cancel_moderator_invitation(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let community_index = self.owned_community_index(form)?;
        let community = &self.communities[community_index].slug;
        let id = id_field(form, "id")?;
        let index = self
            .moderator_invitations
            .iter()
            .position(|invite| invite.id == id && invite.community == *community)
            .ok_or("This moderator invitation is unavailable.")?;
        let invite = self.moderator_invitations.remove(index);
        self.record_moderation_action(
            &invite.community,
            &format!("Canceled the moderator invitation to u/{}", invite.handle),
        )?;
        Ok("Moderator invitation canceled.")
    }

    pub(super) fn respond_to_moderator_invitation(
        &mut self,
        form: &BTreeMap<String, String>,
        accept: bool,
    ) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .moderator_invitations
            .iter()
            .position(|invite| invite.id == id && invite.handle == self.username)
            .ok_or("This moderator invitation is unavailable.")?;
        let invite = &self.moderator_invitations[index];
        if accept {
            let community = self
                .communities
                .iter()
                .find(|item| item.slug == invite.community)
                .filter(|item| {
                    item.owner
                        .as_deref()
                        .is_some_and(|owner| owner != DELETED_ACCOUNT)
                })
                .ok_or("This moderator invitation is unavailable.")?;
            self.validate_invitation_target(community, &invite.handle)?;
        }
        let invite = self.moderator_invitations.remove(index);
        let (verb, notice) = if accept {
            self.moderator_roles.push(ModeratorAssignment {
                community: invite.community.clone(),
                handle: invite.handle.clone(),
                role: invite.role,
            });
            ("Accepted", "Moderator invitation accepted.")
        } else {
            ("Declined", "Moderator invitation declined.")
        };
        self.record_moderation_action(
            &invite.community,
            &format!("{verb} a moderator invitation for u/{}", invite.handle),
        )?;
        Ok(notice)
    }

    pub(super) fn set_moderator_permissions(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.owned_community_index(form)?;
        let community = self.communities[index].slug.clone();
        let handle = restriction_handle(value(form, "handle"))?;
        let role = ModeratorRole::from_form(form)?;
        let assignment = self
            .moderator_roles
            .iter_mut()
            .find(|assignment| assignment.community == community && assignment.handle == handle)
            .ok_or("This account has no delegated role here.")?;
        if assignment.role != role {
            assignment.role = role;
            self.record_moderation_action(
                &community,
                &format!("Changed moderator permissions for u/{handle}"),
            )?;
        }
        Ok("Moderator permissions saved.")
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
        scopes: &[&str],
    ) -> Option<()> {
        let slug = decode(slug)?;
        let handle = decode(handle)?;
        let role = ModeratorRole::from_record(role, scopes)?;
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

    pub(super) fn read_moderator_invitation(
        &mut self,
        id: u64,
        slug: &str,
        handle: &str,
        role: &str,
        scopes: &[&str],
    ) -> Option<()> {
        let community = decode(slug)?;
        let handle = decode(handle)?;
        let role = ModeratorRole::from_record(role, scopes)?;
        let item = self
            .communities
            .iter()
            .find(|item| item.slug == community)?;
        let owner = item.owner.as_deref()?;
        if owner == DELETED_ACCOUNT
            || owner == handle
            || restriction_handle(&handle).ok()? != handle
            || self.moderator_role(item, &handle).is_some()
            || id == 0
            || self.moderator_invitations.iter().any(|invite| {
                invite.id == id || invite.community == community && invite.handle == handle
            })
        {
            return None;
        }
        self.next_moderator_invitation = self.next_moderator_invitation.max(id.checked_add(1)?);
        self.moderator_invitations.push(ModeratorInvitation {
            id,
            community,
            handle,
            role,
        });
        Some(())
    }

    pub(super) fn write_moderator_roles(&self, file: &mut impl Write) -> io::Result<()> {
        for assignment in &self.moderator_roles {
            write!(
                file,
                "moderator-role\t{}\t{}\t",
                encode(&assignment.community),
                encode(&assignment.handle)
            )?;
            assignment.role.write(file)?;
            writeln!(file)?;
        }
        if self.next_moderator_invitation > 1 {
            writeln!(
                file,
                "moderator-invitation-sequence\t{}",
                self.next_moderator_invitation
            )?;
        }
        for invite in &self.moderator_invitations {
            write!(
                file,
                "moderator-invitation\t{}\t{}\t{}\t",
                invite.id,
                encode(&invite.community),
                encode(&invite.handle),
            )?;
            invite.role.write(file)?;
            writeln!(file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|&(key, val)| (key.into(), val.into()))
            .collect()
    }

    struct StateFile(PathBuf);

    impl Drop for StateFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn invitations_grant_only_selected_permissions_after_the_recipient_accepts() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let state_file = StateFile(std::env::temp_dir().join(format!(
            "commonline-moderator-invitations-{}-{unique}.state",
            std::process::id()
        )));
        let mut state = State::seeded();
        state.activate_profile(Some("owner_roles"));
        for slug in ["role-workroom", "other-role-room"] {
            state
                .create_community(&form(&[
                    ("slug", slug),
                    ("name", slug),
                    ("visibility", "private"),
                ]))
                .unwrap();
        }
        let item = state
            .communities
            .iter()
            .find(|item| item.slug == "role-workroom")
            .unwrap()
            .clone();
        let other = state
            .communities
            .iter()
            .find(|item| item.slug == "other-role-room")
            .unwrap()
            .clone();
        let invite = form(&[
            ("community", &item.slug),
            ("handle", "invited_mod"),
            ("permission-mail", "on"),
            ("permission-reports", "on"),
        ]);
        assert!(state
            .invite_moderator(&form(&[
                ("community", &item.slug),
                ("handle", "invited_mod")
            ]))
            .is_err());
        state
            .apply("invite-moderator", &invite, &state_file.0)
            .unwrap();
        let id = state
            .outgoing_moderator_invitations(&item)
            .next()
            .unwrap()
            .id
            .to_string();
        let response = form(&[("id", &id)]);
        let wrong_cancel = form(&[("id", &id), ("community", &other.slug)]);
        assert!(state
            .apply("cancel-moderator-invitation", &wrong_cancel, &state_file.0)
            .is_err());
        assert!(state
            .apply("invite-moderator", &invite, &state_file.0)
            .is_err());

        let mut state = State::load(&state_file.0).unwrap();
        state.activate_profile(Some("unrelated_reader"));
        assert_eq!(state.incoming_moderator_invitations().count(), 0);
        assert!(state
            .apply("accept-moderator-invitation", &response, &state_file.0)
            .is_err());
        state.activate_profile(Some("invited_mod"));
        assert_eq!(state.incoming_moderator_invitations().count(), 1);
        assert!(!state.can_open_mod_tools(&item));
        state
            .apply("accept-moderator-invitation", &response, &state_file.0)
            .unwrap();
        assert!(state.can_open_mod_tools(&item));
        assert!(state.can_view_community(&item));
        assert!(!state.can_contribute(&item));
        assert!(state.can_moderate(&item, ModPermission::Mail));
        assert!(state.can_moderate(&item, ModPermission::Reports));
        assert!(!state.can_moderate(&item, ModPermission::Appeals));
        assert!(!state.can_open_mod_tools(&other));
        assert!(state
            .apply("accept-moderator-invitation", &response, &state_file.0)
            .is_err());

        let mut state = State::load(&state_file.0).unwrap();
        state.activate_profile(Some("invited_mod"));
        assert!(state.can_moderate(&item, ModPermission::Reports));
        assert!(!state.can_moderate(&item, ModPermission::Content));
        assert!(state
            .apply("set-moderator-permissions", &invite, &state_file.0)
            .is_err());
        state.activate_profile(Some("owner_roles"));
        let mail_only = form(&[
            ("community", &item.slug),
            ("handle", "invited_mod"),
            ("permission-mail", "on"),
        ]);
        state
            .apply("set-moderator-permissions", &mail_only, &state_file.0)
            .unwrap();
        state.activate_profile(Some("invited_mod"));
        assert!(state.can_open_mod_tools(&item));
        assert!(!state.can_view_community(&item));
        assert!(!state.can_moderate(&item, ModPermission::Reports));
        state.activate_profile(Some("owner_roles"));
        state
            .apply("remove-moderator-role", &invite, &state_file.0)
            .unwrap();
        state
            .apply("invite-moderator", &invite, &state_file.0)
            .unwrap();
        let new_id = state
            .outgoing_moderator_invitations(&item)
            .next()
            .unwrap()
            .id
            .to_string();
        assert_ne!(id, new_id);
        state.activate_profile(Some("invited_mod"));
        assert!(!state.can_open_mod_tools(&item));
        assert!(state
            .apply("accept-moderator-invitation", &response, &state_file.0)
            .is_err());
        state
            .apply(
                "decline-moderator-invitation",
                &form(&[("id", &new_id)]),
                &state_file.0,
            )
            .unwrap();
        assert_eq!(state.incoming_moderator_invitations().count(), 0);
        assert!(!state.can_open_mod_tools(&item));
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

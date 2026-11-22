use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    canonical_u64, decode, encode, next_id, number, optional_number, required, unix_seconds, value,
    CommunityRestriction, ModPermission, State,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppealStatus {
    Pending,
    Approved,
    Denied,
    Lifted,
    Expired,
    Restored,
    Unavailable,
}

impl AppealStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending review",
            Self::Approved => "Approved",
            Self::Denied => "Denied",
            Self::Lifted => "Restriction lifted",
            Self::Expired => "Restriction expired",
            Self::Restored => "Content restored",
            Self::Unavailable => "Content unavailable",
        }
    }

    pub(super) fn as_record(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Lifted => "lifted",
            Self::Expired => "expired",
            Self::Restored => "restored",
            Self::Unavailable => "unavailable",
        }
    }

    pub(super) fn from_record(status: &str) -> Option<Self> {
        match status {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "denied" => Some(Self::Denied),
            "lifted" => Some(Self::Lifted),
            "expired" => Some(Self::Expired),
            "restored" => Some(Self::Restored),
            "unavailable" => Some(Self::Unavailable),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Appeal {
    pub id: u64,
    pub community: String,
    pub restriction_id: u64,
    pub appellant: String,
    pub kind: String,
    pub body: String,
    pub status: AppealStatus,
    pub response: String,
    pub reviewer: Option<String>,
    pub created_at: u64,
    pub decided_at: Option<u64>,
}

impl State {
    pub fn can_submit_appeal(&self, restriction: &CommunityRestriction) -> bool {
        self.can_submit_appeal_at(restriction, unix_seconds())
    }

    fn can_submit_appeal_at(&self, restriction: &CommunityRestriction, now: u64) -> bool {
        !self.username.is_empty()
            && restriction.username == self.username
            && restriction.is_active_at(now)
            && !self
                .appeals
                .iter()
                .any(|appeal| appeal.restriction_id == restriction.id)
    }

    pub(super) fn submit_appeal(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let id =
            canonical_u64(value(form, "restriction")).ok_or("Choose an active restriction.")?;
        let now = unix_seconds();
        let restriction = self
            .restrictions
            .iter()
            .find(|item| item.id == id && item.username == self.username && item.is_active_at(now))
            .ok_or("Choose one of your active restrictions.")?;
        if !self.can_submit_appeal_at(restriction, now) {
            return Err("You have already appealed this restriction.".into());
        }
        let body = required(form, "body", "Appeal", 4_000)?;
        self.appeals.push(Appeal {
            id: next_id(&mut self.next_appeal)?,
            community: restriction.community.clone(),
            restriction_id: restriction.id,
            appellant: self.username.clone(),
            kind: restriction.kind.clone(),
            body: body.into(),
            status: AppealStatus::Pending,
            response: String::new(),
            reviewer: None,
            created_at: unix_seconds(),
            decided_at: None,
        });
        Ok("Appeal submitted.")
    }

    pub(super) fn decide_appeal(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let community_index = self.moderated_community_index(form, ModPermission::Appeals)?;
        let community = &self.communities[community_index].slug;
        let id = canonical_u64(value(form, "appeal")).ok_or("Choose a valid appeal.")?;
        let appeal = self
            .appeals
            .iter_mut()
            .find(|item| item.id == id && &item.community == community)
            .ok_or("This appeal is unavailable in this community.")?;
        if appeal.status != AppealStatus::Pending {
            return Err("This appeal has already been resolved.".into());
        }
        let (status, notice) = match value(form, "decision") {
            "approve" => (AppealStatus::Approved, "Appeal approved."),
            "deny" => (AppealStatus::Denied, "Appeal denied."),
            _ => return Err("Choose whether to approve or deny the appeal.".into()),
        };
        let response = required(form, "response", "Decision explanation", 4_000)?;
        let now = unix_seconds();
        let restriction = self
            .restrictions
            .iter()
            .position(|item| {
                item.id == appeal.restriction_id
                    && &item.community == community
                    && item.username == appeal.appellant
                    && item.kind == appeal.kind
                    && item.is_active_at(now)
            })
            .ok_or("The restriction attached to this appeal is no longer active.")?;
        if status == AppealStatus::Approved {
            self.restrictions.remove(restriction);
        }
        appeal.status = status;
        appeal.response = response.into();
        appeal.reviewer = Some(self.username.clone());
        appeal.decided_at = Some(now);

        let community = community.clone();
        let appellant = appeal.appellant.clone();
        let decision = status.label().to_ascii_lowercase();
        self.push_appeal_notification(
            &appellant,
            format!("Your appeal for c/{community} was {decision}."),
        )?;
        let audit = format!("Appeal #{id} {decision} by u/{}", self.username);
        self.record_moderation_action(&community, &audit)?;
        Ok(notice)
    }

    pub(super) fn notify_restriction_issued(
        &mut self,
        username: &str,
        community: &str,
        kind: &str,
    ) -> Result<(), String> {
        let restriction = if kind == "ban" {
            "ban"
        } else {
            "mod mail mute"
        };
        self.push_appeal_notification(
            username,
            format!("You received a {restriction} in c/{community}. You can submit an appeal."),
        )
    }

    pub(super) fn resolve_manually_lifted_appeal(
        &mut self,
        restriction: &CommunityRestriction,
    ) -> Result<(), String> {
        let Some(appeal) = self.appeals.iter_mut().find(|item| {
            item.restriction_id == restriction.id && item.status == AppealStatus::Pending
        }) else {
            return Ok(());
        };
        appeal.status = AppealStatus::Lifted;
        appeal.response = "The moderation team lifted this restriction.".into();
        appeal.reviewer = Some(self.username.clone());
        appeal.decided_at = Some(unix_seconds());
        self.push_appeal_notification(
            &restriction.username,
            format!(
                "Your restriction in c/{} was lifted.",
                restriction.community
            ),
        )
    }

    pub(super) fn resolve_expired_appeal(&mut self, restriction: &CommunityRestriction, now: u64) {
        if let Some(appeal) = self.appeals.iter_mut().find(|item| {
            item.restriction_id == restriction.id && item.status == AppealStatus::Pending
        }) {
            appeal.status = AppealStatus::Expired;
            appeal.response = "This restriction expired automatically.".into();
            appeal.reviewer = None;
            appeal.decided_at = Some(now);
        }
    }

    pub(super) fn read_appeal_record(&mut self, fields: &[&str]) -> Option<()> {
        match fields {
            ["appeal-sequence", next] => self.next_appeal = number(next)?,
            ["appeal", id, community, restriction, appellant, kind, body, status, response, reviewer, created, decided]
                if matches!(*kind, "ban" | "mute") =>
            {
                let reviewer = decode(reviewer)?;
                self.appeals.push(Appeal {
                    id: number(id)?,
                    community: decode(community)?,
                    restriction_id: number(restriction)?,
                    appellant: decode(appellant)?,
                    kind: (*kind).into(),
                    body: decode(body)?,
                    status: AppealStatus::from_record(status)?,
                    response: decode(response)?,
                    reviewer: (!reviewer.is_empty()).then_some(reviewer),
                    created_at: number(created)?,
                    decided_at: optional_number(decided)?,
                });
            }
            _ => return None,
        }
        Some(())
    }

    pub(super) fn write_appeal_records(&self, file: &mut impl Write) -> io::Result<()> {
        writeln!(file, "appeal-sequence\t{}", self.next_appeal)?;
        for appeal in &self.appeals {
            let decided = appeal
                .decided_at
                .map_or_else(String::new, |at| at.to_string());
            writeln!(
                file,
                "appeal\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{decided}",
                appeal.id,
                encode(&appeal.community),
                appeal.restriction_id,
                encode(&appeal.appellant),
                appeal.kind,
                encode(&appeal.body),
                appeal.status.as_record(),
                encode(&appeal.response),
                encode(appeal.reviewer.as_deref().unwrap_or_default()),
                appeal.created_at,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AppealStatus, State};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn a_private_community_appeal_lifts_only_its_own_restriction() {
        let filename = format!(
            "commonline-appeal-{}-{}.state",
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
        let path = &fixture.0;
        let form = |fields: &[(&str, &str)]| -> BTreeMap<String, String> {
            fields
                .iter()
                .map(|&(key, value)| (key.into(), value.into()))
                .collect()
        };
        state
            .apply(
                "create-community",
                &form(&[
                    ("slug", "private-test"),
                    ("name", "Private test"),
                    ("visibility", "private"),
                ]),
                path,
            )
            .unwrap();
        state
            .apply(
                "set-moderator-role",
                &form(&[
                    ("community", "private-test"),
                    ("handle", "reviewer"),
                    ("role", "appeals-reviewer"),
                ]),
                path,
            )
            .unwrap();
        for kind in ["ban", "mute"] {
            state
                .apply(
                    "restrict-user",
                    &form(&[
                        ("community", "private-test"),
                        ("username", "appellant"),
                        ("kind", kind),
                        ("reason", "Internal moderator note"),
                    ]),
                    path,
                )
                .unwrap();
        }
        let restriction = state.restrictions[0].id.to_string();
        let submit = form(&[
            ("restriction", &restriction),
            ("body", "Please reconsider."),
        ]);
        state.activate_profile(Some("bystander"));
        assert!(state.apply("submit-appeal", &submit, path).is_err());
        state.activate_profile(Some("appellant"));
        assert!(!state.can_view_community(state.communities.last().unwrap()));
        state.apply("submit-appeal", &submit, path).unwrap();
        assert!(state.apply("submit-appeal", &submit, path).is_err());

        state = State::load(path).unwrap();
        let appeal = state.appeals[0].id.to_string();
        let decision = form(&[
            ("community", "private-test"),
            ("appeal", &appeal),
            ("decision", "approve"),
            ("response", "Restriction lifted on review."),
        ]);
        assert!(state.apply("decide-appeal", &decision, path).is_err());
        state.activate_profile(Some("reviewer"));
        assert!(!state.can_view_community(state.communities.last().unwrap()));
        state.apply("decide-appeal", &decision, path).unwrap();
        assert!(state.apply("decide-appeal", &decision, path).is_err());

        let state = State::load(path).unwrap();
        assert_eq!(state.appeals[0].status, AppealStatus::Approved);
        assert_eq!(state.appeals[0].reviewer.as_deref(), Some("reviewer"));
        assert!(!state.is_banned("private-test", "appellant"));
        assert!(state.is_mod_mail_muted("private-test", "appellant"));
        assert!(state
            .notifications
            .iter()
            .any(|item| item.recipient == "appellant"
                && item.appeal
                && item.body.contains("approved")));
        assert!(!state
            .notifications
            .iter()
            .any(|item| item.body.contains("Internal moderator note")));
    }
}

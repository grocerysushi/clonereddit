use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use super::{value, CommunityRestriction, State};
use crate::schedule_time;

impl CommunityRestriction {
    pub fn is_active_at(&self, now: u64) -> bool {
        self.expires_at.is_none_or(|expires_at| now < expires_at)
    }
}

pub(super) fn restriction_expiration(
    form: &BTreeMap<String, String>,
    now: u64,
) -> Result<Option<u64>, String> {
    let duration = value(form, "duration");
    if duration.is_empty() {
        return Ok(None);
    }
    let duration = duration
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0 && duration.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or("Enter a positive whole number for the duration.")?;
    let seconds_per_unit = match value(form, "duration_unit") {
        "minutes" => 60,
        "hours" => 3_600,
        "days" => 86_400,
        _ => return Err("Choose minutes, hours, or days.".into()),
    };
    let expires_at = duration
        .checked_mul(seconds_per_unit)
        .and_then(|seconds| now.checked_add(seconds))
        .filter(|timestamp| representable_expiration(*timestamp))
        .ok_or("That duration exceeds the supported date range.")?;
    Ok(Some(expires_at))
}

pub(super) fn representable_expiration(timestamp: u64) -> bool {
    !schedule_time::to_local(timestamp, "UTC").is_empty()
}

impl State {
    pub fn expire_restrictions(&mut self, path: &Path, now: u64) -> io::Result<usize> {
        if self.restrictions.iter().all(|item| item.is_active_at(now)) {
            return Ok(0);
        }

        let mut updated = self.clone();
        let mut expired = Vec::new();
        updated.restrictions.retain(|item| {
            if item.is_active_at(now) {
                true
            } else {
                expired.push(item.clone());
                false
            }
        });
        for item in &expired {
            updated
                .record_expired_restriction(item, now)
                .map_err(io::Error::other)?;
        }
        updated.save(path)?;
        *self = updated;
        Ok(expired.len())
    }

    pub(super) fn record_expired_restriction(
        &mut self,
        restriction: &CommunityRestriction,
        now: u64,
    ) -> Result<(), String> {
        self.resolve_expired_appeal(restriction, now);
        let kind = if restriction.kind == "ban" {
            "community ban"
        } else {
            "mod mail mute"
        };
        self.push_appeal_notification(
            &restriction.username,
            format!(
                "Your {kind} in c/{} expired automatically.",
                restriction.community
            ),
        )?;
        self.record_moderation_action_at(
            &restriction.community,
            &format!("Automatic expiry of {kind} for u/{}", restriction.username),
            now,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::model::{unix_seconds, AppealStatus, NotificationPreferences};

    const ROOM: &str = "timed-room";
    const OWNER: &str = "timer_owner";
    const MEMBER: &str = "timer_member";

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "commonline-restriction-{}-{}.state",
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

    fn room() -> State {
        let mut state = State::seeded();
        state.activate_profile(Some(OWNER));
        state
            .create_community(&form(&[
                ("slug", ROOM),
                ("name", "Timed restrictions room"),
            ]))
            .unwrap();
        state
    }

    fn restriction_form(kind: &str, duration: &str, unit: &str) -> BTreeMap<String, String> {
        form(&[
            ("community", ROOM),
            ("username", MEMBER),
            ("kind", kind),
            ("duration", duration),
            ("duration_unit", unit),
        ])
    }

    #[test]
    fn duration_checks_arithmetic_and_the_supported_calendar() {
        let now = schedule_time::parse_local("2026-11-21T12:00:00", "UTC").unwrap();
        assert_eq!(restriction_expiration(&BTreeMap::new(), now).unwrap(), None);
        for (unit, seconds) in [("minutes", 60), ("hours", 3_600), ("days", 86_400)] {
            assert_eq!(
                restriction_expiration(&restriction_form("ban", "3", unit), now).unwrap(),
                Some(now + 3 * seconds)
            );
        }
        for duration in ["0", "-1", "1.5", "1e2", "18446744073709551615"] {
            assert!(
                restriction_expiration(&restriction_form("ban", duration, "days"), now).is_err()
            );
        }
        assert!(restriction_expiration(&restriction_form("ban", "1", "weeks"), now).is_err());
        assert!(restriction_expiration(&restriction_form("ban", "1000000", "days"), now).is_ok());
        let last_date = schedule_time::parse_local("9999-12-31T23:59:59", "UTC").unwrap();
        assert!(
            restriction_expiration(&restriction_form("ban", "1", "minutes"), last_date).is_err()
        );
    }

    #[test]
    fn expiry_is_exact_durable_and_closes_only_its_own_pending_appeal() {
        let fixture = Fixture::new();
        let mut state = room();
        state
            .apply(
                "restrict-user",
                &restriction_form("ban", "2", "minutes"),
                &fixture.0,
            )
            .unwrap();
        state
            .apply(
                "restrict-user",
                &restriction_form("mute", "", ""),
                &fixture.0,
            )
            .unwrap();
        let ban = &state.restrictions[0];
        let ban_id = ban.id.to_string();
        let deadline = ban.expires_at.unwrap();
        assert_eq!(deadline, ban.created_at + 120);
        assert!(ban.is_active_at(deadline - 1));
        assert!(!ban.is_active_at(deadline));
        let mute_id = state.restrictions[1].id;
        assert_eq!(state.restrictions[1].expires_at, None);
        let record_lengths: Vec<_> = fs::read_to_string(&fixture.0)
            .unwrap()
            .lines()
            .filter(|record| record.starts_with("community-restriction\t"))
            .map(|record| record.split('\t').count())
            .collect();
        assert_eq!(record_lengths, [8, 7]);

        state.activate_profile(Some(MEMBER));
        state.set_notification_preferences(NotificationPreferences {
            moderation: false,
            ..NotificationPreferences::default()
        });
        for id in [&ban_id, &mute_id.to_string()] {
            state
                .apply(
                    "submit-appeal",
                    &form(&[("restriction", id), ("body", "Review appeal")]),
                    &fixture.0,
                )
                .unwrap();
        }
        let mut state = State::load(&fixture.0).unwrap();
        assert_eq!(state.restrictions[0].expires_at, Some(deadline));
        assert_eq!(state.restrictions[1].expires_at, None);
        assert_eq!(
            state.expire_restrictions(&fixture.0, deadline - 1).unwrap(),
            0
        );
        let invalid_path = fixture.0.join("unavailable.state");
        assert!(state.expire_restrictions(&invalid_path, deadline).is_err());
        assert_eq!(state.restrictions.len(), 2);
        assert_eq!(state.appeals[0].status, AppealStatus::Pending);
        assert_eq!(state.expire_restrictions(&fixture.0, deadline).unwrap(), 1);
        assert_eq!(
            state.expire_restrictions(&fixture.0, deadline + 1).unwrap(),
            0
        );

        let state = State::load(&fixture.0).unwrap();
        assert!(!state.is_banned(ROOM, MEMBER));
        assert!(state.is_mod_mail_muted(ROOM, MEMBER));
        assert_eq!(state.restrictions[0].id, mute_id);
        assert_eq!(state.appeals[0].status, AppealStatus::Expired);
        assert_eq!(state.appeals[0].reviewer, None);
        assert_eq!(state.appeals[0].decided_at, Some(deadline));
        assert_eq!(state.appeals[1].status, AppealStatus::Pending);
        let expiry_notice = state
            .notifications
            .iter()
            .find(|notice| notice.body.contains("expired automatically"))
            .unwrap();
        assert_eq!(expiry_notice.recipient, MEMBER);
        assert!(expiry_notice.appeal && expiry_notice.silent);
        assert_eq!(
            state
                .mod_log
                .iter()
                .filter(|event| event.action.contains("Automatic expiry"))
                .count(),
            1
        );
    }

    #[test]
    fn stale_actions_cannot_rewrite_expiry_and_restriction_ids_are_not_reused() {
        let fixture = Fixture::new();
        let mut state = room();
        state
            .apply(
                "restrict-user",
                &restriction_form("ban", "1", "minutes"),
                &fixture.0,
            )
            .unwrap();
        let old_id = state.restrictions[0].id;
        let old_id_text = old_id.to_string();
        state.activate_profile(Some(MEMBER));
        state
            .apply(
                "submit-appeal",
                &form(&[("restriction", &old_id_text), ("body", "Appeal this ban")]),
                &fixture.0,
            )
            .unwrap();

        state.restrictions[0].expires_at = Some(unix_seconds());
        assert!(!state.is_banned(ROOM, MEMBER));
        let old_appeal = state.appeals[0].id.to_string();
        state.activate_profile(Some(OWNER));
        assert!(state
            .apply(
                "lift-restriction",
                &form(&[("community", ROOM), ("id", &old_id_text)]),
                &fixture.0
            )
            .is_err());
        assert!(state
            .apply(
                "decide-appeal",
                &form(&[
                    ("community", ROOM),
                    ("appeal", &old_appeal),
                    ("decision", "approve"),
                    ("response", "Approved")
                ]),
                &fixture.0
            )
            .is_err());
        assert_eq!(state.appeals[0].status, AppealStatus::Pending);

        state
            .apply(
                "restrict-user",
                &restriction_form("ban", "1", "hours"),
                &fixture.0,
            )
            .unwrap();
        assert_eq!(state.restrictions.len(), 1);
        let current = &state.restrictions[0];
        assert_ne!(current.id, old_id);
        assert_eq!(current.expires_at, Some(current.created_at + 3_600));
        assert_eq!(state.appeals[0].status, AppealStatus::Expired);
        let replacement_id = current.id;
        state
            .apply(
                "restrict-user",
                &restriction_form("ban", "", ""),
                &fixture.0,
            )
            .unwrap();
        assert_eq!(state.restrictions[0].id, replacement_id);
        assert_eq!(state.restrictions[0].expires_at, None);

        state
            .apply(
                "restrict-user",
                &restriction_form("mute", "1", "days"),
                &fixture.0,
            )
            .unwrap();
        let mute_id = state.restrictions[1].id.to_string();
        state
            .apply(
                "lift-restriction",
                &form(&[("community", ROOM), ("id", &mute_id)]),
                &fixture.0,
            )
            .unwrap();
        assert!(state.is_banned(ROOM, MEMBER));
        assert!(!state.is_mod_mail_muted(ROOM, MEMBER));
    }
}

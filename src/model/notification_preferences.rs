use super::State;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotificationPreferences {
    pub replies: bool,
    pub messages: bool,
    pub moderation: bool,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            replies: true,
            messages: true,
            moderation: true,
        }
    }
}

impl State {
    pub fn notification_preferences(&self) -> NotificationPreferences {
        self.notification_preferences
    }

    pub fn set_notification_preferences(&mut self, preferences: NotificationPreferences) {
        if !self.username.is_empty() {
            self.notification_preferences = preferences;
        }
    }

    pub(super) fn notification_preferences_for(&self, recipient: &str) -> NotificationPreferences {
        if recipient == self.username {
            self.notification_preferences
        } else {
            self.account_profiles
                .get(recipient)
                .map_or_else(NotificationPreferences::default, |profile| {
                    profile.notification_preferences
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf, time::SystemTime};

    use super::*;

    const ALICE: &str = "alerts_alice";
    const BOB: &str = "alerts_bob";
    const ROOM: &str = "open-debate";

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
    fn notification_preferences_are_recipient_specific_and_survive_reloading() {
        let fixture = StateFile(std::env::temp_dir().join(format!(
            "commonline-notification-preferences-{}-{}.state",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos(),
        )));
        let default = NotificationPreferences::default();
        let muted = NotificationPreferences {
            replies: false,
            messages: false,
            moderation: false,
        };
        let mut state = State::seeded();
        state.activate_profile(Some(ALICE));
        assert_eq!(state.notification_preferences(), default);
        state
            .create_post(&form(&[
                ("community", ROOM),
                ("title", "Alert recipient's post"),
            ]))
            .unwrap();
        let post = state.posts.last().unwrap().id.to_string();
        state.activate_profile(Some(BOB));
        state
            .send_direct_message(&form(&[("recipient", ALICE), ("body", "First message")]))
            .unwrap();
        assert_eq!(state.notifications.len(), 1);
        assert!(!state.notifications[0].read);

        state.save(&fixture.0).unwrap();
        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some(ALICE));
        assert_eq!(
            state.notification_preferences(),
            default,
            "earlier profiles opt in by default"
        );
        assert!(
            !state.notifications[0].silent,
            "earlier notifications stay visible"
        );
        state.set_notification_preferences(muted);
        state.activate_profile(Some(BOB));
        state.save(&fixture.0).unwrap();

        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some(BOB));
        assert_eq!(state.notification_preferences(), default);
        state
            .send_direct_message(&form(&[("recipient", ALICE), ("body", "Second message")]))
            .unwrap();
        state
            .create_comment(&form(&[("post", &post), ("body", "Still delivered")]))
            .unwrap();
        assert_eq!(
            state.notifications.len(),
            1,
            "the existing alert is preserved and no new alert is added"
        );
        assert_eq!(
            state
                .messages
                .iter()
                .filter(|message| message.to == ALICE)
                .count(),
            2
        );
        assert_eq!(state.comments.last().unwrap().body, "Still delivered");
        state.notify_restriction_issued(ALICE, ROOM, "ban").unwrap();
        state
            .push_moderation_notification(ALICE, "A private moderation reason".into())
            .unwrap();
        state
            .push_moderation_notification(BOB, "A regular moderation alert".into())
            .unwrap();
        state.save(&fixture.0).unwrap();

        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some(ALICE));
        assert_eq!(state.notification_preferences(), muted);
        assert!(!state.notifications[0].read);
        assert!(
            state.notifications[1].silent
                && state.notifications[1].read
                && state.notifications[1].appeal
        );
        assert!(
            state.notifications[2].silent
                && state.notifications[2].read
                && state.notifications[2].moderation
        );
        assert_eq!(state.notifications[2].body, "A private moderation reason");
        assert!(!state.notifications[3].silent && !state.notifications[3].read);
        state
            .send_direct_message(&form(&[
                ("recipient", BOB),
                ("body", "Sender preference is irrelevant"),
            ]))
            .unwrap();
        assert_eq!(state.notifications.last().unwrap().recipient, BOB);
    }
}

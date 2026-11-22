#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Appearance {
    #[default]
    Light,
    Dark,
}

impl Appearance {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::Appearance;
    use crate::model::{FeedLayout, State};

    struct StateFile(PathBuf);

    impl Drop for StateFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn appearance_persists_per_account_without_affecting_other_preferences() {
        let fixture = StateFile(std::env::temp_dir().join(format!(
            "commonline-appearance-{}-{}.state",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        let mut state = State::seeded();
        state.activate_profile(Some("appearance_alice"));
        state.save(&fixture.0).unwrap();

        let mut state = State::load(&fixture.0).unwrap();
        assert_eq!(state.appearance, Appearance::Light);
        let dark = BTreeMap::from([("appearance".into(), "dark".into())]);
        state.apply("set-appearance", &dark, &fixture.0).unwrap();
        let invalid = BTreeMap::from([("appearance".into(), "system".into())]);
        assert!(state.apply("set-appearance", &invalid, &fixture.0).is_err());
        state
            .apply(
                "set-feed-layout",
                &BTreeMap::from([("layout".into(), "compact".into())]),
                &fixture.0,
            )
            .unwrap();
        assert_eq!(state.appearance, Appearance::Dark);

        state.activate_profile(Some("appearance_bob"));
        assert_eq!(state.appearance, Appearance::Light);
        state.activate_profile(None);
        assert!(state.apply("set-appearance", &dark, &fixture.0).is_err());
        state.appearance = Appearance::Dark;
        state.save(&fixture.0).unwrap();

        let mut state = State::load(&fixture.0).unwrap();
        assert_eq!(state.appearance, Appearance::Light);
        state.activate_profile(Some("appearance_bob"));
        assert_eq!(state.appearance, Appearance::Light);
        state.activate_profile(Some("appearance_alice"));
        assert_eq!(state.appearance, Appearance::Dark);
        assert_eq!(state.feed_layout, FeedLayout::Compact);

        let light = BTreeMap::from([("appearance".into(), "light".into())]);
        state.apply("set-appearance", &light, &fixture.0).unwrap();
        let state = State::load(&fixture.0).unwrap();
        assert_eq!(state.appearance, Appearance::Light);
        assert_eq!(state.feed_layout, FeedLayout::Compact);
    }
}

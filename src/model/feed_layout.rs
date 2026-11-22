#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FeedLayout {
    #[default]
    Card,
    Compact,
}

impl FeedLayout {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "card" => Some(Self::Card),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::Compact => "compact",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::FeedLayout;
    use crate::model::State;

    struct StateFile(PathBuf);

    impl Drop for StateFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn feed_layout_defaults_for_older_profiles_and_persists_per_account() {
        let fixture = StateFile(std::env::temp_dir().join(format!(
            "commonline-feed-layout-{}-{}.state",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        )));
        let mut state = State::seeded();
        state.activate_profile(Some("layout_alice"));
        state.save(&fixture.0).unwrap();

        let mut state = State::load(&fixture.0).unwrap();
        assert_eq!(state.feed_layout, FeedLayout::Card);
        let compact = BTreeMap::from([("layout".into(), "compact".into())]);
        let invalid = BTreeMap::from([("layout".into(), "expanded".into())]);
        state
            .apply("set-feed-layout", &compact, &fixture.0)
            .unwrap();
        assert!(state
            .apply("set-feed-layout", &invalid, &fixture.0)
            .is_err());
        state
            .apply(
                "preferences",
                &BTreeMap::from([("sensitive".into(), "hide".into())]),
                &fixture.0,
            )
            .unwrap();
        assert_eq!(state.feed_layout, FeedLayout::Compact);

        state.activate_profile(Some("layout_bob"));
        assert_eq!(state.feed_layout, FeedLayout::Card);
        state.save(&fixture.0).unwrap();
        let mut state = State::load(&fixture.0).unwrap();
        assert_eq!(state.feed_layout, FeedLayout::Card);

        state.activate_profile(None);
        assert!(state
            .apply("set-feed-layout", &compact, &fixture.0)
            .is_err());
        assert_eq!(state.feed_layout, FeedLayout::Card);
        state.activate_profile(Some("layout_alice"));
        assert_eq!(state.feed_layout, FeedLayout::Compact);

        let card = BTreeMap::from([("layout".into(), "card".into())]);
        state.apply("set-feed-layout", &card, &fixture.0).unwrap();
        let state = State::load(&fixture.0).unwrap();
        assert_eq!(state.feed_layout, FeedLayout::Card);
        assert_eq!(state.sensitive, "hide");
    }
}

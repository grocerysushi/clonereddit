use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use super::{unix_seconds, Post, State, HISTORY_LIMIT};

const DAYS_RETAINED: u64 = 30;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PostInsightDay {
    pub day: u64,
    pub views: u64,
    pub votes: i64,
    pub comments: u64,
}

#[derive(Clone)]
pub(super) struct PostInsights {
    pub(super) since: u64,
    by_post: BTreeMap<u64, BTreeMap<u64, PostInsightDay>>,
}

impl PostInsights {
    pub(super) fn new(now: u64) -> Self {
        Self {
            since: utc_day(now),
            by_post: BTreeMap::new(),
        }
    }

    fn get(&self, id: u64, day: u64) -> Option<PostInsightDay> {
        self.by_post.get(&id)?.get(&day).copied()
    }

    fn day_mut(&mut self, id: u64, day: u64) -> &mut PostInsightDay {
        self.by_post
            .entry(id)
            .or_default()
            .entry(day)
            .or_insert_with(|| PostInsightDay {
                day,
                ..PostInsightDay::default()
            })
    }

    fn restore_day(&mut self, id: u64, day: u64, previous: Option<PostInsightDay>) {
        if let Some(bucket) = previous {
            *self.day_mut(id, day) = bucket;
        } else if let Some(days) = self.by_post.get_mut(&id) {
            days.remove(&day);
            if days.is_empty() {
                self.by_post.remove(&id);
            }
        }
    }

    fn prune_post(&mut self, id: u64, today: u64) {
        if let Some(days) = self.by_post.get_mut(&id) {
            days.retain(|day, _| *day >= first_retained_day(today));
        }
    }

    pub(super) fn prune_all(&mut self, now: u64) {
        let first = first_retained_day(utc_day(now));
        self.by_post.retain(|_, days| {
            days.retain(|day, _| *day >= first);
            !days.is_empty()
        });
    }

    pub(super) fn remove(&mut self, id: u64) {
        self.by_post.remove(&id);
    }

    pub(super) fn read_day(&mut self, id: u64, bucket: PostInsightDay) -> Option<()> {
        if bucket.day < self.since || self.get(id, bucket.day).is_some() {
            return None;
        }
        *self.day_mut(id, bucket.day) = bucket;
        Some(())
    }

    pub(super) fn write(&self, file: &mut impl Write, now: u64) -> io::Result<()> {
        writeln!(file, "post-insights-start\t{}", self.since)?;
        let first = first_retained_day(utc_day(now));
        for (id, days) in &self.by_post {
            for (day, bucket) in days.range(first..) {
                writeln!(
                    file,
                    "post-insight\t{id}\t{day}\t{}\t{}\t{}",
                    bucket.views, bucket.votes, bucket.comments
                )?;
            }
        }
        Ok(())
    }
}

impl State {
    pub fn post_insight_days(&self, post: &Post, days: u64, now: u64) -> Vec<PostInsightDay> {
        let today = utc_day(now);
        let Some(offset) = days.min(DAYS_RETAINED).checked_sub(1) else {
            return Vec::new();
        };
        let created = post.created_at.map_or(self.post_insights.since, utc_day);
        let first = today
            .saturating_sub(offset)
            .max(self.post_insights.since)
            .max(created);
        (first..=today)
            .map(|day| {
                self.post_insights
                    .get(post.id, day)
                    .unwrap_or(PostInsightDay {
                        day,
                        ..PostInsightDay::default()
                    })
            })
            .collect()
    }

    pub fn record_post_visit(&mut self, id: u64, path: &Path) {
        if self.viewable_post_community(id).is_none() {
            return;
        }
        let today = utc_day(unix_seconds());
        let track_view = today >= self.post_insights.since
            && self
                .posts
                .iter()
                .any(|post| post.id == id && post.author != self.username);
        let update_history = !self.guest() && self.history.first() != Some(&id);
        if !track_view && !update_history {
            return;
        }

        let previous_history = update_history.then(|| self.history.clone());
        if update_history {
            self.history.retain(|visited| *visited != id);
            self.history.insert(0, id);
            self.history.truncate(HISTORY_LIMIT);
        }
        let previous_day = if track_view {
            let previous = self.post_insights.get(id, today);
            let bucket = self.post_insights.day_mut(id, today);
            bucket.views = bucket.views.saturating_add(1);
            previous
        } else {
            None
        };

        if self.save(path).is_err() {
            if let Some(previous) = previous_history {
                self.history = previous;
            }
            if track_view {
                self.post_insights.restore_day(id, today, previous_day);
            }
        } else if track_view {
            self.post_insights.prune_post(id, today);
        }
    }

    fn track_post_engagement(&mut self, id: u64, update: impl FnOnce(&mut PostInsightDay)) {
        let today = utc_day(unix_seconds());
        if today >= self.post_insights.since {
            update(self.post_insights.day_mut(id, today));
            self.post_insights.prune_post(id, today);
        }
    }

    pub(super) fn record_post_vote_change(&mut self, id: u64, delta: i64) {
        self.track_post_engagement(id, |bucket| {
            bucket.votes = bucket.votes.saturating_add(delta);
        });
    }

    pub(super) fn record_post_comment(&mut self, id: u64) {
        self.track_post_engagement(id, |bucket| {
            bucket.comments = bucket.comments.saturating_add(1);
        });
    }
}

fn utc_day(now: u64) -> u64 {
    now / 86_400
}

fn first_retained_day(today: u64) -> u64 {
    today.saturating_sub(DAYS_RETAINED - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "commonline-insights-{}-{nonce}.state",
                std::process::id()
            )))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn apply(state: &mut State, fixture: &Fixture, action: &str, fields: &[(&str, &str)]) {
        let form = fields
            .iter()
            .map(|&(key, value)| (key.into(), value.into()))
            .collect();
        assert!(state.apply(action, &form, &fixture.0).is_ok(), "{action}");
    }

    #[test]
    fn tracks_non_author_views_and_committed_engagement_across_reload() {
        let fixture = Fixture::new();
        let mut state = State::seeded();
        state.activate_profile(Some("insight_author"));
        apply(
            &mut state,
            &fixture,
            "create",
            &[
                ("community", "open-debate"),
                ("title", "A post with insights"),
            ],
        );
        let post = state.posts.last().unwrap().clone();
        let id = post.id.to_string();

        state.record_post_visit(post.id, &fixture.0);
        assert_eq!(state.history.first(), Some(&post.id));
        state.activate_profile(Some("insight_reader"));
        state.record_post_visit(post.id, &fixture.0.join("invalid"));
        assert!(state.history.is_empty());
        state.record_post_visit(post.id, &fixture.0);
        state.record_post_visit(post.id, &fixture.0);
        let day = utc_day(unix_seconds());
        assert_eq!(state.post_insights.get(post.id, day).unwrap().views, 2);
        assert_eq!(state.history, [post.id]);

        for (direction, expected) in [("1", 1), ("-1", -1), ("-1", 0), ("1", 1)] {
            apply(
                &mut state,
                &fixture,
                "vote",
                &[("id", &id), ("direction", direction)],
            );
            assert_eq!(
                state.post_insights.get(post.id, day).unwrap().votes,
                expected
            );
        }
        apply(
            &mut state,
            &fixture,
            "comment",
            &[("post", &id), ("body", "A visible comment.")],
        );
        let comment = state.comments.last().unwrap().id.to_string();
        apply(&mut state, &fixture, "delete-comment", &[("id", &comment)]);

        state.activate_profile(None);
        state.record_post_visit(post.id, &fixture.0);
        assert!(state.history.is_empty());
        let mut restored = State::load(&fixture.0).unwrap();
        assert_eq!(
            restored.post_insight_days(&post, 7, unix_seconds()).last(),
            Some(&PostInsightDay {
                day,
                views: 3,
                votes: 1,
                comments: 1,
            })
        );
        restored.activate_profile(Some("insight_author"));
        apply(&mut restored, &fixture, "delete", &[("id", &id)]);
        assert!(restored.post_insights.get(post.id, day).is_none());
        assert!(State::load(&fixture.0)
            .unwrap()
            .post_insights
            .get(post.id, day)
            .is_none());
    }

    #[test]
    fn prunes_old_buckets_and_starts_legacy_posts_without_fabricating_history() {
        let fixture = Fixture::new();
        let mut state = State::seeded();
        let now = unix_seconds();
        let today = utc_day(now);
        state.posts[0].created_at = Some((today - 45) * 86_400);
        let post = state.posts[0].clone();
        state.save(&fixture.0).unwrap();
        let old_snapshot = fs::read_to_string(&fixture.0)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("post-insights-start\t"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&fixture.0, old_snapshot).unwrap();

        let mut restored = State::load(&fixture.0).unwrap();
        assert_eq!(
            restored.post_insight_days(&post, 30, now),
            vec![PostInsightDay {
                day: today,
                ..PostInsightDay::default()
            }]
        );

        restored.post_insights.since = today - 45;
        restored.post_insights.day_mut(post.id, today - 29).views = 3;
        restored.post_insights.day_mut(post.id, today - 30).views = 9;
        restored.save(&fixture.0).unwrap();
        let restored = State::load(&fixture.0).unwrap();
        assert!(restored.post_insights.get(post.id, today - 30).is_none());
        let days = restored.post_insight_days(&post, 30, now);
        assert_eq!(days.len(), 30);
        assert_eq!(days.first().unwrap().views, 3);
        assert_eq!(days.last().unwrap().views, 0);
    }

    #[test]
    fn unavailable_thread_opens_never_enter_insights_or_history() {
        let fixture = Fixture::new();
        let mut state = State::seeded();
        state.activate_profile(Some("insight_owner"));
        apply(
            &mut state,
            &fixture,
            "create-community",
            &[
                ("name", "Insights private discussion"),
                ("slug", "insight-private"),
                ("visibility", "private"),
            ],
        );
        apply(
            &mut state,
            &fixture,
            "create",
            &[
                ("community", "insight-private"),
                ("title", "Private insights"),
            ],
        );
        let post = state.posts.last().unwrap().id;
        let id = post.to_string();
        let today = utc_day(unix_seconds());

        state.activate_profile(Some("insight_other"));
        state.record_post_visit(post, &fixture.0);
        assert!(state.history.is_empty());
        assert!(state.post_insights.get(post, today).is_none());

        state.activate_profile(Some("insight_owner"));
        apply(
            &mut state,
            &fixture,
            "approve-member",
            &[
                ("community", "insight-private"),
                ("handle", "insight_other"),
            ],
        );
        apply(
            &mut state,
            &fixture,
            "remove-content",
            &[
                ("community", "insight-private"),
                ("id", &id),
                ("kind", "post"),
                ("reason", "Removed in an insights fixture."),
            ],
        );
        state.activate_profile(Some("insight_other"));
        state.record_post_visit(post, &fixture.0);
        assert!(state.history.is_empty());
        assert!(state.post_insights.get(post, today).is_none());
    }
}

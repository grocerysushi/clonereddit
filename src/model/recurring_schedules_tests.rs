use super::*;
use std::path::PathBuf;

const OWNER: &str = "recurring_owner";
const COMMUNITY: &str = "recurring-posts";
const DAY: u64 = 86_400;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> (Self, State) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Self(std::env::temp_dir().join(format!(
            "commonline-recurring-schedule-{}-{unique}.state",
            std::process::id(),
        )));
        let mut state = State::seeded();
        state.activate_profile(Some(OWNER));
        state
            .apply(
                "create-community",
                &fields(&[("slug", COMMUNITY), ("name", "Recurring posts")]),
                &fixture.0,
            )
            .unwrap();
        (fixture, state)
    }

    fn reload(&self) -> State {
        let mut state = State::load(&self.0).unwrap();
        state.activate_profile(Some(OWNER));
        state
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn fields(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|&(key, value)| (key.into(), value.into()))
        .collect()
}

fn schedule_form(title: &str, publish_at: u64) -> BTreeMap<String, String> {
    fields(&[
        ("community", COMMUNITY),
        ("kind", "post"),
        ("title", title),
        ("zone", "UTC"),
        ("publish_at", &publish_at.to_string()),
    ])
}

#[test]
fn overdue_series_publishes_once_and_cancel_preserves_history() {
    let (fixture, mut state) = Fixture::new();
    let first = unix_seconds() + 3_600;
    let mut recurring = schedule_form("Daily check-in", first);
    recurring.insert("repeat".into(), "daily".into());
    state
        .apply("schedule-item", &recurring, &fixture.0)
        .unwrap();
    state
        .apply(
            "schedule-item",
            &schedule_form("One-off post", first),
            &fixture.0,
        )
        .unwrap();
    let series_id = state.scheduled[0].id;
    let one_off_id = state.scheduled[1].id;
    let posts_before = state.posts.len();
    let replay_at = first + 2 * DAY + 3_600;
    let unavailable = fixture.0.with_extension("missing").join("state");
    assert!(state.publish_due(&unavailable, replay_at).is_err());
    assert_eq!(state.posts.len(), posts_before);
    assert_eq!(state.scheduled.len(), 2);
    assert_eq!(state.scheduled[0].publish_at, first);

    assert_eq!(state.publish_due(&fixture.0, replay_at).unwrap(), 2);
    let mut state = fixture.reload();
    assert_eq!(state.posts.len(), posts_before + 2);
    assert_eq!(state.scheduled.len(), 3);
    let series = &state.scheduled[0];
    assert_eq!((series.id, series.status.as_str()), (series_id, "pending"));
    assert_eq!(series.publish_at, first + 3 * DAY);
    assert_eq!(series.recurrence.unwrap().anchor_at, first);
    assert!(series.post_id.is_none());
    assert_eq!(state.scheduled[1].id, one_off_id);
    assert_eq!(state.scheduled[1].status, "published");
    assert!(state.scheduled[1].recurrence.is_none());
    let occurrence = &state.scheduled[2];
    assert_ne!(occurrence.id, series_id);
    assert_ne!(occurrence.id, one_off_id);
    assert_eq!(occurrence.status, "published");
    assert_eq!(occurrence.publish_at, first);
    assert_eq!(occurrence.post_id, Some(state.posts[posts_before].id));
    assert_eq!(state.publish_due(&fixture.0, replay_at).unwrap(), 0);

    let revised = first + 4 * DAY;
    let mut edit = schedule_form("Weekly check-in", revised);
    edit.insert("id".into(), series_id.to_string());
    edit.insert("repeat".into(), "weekly".into());
    state
        .apply("edit-scheduled-item", &edit, &fixture.0)
        .unwrap();
    let recurrence = state.scheduled[0].recurrence.unwrap();
    assert_eq!(recurrence.cadence, Cadence::Weekly);
    assert_eq!(recurrence.anchor_at, revised);
    state
        .apply(
            "cancel-scheduled-item",
            &fields(&[("community", COMMUNITY), ("id", &series_id.to_string())]),
            &fixture.0,
        )
        .unwrap();
    let mut state = fixture.reload();
    assert_eq!(state.scheduled[0].status, "canceled");
    assert_eq!(state.scheduled[2].status, "published");
    assert_eq!(state.scheduled[2].title, "Daily check-in");
    assert_eq!(
        state.publish_due(&fixture.0, revised + 14 * DAY).unwrap(),
        0
    );
    assert_eq!(state.posts.len(), posts_before + 2);
}

#[test]
fn monthly_series_preserves_its_anchor_after_a_content_only_edit() {
    let (fixture, mut state) = Fixture::new();
    let today = schedule_time::to_local(unix_seconds(), "UTC");
    let year = today[..4].parse::<u64>().unwrap() + 1;
    let at = |day| schedule_time::parse_local(&format!("{year:04}-{day}T13:00"), "UTC").unwrap();
    let january = at("01-31");
    let mut original = schedule_form("Monthly announcement", january);
    original.insert("repeat".into(), "monthly".into());
    state.apply("schedule-item", &original, &fixture.0).unwrap();
    assert_eq!(state.publish_due(&fixture.0, january).unwrap(), 1);
    let february = state.scheduled[0].publish_at;
    assert!(schedule_time::to_local(february, "UTC").starts_with(&format!("{year:04}-02-")));
    let mut edit = schedule_form("Revised announcement", february);
    edit.insert("id".into(), state.scheduled[0].id.to_string());
    state
        .apply("edit-scheduled-item", &edit, &fixture.0)
        .unwrap();
    let mut state = fixture.reload();
    assert_eq!(state.scheduled[0].recurrence.unwrap().anchor_at, january);
    assert_eq!(state.publish_due(&fixture.0, february).unwrap(), 1);
    assert_eq!(state.scheduled[0].publish_at, at("03-31"));
    assert_eq!(state.scheduled[1].title, "Monthly announcement");
    assert_eq!(state.scheduled[2].title, "Revised announcement");

    let mut event = schedule_form("Invalid recurring event", at("03-31"));
    event.insert("kind".into(), "event".into());
    event.insert("event_start".into(), at("03-31").to_string());
    event.insert("repeat".into(), "monthly".into());
    assert!(state.apply("schedule-item", &event, &fixture.0).is_err());
    assert_eq!(state.scheduled.len(), 3);
}

use std::collections::{BTreeMap, BTreeSet};

use super::{id_field, number, value, Community, ContentKind, ModPermission, State};

impl Community {
    pub fn highlighted_posts(&self) -> &[u64] {
        &self.highlighted_posts
    }

    pub fn is_highlighted(&self, post_id: u64) -> bool {
        self.highlighted_posts.contains(&post_id)
    }
}

impl State {
    pub(super) fn set_highlight(
        &mut self,
        form: &BTreeMap<String, String>,
        pinned: bool,
    ) -> Result<&'static str, String> {
        let (index, id) = self.highlight_target(form)?;
        let unavailable = pinned && self.is_post_removed(id);
        let community = &mut self.communities[index];
        let order = &mut community.highlighted_posts;
        let position = order.iter().position(|&item| item == id);
        let (action, notice) = match (pinned, position) {
            (true, Some(_)) => return Ok("Post highlighted."),
            (true, None) if unavailable => {
                return Err("Restore this post before highlighting it.".into())
            }
            (true, None) => {
                order.push(id);
                ("highlighted", "Post highlighted.")
            }
            (false, Some(position)) => {
                order.remove(position);
                ("unpinned from highlights", "Post unpinned.")
            }
            (false, None) => return Ok("Post unpinned."),
        };
        let slug = community.slug.clone();
        self.record_moderation_action(
            &slug,
            &format!("Post #{id} {action} by u/{}", self.username),
        )?;
        Ok(notice)
    }

    pub(super) fn move_highlight(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let (index, id) = self.highlight_target(form)?;
        let community = &mut self.communities[index];
        let order = &mut community.highlighted_posts;
        let position = order
            .iter()
            .position(|&item| item == id)
            .ok_or("Pin this post before moving it.")?;
        let direction = value(form, "direction");
        let neighbor = match direction {
            "up" => position.checked_sub(1),
            "down" => Some(position + 1).filter(|&next| next < order.len()),
            _ => return Err("Choose whether to move this highlight up or down.".into()),
        };
        if form.get("neighbor").is_some_and(|expected| {
            neighbor.is_none_or(|index| number(expected) != Some(order[index]))
        }) {
            return Err("Highlights changed. Refresh the page and try again.".into());
        }
        let Some(neighbor) = neighbor else {
            return Ok("Highlight is already at that end of the list.");
        };
        order.swap(position, neighbor);
        let slug = community.slug.clone();
        self.record_moderation_action(
            &slug,
            &format!("Highlight #{id} moved {direction} by u/{}", self.username),
        )?;
        Ok("Highlight moved.")
    }

    fn highlight_target(&self, form: &BTreeMap<String, String>) -> Result<(usize, u64), String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let id = id_field(form, "post")?;
        self.content_moderation_target(ContentKind::Post, id, &self.communities[index].slug)
            .ok_or("Choose a post from this community.")?;
        Ok((index, id))
    }

    pub(super) fn pin_community(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.owned_community_index(form)?;
        let slug = self.communities[index].slug.clone();
        if value(form, "post").is_empty() {
            let order = &mut self.communities[index].highlighted_posts;
            if !order.is_empty() {
                order.remove(0);
            }
            return self.record_moderation(&slug, "Discussion unpinned.");
        }
        let id = id_field(form, "post")?;
        if self
            .viewable_post_community(id)
            .is_none_or(|community| community.slug != slug)
        {
            return Err("Choose a discussion from this community to pin.".into());
        }
        let order = &mut self.communities[index].highlighted_posts;
        if order.first() != Some(&id) {
            if !order.is_empty() {
                order.remove(0);
            }
            order.retain(|&post_id| post_id != id);
            order.insert(0, id);
        }
        self.record_moderation(&slug, "Discussion pinned.")
    }
}

pub(super) fn read_order(stored: &str) -> Option<Vec<u64>> {
    if stored.is_empty() {
        return Some(Vec::new());
    }
    let mut seen = BTreeSet::new();
    stored
        .split(',')
        .map(|field| number(field).filter(|id| seen.insert(*id)))
        .collect()
}

pub(super) fn write_order(order: &[u64]) -> String {
    order
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use super::*;

    const ROOM: &str = "highlight-room";
    const OTHER: &str = "elsewhere";
    const OWNER: &str = "highlight_owner";
    const OTHER_OWNER: &str = "other_owner";
    const MODERATOR: &str = "highlight_mod";
    const REVIEWER: &str = "highlight_reviewer";
    const MEMBER: &str = "highlight_member";

    struct Fixture(PathBuf);

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

    fn order<'a>(state: &'a State, slug: &str) -> &'a [u64] {
        state
            .communities
            .iter()
            .find(|item| item.slug == slug)
            .unwrap()
            .highlighted_posts()
    }

    fn publish(state: &mut State, slug: &str, title: &str) -> u64 {
        state
            .create_post(&form(&[("community", slug), ("title", title)]))
            .unwrap();
        state.posts.last().unwrap().id
    }

    #[test]
    fn legacy_pin_and_ordered_highlights_preserve_community_scope_and_post_lifecycle() {
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "commonline-highlights-{}-{}.state",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos(),
        )));
        let mut state = State::seeded();
        state.activate_profile(Some(OTHER_OWNER));
        state
            .create_community(&form(&[("slug", OTHER), ("name", "Other community")]))
            .unwrap();
        let foreign = publish(&mut state, OTHER, "Unrelated post");
        state.activate_profile(Some(OWNER));
        state
            .create_community(&form(&[("slug", ROOM), ("name", "Highlight community")]))
            .unwrap();
        for (handle, role) in [(MODERATOR, "moderator"), (REVIEWER, "appeals-reviewer")] {
            state
                .set_moderator_role(&form(&[
                    ("community", ROOM),
                    ("handle", handle),
                    ("role", role),
                ]))
                .unwrap();
        }
        let first = publish(&mut state, ROOM, "Legacy pin");
        let second = publish(&mut state, ROOM, "Second post");
        let third = publish(&mut state, ROOM, "Third post");
        let target = |id: u64| form(&[("community", ROOM), ("post", &id.to_string())]);
        let move_form = |id: u64, direction: &str| {
            form(&[
                ("community", ROOM),
                ("post", &id.to_string()),
                ("direction", direction),
            ])
        };

        state
            .apply("pin-community", &target(first), &fixture.0)
            .unwrap();
        let stored = fs::read_to_string(&fixture.0).unwrap();
        let legacy_pin = stored
            .lines()
            .find(|line| line.starts_with("community\thighlight-room\t"))
            .unwrap();
        assert_eq!(
            legacy_pin.split('\t').nth(9),
            Some(first.to_string().as_str())
        );
        let mut state = State::load(&fixture.0).unwrap();
        assert_eq!(order(&state, ROOM), &[first]);

        for viewer in [MEMBER, REVIEWER, OTHER_OWNER] {
            state.activate_profile(Some(viewer));
            assert!(state
                .apply("highlight-post", &target(second), &fixture.0)
                .is_err());
            assert!(state
                .apply("unhighlight-post", &target(first), &fixture.0)
                .is_err());
            assert!(state
                .apply("move-highlight", &move_form(first, "up"), &fixture.0)
                .is_err());
        }
        state.activate_profile(Some(MODERATOR));
        assert!(state
            .apply("highlight-post", &target(foreign), &fixture.0)
            .is_err());
        assert!(state
            .apply("move-highlight", &move_form(third, "up"), &fixture.0)
            .is_err());
        let moderation = form(&[
            ("community", ROOM),
            ("kind", "post"),
            ("id", &third.to_string()),
            ("reason", "Test removal"),
        ]);
        state
            .apply("remove-content", &moderation, &fixture.0)
            .unwrap();
        assert!(state
            .apply("highlight-post", &target(third), &fixture.0)
            .is_err());
        state
            .apply("restore-content", &moderation, &fixture.0)
            .unwrap();
        for id in [second, second, third] {
            state
                .apply("highlight-post", &target(id), &fixture.0)
                .unwrap();
        }
        assert_eq!(order(&state, ROOM), &[first, second, third]);
        assert!(state
            .apply("move-highlight", &move_form(third, "last"), &fixture.0)
            .is_err());
        state
            .apply("move-highlight", &move_form(first, "up"), &fixture.0)
            .unwrap();
        let mut boundary_move = move_form(first, "up");
        boundary_move.insert("neighbor".into(), second.to_string());
        assert_eq!(
            state
                .apply("move-highlight", &boundary_move, &fixture.0)
                .unwrap_err(),
            "Highlights changed. Refresh the page and try again."
        );
        let mut guarded_move = move_form(third, "up");
        guarded_move.insert("neighbor".into(), second.to_string());
        state
            .apply("move-highlight", &guarded_move, &fixture.0)
            .unwrap();
        assert_eq!(
            state
                .apply("move-highlight", &guarded_move, &fixture.0)
                .unwrap_err(),
            "Highlights changed. Refresh the page and try again."
        );
        state
            .apply("move-highlight", &move_form(first, "down"), &fixture.0)
            .unwrap();
        assert_eq!(order(&state, ROOM), &[third, first, second]);
        assert!(order(&state, OTHER).is_empty());
        assert_eq!(
            state
                .mod_log
                .iter()
                .filter(|entry| entry.action.contains("highlighted by"))
                .count(),
            2
        );
        assert_eq!(
            state
                .mod_log
                .iter()
                .filter(|entry| entry.action.starts_with("Highlight #"))
                .count(),
            2
        );

        state
            .apply("remove-content", &moderation, &fixture.0)
            .unwrap();
        let mut state = State::load(&fixture.0).unwrap();
        assert!(state.is_post_removed(third));
        assert_eq!(order(&state, ROOM), &[third, first, second]);
        state.activate_profile(Some(MODERATOR));
        state
            .apply("restore-content", &moderation, &fixture.0)
            .unwrap();
        for _ in 0..2 {
            state
                .apply("unhighlight-post", &target(first), &fixture.0)
                .unwrap();
        }
        assert_eq!(
            state
                .mod_log
                .iter()
                .filter(|entry| entry.action.contains("unpinned from highlights"))
                .count(),
            1
        );
        state.activate_profile(Some(OWNER));
        state
            .apply("delete", &form(&[("id", &third.to_string())]), &fixture.0)
            .unwrap();
        let state = State::load(&fixture.0).unwrap();
        assert_eq!(order(&state, ROOM), &[second]);
        assert!(order(&state, OTHER).is_empty());
    }
}

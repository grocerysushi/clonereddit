use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    bounded, canonical_u64, decode, encode, next_id, required, value, Community, ModPermission,
    State,
};

pub const COMMUNITY_RULE_TITLE_MAX: usize = 100;
pub const COMMUNITY_RULE_DESCRIPTION_MAX: usize = 1000;

#[derive(Clone)]
pub struct CommunityRule {
    pub id: u64,
    pub title: String,
    pub description: String,
}

impl Community {
    pub fn rules(&self) -> &[CommunityRule] {
        &self.rules
    }

    pub(super) fn write_rules(&self, file: &mut impl Write) -> io::Result<()> {
        for rule in &self.rules {
            writeln!(
                file,
                "community-rule\t{}\t{}\t{}\t{}",
                encode(&self.slug),
                rule.id,
                encode(&rule.title),
                encode(&rule.description)
            )?;
        }
        Ok(())
    }
}

impl State {
    pub(super) fn save_community_rule(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let rules = &mut self.communities[index].rules;
        let edit = if value(form, "id").is_empty() {
            None
        } else {
            Some(rule_position(rules, form)?)
        };
        let title = required(form, "title", "Rule title", COMMUNITY_RULE_TITLE_MAX)?;
        if title.chars().any(char::is_control) {
            return Err("Enter the rule title on one line.".into());
        }
        let description = bounded(
            form,
            "description",
            "Rule description",
            COMMUNITY_RULE_DESCRIPTION_MAX,
        )?;
        let folded = title.to_lowercase();
        if rules
            .iter()
            .enumerate()
            .any(|(position, rule)| Some(position) != edit && rule.title.to_lowercase() == folded)
        {
            return Err("A rule with that title already exists in this community.".into());
        }

        let (id, action, notice) = if let Some(position) = edit {
            let rule = &mut rules[position];
            if rule.title == title && rule.description == description {
                return Ok("Community rule updated.");
            }
            rule.title = title.into();
            rule.description = description.into();
            (rule.id, "updated", "Community rule updated.")
        } else {
            let id = next_id(&mut self.next_community_rule)?;
            rules.push(CommunityRule {
                id,
                title: title.into(),
                description: description.into(),
            });
            (id, "added", "Community rule added.")
        };
        let slug = self.communities[index].slug.clone();
        self.record_moderation_action(
            &slug,
            &format!("Community rule #{id} {action} by u/{}", self.username),
        )?;
        Ok(notice)
    }

    pub(super) fn move_community_rule(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let rules = &mut self.communities[index].rules;
        let position = rule_position(rules, form)?;
        let direction = value(form, "direction");
        let neighbor = match direction {
            "up" => position.checked_sub(1),
            "down" => Some(position + 1).filter(|&next| next < rules.len()),
            _ => return Err("Choose whether to move this community rule up or down.".into()),
        };
        if form.get("neighbor").is_some_and(|expected| {
            neighbor.is_none_or(|next| canonical_u64(expected) != Some(rules[next].id))
        }) {
            return Err("Community rules changed. Refresh the page and try again.".into());
        }
        let Some(neighbor) = neighbor else {
            return Ok("Community rule is already at that end of the list.");
        };
        let id = rules[position].id;
        rules.swap(position, neighbor);
        let slug = self.communities[index].slug.clone();
        self.record_moderation_action(
            &slug,
            &format!(
                "Community rule #{id} moved {direction} by u/{}",
                self.username
            ),
        )?;
        Ok("Community rule moved.")
    }

    pub(super) fn read_community_rule(
        &mut self,
        slug: &str,
        id: &str,
        title: &str,
        description: &str,
    ) -> Option<()> {
        let slug = decode(slug)?;
        let id = canonical_u64(id)?;
        let title = decode(title)?;
        let description = decode(description)?;
        if id == 0
            || title.trim().is_empty()
            || title.chars().count() > COMMUNITY_RULE_TITLE_MAX
            || title.chars().any(char::is_control)
            || description.chars().count() > COMMUNITY_RULE_DESCRIPTION_MAX
            || self
                .communities
                .iter()
                .flat_map(Community::rules)
                .any(|rule| rule.id == id)
        {
            return None;
        }
        let item = self
            .communities
            .iter_mut()
            .find(|item| item.slug == slug && item.owner.is_some())?;
        self.next_community_rule = self.next_community_rule.max(id.checked_add(1)?);
        item.rules.push(CommunityRule {
            id,
            title,
            description,
        });
        Some(())
    }
}

fn rule_position(
    rules: &[CommunityRule],
    form: &BTreeMap<String, String>,
) -> Result<usize, String> {
    let id = canonical_u64(value(form, "id")).ok_or("Choose a valid community rule.")?;
    rules
        .iter()
        .position(|rule| rule.id == id)
        .ok_or_else(|| "This rule is unavailable in this community.".into())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const ROOM: &str = "rule-workshop";
    const OTHER: &str = "other-rule-workshop";
    const MODERATOR: &str = "rule_moderator";
    const REVIEWER: &str = "rule_reviewer";
    const OTHER_OWNER: &str = "rule_other_owner";

    struct TestStateFile(PathBuf);

    impl Drop for TestStateFile {
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

    fn rules<'a>(state: &'a State, slug: &str) -> &'a [CommunityRule] {
        state
            .communities
            .iter()
            .find(|item| item.slug == slug)
            .unwrap()
            .rules()
    }

    #[test]
    fn rules_are_scoped_and_keep_identity_and_order_after_restart() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let file = TestStateFile(std::env::temp_dir().join(format!(
            "commonline-community-rules-{}-{nonce}.state",
            std::process::id()
        )));
        let mut state = State::seeded();
        state.activate_profile(Some("rule_owner"));
        state
            .apply(
                "create-community",
                &form(&[
                    ("slug", ROOM),
                    ("name", "Workshop"),
                    ("guidelines", "General guidance"),
                ]),
                &file.0,
            )
            .unwrap();
        for (handle, role) in [(MODERATOR, "moderator"), (REVIEWER, "appeals-reviewer")] {
            state
                .apply(
                    "set-moderator-role",
                    &form(&[("community", ROOM), ("handle", handle), ("role", role)]),
                    &file.0,
                )
                .unwrap();
        }
        state.activate_profile(Some(OTHER_OWNER));
        state
            .apply(
                "create-community",
                &form(&[("slug", OTHER), ("name", "Other workshop")]),
                &file.0,
            )
            .unwrap();
        assert!(!fs::read_to_string(&file.0)
            .unwrap()
            .contains("community-rule\t"));
        let mut state = State::load(&file.0).unwrap();
        assert!(rules(&state, ROOM).is_empty());

        let create = form(&[
            ("community", ROOM),
            ("title", "Cite sources"),
            (
                "description",
                "Start with a source.\nThen explain how it applies.",
            ),
        ]);
        for handle in ["rule_member", REVIEWER, OTHER_OWNER] {
            state.activate_profile(Some(handle));
            assert!(state
                .apply("save-community-rule", &create, &file.0)
                .is_err());
        }
        state.activate_profile(Some(MODERATOR));
        state
            .apply("save-community-rule", &create, &file.0)
            .unwrap();
        for title in ["Stay on topic", "Use clear titles"] {
            state
                .apply(
                    "save-community-rule",
                    &form(&[("community", ROOM), ("title", title)]),
                    &file.0,
                )
                .unwrap();
        }
        assert!(state
            .apply(
                "save-community-rule",
                &form(&[("community", ROOM), ("title", "  CITE SOURCES ")]),
                &file.0,
            )
            .is_err());
        assert!(state
            .apply(
                "save-community-rule",
                &form(&[("community", ROOM), ("title", " 	 ")]),
                &file.0,
            )
            .is_err());
        let [cite, topic, titles] = rules(&state, ROOM)
            .iter()
            .map(|rule| rule.id)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let cite_field = cite.to_string();
        let topic_field = topic.to_string();
        let move_up = form(&[
            ("community", ROOM),
            ("id", &topic_field),
            ("direction", "up"),
            ("neighbor", &cite_field),
        ]);
        state
            .apply("move-community-rule", &move_up, &file.0)
            .unwrap();
        assert_eq!(
            state.apply("move-community-rule", &move_up, &file.0),
            Err("Community rules changed. Refresh the page and try again.".into())
        );
        let edit = form(&[
            ("community", ROOM),
            ("id", &cite_field),
            ("title", "Cite primary sources"),
            (
                "description",
                "Name the source.\nUse a <link> when available.",
            ),
        ]);
        state.apply("save-community-rule", &edit, &file.0).unwrap();
        let count = state.mod_log.len();
        state.apply("save-community-rule", &edit, &file.0).unwrap();
        assert_eq!(state.mod_log.len(), count);

        state.activate_profile(Some(OTHER_OWNER));
        assert!(state
            .apply("move-community-rule", &move_up, &file.0)
            .is_err());
        let mut cross_edit = edit.clone();
        cross_edit.insert("community".into(), OTHER.into());
        assert!(state
            .apply("save-community-rule", &cross_edit, &file.0)
            .is_err());
        state
            .apply(
                "save-community-rule",
                &form(&[("community", OTHER), ("title", "Cite primary sources")]),
                &file.0,
            )
            .unwrap();
        let other_id = rules(&state, OTHER)[0].id;

        let mut state = State::load(&file.0).unwrap();
        assert_eq!(
            rules(&state, ROOM)
                .iter()
                .map(|rule| rule.id)
                .collect::<Vec<_>>(),
            [topic, cite, titles]
        );
        assert_eq!(rules(&state, ROOM)[1].title, "Cite primary sources");
        assert_eq!(
            rules(&state, ROOM)[1].description,
            "Name the source.\nUse a <link> when available."
        );
        assert_eq!(rules(&state, OTHER)[0].id, other_id);
        assert!(other_id > titles);
        assert!(state.mod_log.iter().any(|event| {
            event.community == ROOM
                && event.action == format!("Community rule #{topic} moved up by u/{MODERATOR}")
        }));

        state.activate_profile(Some(MODERATOR));
        state
            .apply(
                "move-community-rule",
                &form(&[
                    ("community", ROOM),
                    ("id", &topic_field),
                    ("direction", "down"),
                    ("neighbor", &cite_field),
                ]),
                &file.0,
            )
            .unwrap();
        assert_eq!(rules(&state, ROOM)[0].id, cite);
        state
            .apply(
                "save-community-rule",
                &form(&[("community", ROOM), ("title", "Explain your edit")]),
                &file.0,
            )
            .unwrap();
        assert!(rules(&state, ROOM).last().unwrap().id > other_id);
    }
}

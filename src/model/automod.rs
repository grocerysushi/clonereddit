use std::collections::BTreeMap;
use std::io::{self, Write};
use std::iter::once_with;

use super::{
    bounded, canonical_u64, decode, encode, next_id, number, required, unix_seconds, value,
    ContentKind, ModPermission, State,
};

#[path = "automod_domains.rs"]
mod domains;

pub(super) const AUTOMOD_REMOVED_POST: &str =
    "AutoMod removed your post. Check Notifications for the reason.";
pub(super) const AUTOMOD_REMOVED_COMMENT: &str =
    "AutoMod removed your comment. Check Notifications for the reason.";
const NAME_LIMIT: usize = 80;
const PHRASE_LIMIT: usize = 240;
const REASON_LIMIT: usize = 1_000;
const AGE_LIMIT_DAYS: u32 = 36_500;
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoModScope {
    Both,
    Post,
    Comment,
}

impl AutoModScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Post => "post",
            Self::Comment => "comment",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Both => "Posts and comments",
            Self::Post => "Posts",
            Self::Comment => "Comments",
        }
    }

    fn parse(scope: &str) -> Option<Self> {
        match scope {
            "both" => Some(Self::Both),
            "post" => Some(Self::Post),
            "comment" => Some(Self::Comment),
            _ => None,
        }
    }

    fn applies_to(self, kind: ContentKind) -> bool {
        matches!(
            (self, kind),
            (Self::Both, _)
                | (Self::Post, ContentKind::Post)
                | (Self::Comment, ContentKind::Comment)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoModAction {
    Flag,
    Remove,
}

impl AutoModAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Remove => "remove",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Flag => "Flag for review",
            Self::Remove => "Remove",
        }
    }

    fn parse(action: &str) -> Option<Self> {
        match action {
            "flag" => Some(Self::Flag),
            "remove" => Some(Self::Remove),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AutoModMatchMode {
    #[default]
    All,
    Any,
}

impl AutoModMatchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All conditions",
            Self::Any => "Any condition",
        }
    }

    fn parse(mode: &str) -> Option<Self> {
        match mode {
            "all" => Some(Self::All),
            "any" => Some(Self::Any),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct AutoModRule {
    pub id: u64,
    pub community: String,
    pub name: String,
    pub scope: AutoModScope,
    pub match_mode: AutoModMatchMode,
    pub phrase: String,
    pub account_age_under_days: Option<u32>,
    pub domains: Vec<String>,
    pub action: AutoModAction,
    pub reason: String,
    pub enabled: bool,
}

#[derive(Clone)]
pub struct AutoModFlag {
    pub id: u64,
    pub community: String,
    pub kind: ContentKind,
    pub content_id: u64,
    pub post_id: u64,
    pub rule_name: String,
    pub reason: String,
    pub created_at: u64,
}

struct AutoModContent<'a> {
    author: &'a str,
    title: &'a str,
    body: &'a str,
    url: &'a str,
    markdown: bool,
}

impl State {
    pub(super) fn save_automod_rule(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let community_index = self.moderated_community_index(form, ModPermission::Content)?;
        let community = &self.communities[community_index].slug;
        let existing = if value(form, "id").is_empty() {
            None
        } else {
            Some(self.automod_rule_index(form, community)?)
        };
        let name = required(form, "name", "Rule name", NAME_LIMIT)?;
        let scope = AutoModScope::parse(value(form, "scope")).ok_or("Choose a content scope.")?;
        let phrase = bounded(form, "phrase", "Trigger phrase", PHRASE_LIMIT)?;
        let previous = existing.map(|index| &self.automod_rules[index]);
        let match_mode = match form.get("match_mode") {
            Some(mode) => {
                AutoModMatchMode::parse(mode).ok_or("Choose how conditions must match.")?
            }
            None => previous.map(|rule| rule.match_mode).unwrap_or_default(),
        };
        let account_age_under_days = match form.get("account_age_under_days") {
            Some(days) => parse_account_age(days)?,
            None => previous.and_then(|rule| rule.account_age_under_days),
        };
        let domains = match form.get("domains") {
            Some(list) => domains::parse_list(list)?,
            None => previous.map_or_else(Vec::new, |rule| rule.domains.clone()),
        };
        if phrase.is_empty() && account_age_under_days.is_none() && domains.is_empty() {
            return Err("Enter at least one condition for this rule.".into());
        }
        let action =
            AutoModAction::parse(value(form, "rule_action")).ok_or("Choose a rule action.")?;
        let reason = required(form, "reason", "Reason", REASON_LIMIT)?;
        let enabled = match value(form, "enabled") {
            "on" => true,
            "" => false,
            _ => return Err("Choose whether to enable this rule.".into()),
        };
        let id = match existing {
            Some(index) => self.automod_rules[index].id,
            None => next_id(&mut self.next_automod_rule)?,
        };
        let rule = AutoModRule {
            id,
            community: community.clone(),
            name: name.into(),
            scope,
            match_mode,
            phrase: phrase.into(),
            account_age_under_days,
            domains,
            action,
            reason: reason.into(),
            enabled,
        };
        let log = format!("AutoMod rule #{id} saved by u/{}", self.username);
        let community = rule.community.clone();
        if let Some(index) = existing {
            self.automod_rules[index] = rule;
        } else {
            self.automod_rules.push(rule);
        }
        self.record_moderation_action(&community, &log)?;
        Ok("AutoMod rule saved.")
    }

    fn automod_rule_index(
        &self,
        form: &BTreeMap<String, String>,
        community: &str,
    ) -> Result<usize, String> {
        let id = canonical_u64(value(form, "id")).ok_or("Choose a valid AutoMod rule.")?;
        self.automod_rules
            .iter()
            .position(|rule| rule.id == id && rule.community == community)
            .ok_or_else(|| "This AutoMod rule is unavailable in this community.".into())
    }

    pub(super) fn delete_automod_rule(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let community_index = self.moderated_community_index(form, ModPermission::Content)?;
        let index = self.automod_rule_index(form, &self.communities[community_index].slug)?;
        let rule = self.automod_rules.remove(index);
        let log = format!("AutoMod rule #{} deleted by u/{}", rule.id, self.username);
        self.record_moderation_action(&rule.community, &log)?;
        Ok("AutoMod rule deleted.")
    }

    pub(super) fn dismiss_automod_flag(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let community = &self.communities[index].slug;
        let id = canonical_u64(value(form, "id")).ok_or("Choose a valid AutoMod flag.")?;
        let index = self
            .automod_flags
            .iter()
            .position(|flag| flag.id == id && flag.community == *community)
            .ok_or("This AutoMod flag is unavailable in this community.")?;
        let flag = self.automod_flags.remove(index);
        let log = format!(
            "AutoMod flag for {} #{} dismissed by u/{}",
            flag.kind.as_str(),
            flag.content_id,
            self.username
        );
        self.record_moderation_action(&flag.community, &log)?;
        Ok("AutoMod flag dismissed.")
    }

    pub(super) fn clear_automod_flag(&mut self, kind: ContentKind, content_id: u64) {
        self.automod_flags
            .retain(|flag| flag.kind != kind || flag.content_id != content_id);
    }

    pub(super) fn screen_post(&mut self, id: u64) -> Result<bool, String> {
        self.screen_post_at(id, unix_seconds())
    }

    pub(super) fn screen_post_at(&mut self, id: u64, now: u64) -> Result<bool, String> {
        let post = self
            .posts
            .iter()
            .find(|post| post.id == id)
            .ok_or("Post unavailable for AutoMod.")?;
        let title = if post.repost_title_from_source {
            post.repost_of
                .and_then(|source| self.repost_source(source))
                .map_or("", |source| source.title.as_str())
        } else {
            &post.title
        };
        let rule = self
            .matching_automod_rule(
                &post.community,
                ContentKind::Post,
                AutoModContent {
                    author: &post.author,
                    title,
                    body: &post.body,
                    url: &post.url,
                    markdown: true,
                },
                now,
            )
            .cloned();
        self.apply_automod_match(ContentKind::Post, id, id, rule)
    }

    pub(super) fn screen_comment(&mut self, id: u64) -> Result<bool, String> {
        let comment = self
            .comments
            .iter()
            .find(|comment| comment.id == id)
            .ok_or("Comment unavailable for AutoMod.")?;
        let post_id = comment.post;
        let community = self
            .community_for_post(post_id)
            .ok_or("Community unavailable for AutoMod.")?;
        let rule = self
            .matching_automod_rule(
                &community.slug,
                ContentKind::Comment,
                AutoModContent {
                    author: &comment.author,
                    title: "",
                    body: &comment.body,
                    url: "",
                    markdown: comment.markdown,
                },
                unix_seconds(),
            )
            .cloned();
        self.apply_automod_match(ContentKind::Comment, id, post_id, rule)
    }

    fn matching_automod_rule(
        &self,
        community: &str,
        kind: ContentKind,
        content: AutoModContent<'_>,
        now: u64,
    ) -> Option<&AutoModRule> {
        let mut lowercase = None;
        let mut account_age_seconds = None;
        let mut hosts = None;
        self.automod_rules
            .iter()
            .filter(|rule| {
                rule.enabled && rule.community == community && rule.scope.applies_to(kind)
            })
            .filter(|rule| {
                let age_matches = once_with(|| {
                    let days = rule.account_age_under_days?;
                    let age = account_age_seconds.get_or_insert_with(|| {
                        self.profile_creation_time(content.author)
                            .and_then(|created_at| now.checked_sub(created_at))
                    });
                    Some(age.is_some_and(|seconds| seconds < u64::from(days) * SECONDS_PER_DAY))
                });
                let phrase_matches = once_with(|| {
                    (!rule.phrase.is_empty()).then(|| {
                        let phrase = rule.phrase.to_lowercase();
                        let fields = lowercase.get_or_insert_with(|| {
                            [content.title, content.body, content.url].map(str::to_lowercase)
                        });
                        fields.iter().any(|field| field.contains(&phrase))
                    })
                });
                let domain_matches = once_with(|| {
                    (!rule.domains.is_empty()).then(|| {
                        hosts
                            .get_or_insert_with(|| {
                                domains::linked_hosts(
                                    content.url,
                                    content.title,
                                    content.body,
                                    content.markdown,
                                )
                            })
                            .iter()
                            .any(|host| {
                                rule.domains
                                    .iter()
                                    .any(|domain| domains::matches(host, domain))
                            })
                    })
                });
                let mut matches = age_matches
                    .chain(phrase_matches)
                    .chain(domain_matches)
                    .flatten();
                match rule.match_mode {
                    AutoModMatchMode::All => matches.all(|matched| matched),
                    AutoModMatchMode::Any => matches.any(|matched| matched),
                }
            })
            .min_by_key(|rule| (rule.action != AutoModAction::Remove, rule.id))
    }

    fn apply_automod_match(
        &mut self,
        kind: ContentKind,
        content_id: u64,
        post_id: u64,
        rule: Option<AutoModRule>,
    ) -> Result<bool, String> {
        let Some(rule) = rule else { return Ok(false) };
        if rule.action == AutoModAction::Remove {
            self.remove_content_automatically(
                kind,
                content_id,
                &rule.community,
                &rule.name,
                &rule.reason,
            )?;
            return Ok(true);
        }
        if !self
            .automod_flags
            .iter()
            .any(|flag| flag.kind == kind && flag.content_id == content_id)
        {
            let id = next_id(&mut self.next_automod_flag)?;
            self.automod_flags.push(AutoModFlag {
                id,
                community: rule.community,
                kind,
                content_id,
                post_id,
                rule_name: rule.name,
                reason: rule.reason,
                created_at: unix_seconds(),
            });
        }
        Ok(false)
    }

    pub(super) fn read_automod_record(&mut self, fields: &[&str]) -> Option<()> {
        match fields {
            ["automod-sequence", next_rule, next_flag] => {
                self.next_automod_rule = number(next_rule)?;
                self.next_automod_flag = number(next_flag)?;
            }
            ["automod-rule", id, community, name, scope, phrase, action, reason, enabled, optional @ ..] =>
            {
                let id = number(id)?;
                let community = decode(community)?;
                let name = decode(name)?;
                let phrase = decode(phrase)?;
                let reason = decode(reason)?;
                let (days, domains, match_mode) = match optional {
                    [] => ("", "", AutoModMatchMode::All),
                    ["conditions", days, domains] => (*days, *domains, AutoModMatchMode::All),
                    ["conditions", days, domains, mode] => {
                        (*days, *domains, AutoModMatchMode::parse(mode)?)
                    }
                    _ => return None,
                };
                let account_age_under_days = parse_account_age(days).ok()?;
                let domains = domains::parse_list(&decode(domains)?).ok()?;
                if self.automod_rules.iter().any(|rule| rule.id == id)
                    || !self
                        .communities
                        .iter()
                        .any(|item| item.slug == community && item.owner.is_some())
                    || !valid_text(&name, NAME_LIMIT)
                    || (!phrase.is_empty() && !valid_text(&phrase, PHRASE_LIMIT))
                    || (phrase.is_empty() && account_age_under_days.is_none() && domains.is_empty())
                    || !valid_text(&reason, REASON_LIMIT)
                {
                    return None;
                }
                self.automod_rules.push(AutoModRule {
                    id,
                    community,
                    name,
                    scope: AutoModScope::parse(scope)?,
                    match_mode,
                    phrase,
                    account_age_under_days,
                    domains,
                    action: AutoModAction::parse(action)?,
                    reason,
                    enabled: number(enabled)?,
                });
            }
            ["automod-flag", id, community, kind, content_id, name, reason, created_at] => {
                let id = number(id)?;
                let community = decode(community)?;
                let kind = ContentKind::parse(kind)?;
                let content_id = number(content_id)?;
                let rule_name = decode(name)?;
                let reason = decode(reason)?;
                let (post_id, _) = self.content_moderation_target(kind, content_id, &community)?;
                if self
                    .automod_flags
                    .iter()
                    .any(|flag| flag.id == id || flag.kind == kind && flag.content_id == content_id)
                    || self.content_removal(kind, content_id).is_some()
                    || !valid_text(&rule_name, NAME_LIMIT)
                    || !valid_text(&reason, REASON_LIMIT)
                {
                    return None;
                }
                self.automod_flags.push(AutoModFlag {
                    id,
                    community,
                    kind,
                    content_id,
                    post_id,
                    rule_name,
                    reason,
                    created_at: number(created_at)?,
                });
            }
            _ => return None,
        }
        Some(())
    }

    pub(super) fn write_automod_records(&self, file: &mut impl Write) -> io::Result<()> {
        writeln!(
            file,
            "automod-sequence\t{}\t{}",
            self.next_automod_rule, self.next_automod_flag
        )?;
        for rule in &self.automod_rules {
            write!(
                file,
                "automod-rule\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                rule.id,
                encode(&rule.community),
                encode(&rule.name),
                rule.scope.as_str(),
                encode(&rule.phrase),
                rule.action.as_str(),
                encode(&rule.reason),
                rule.enabled,
            )?;
            if rule.account_age_under_days.is_some()
                || !rule.domains.is_empty()
                || rule.match_mode == AutoModMatchMode::Any
            {
                let days = rule
                    .account_age_under_days
                    .map(|days| days.to_string())
                    .unwrap_or_default();
                write!(
                    file,
                    "\tconditions\t{days}\t{}",
                    encode(&rule.domains.join(","))
                )?;
                if rule.match_mode == AutoModMatchMode::Any {
                    write!(file, "\t{}", rule.match_mode.as_str())?;
                }
            }
            writeln!(file)?;
        }
        for flag in &self.automod_flags {
            writeln!(
                file,
                "automod-flag\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                flag.id,
                encode(&flag.community),
                flag.kind.as_str(),
                flag.content_id,
                encode(&flag.rule_name),
                encode(&flag.reason),
                flag.created_at,
            )?;
        }
        Ok(())
    }
}

fn parse_account_age(input: &str) -> Result<Option<u32>, String> {
    let days = input.trim();
    if days.is_empty() {
        return Ok(None);
    }
    canonical_u64(days)
        .and_then(|days| u32::try_from(days).ok())
        .filter(|days| (1..=AGE_LIMIT_DAYS).contains(days))
        .map(Some)
        .ok_or_else(|| format!("Enter a whole number from 1 to {AGE_LIMIT_DAYS} for account age."))
}

fn valid_text(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= limit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PostMedia;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|&(key, value)| (key.into(), value.into()))
            .collect()
    }

    fn rule(
        name: &str,
        scope: &str,
        phrase: &str,
        action: &str,
        reason: &str,
    ) -> BTreeMap<String, String> {
        form(&[
            ("community", "automod-test"),
            ("name", name),
            ("scope", scope),
            ("phrase", phrase),
            ("rule_action", action),
            ("reason", reason),
            ("enabled", "on"),
        ])
    }

    fn community_fixture() -> State {
        let mut state = State::seeded();
        state.activate_profile(Some("test_owner"));
        for slug in ["automod-test", "separate-test"] {
            state
                .create_community(&form(&[("slug", slug), ("name", slug)]))
                .unwrap();
        }
        for (handle, role) in [
            ("test_mod", "moderator"),
            ("test_reviewer", "appeals-reviewer"),
        ] {
            state
                .set_moderator_role(&form(&[
                    ("community", "automod-test"),
                    ("handle", handle),
                    ("role", role),
                ]))
                .unwrap();
        }
        state
    }

    struct TempState(PathBuf);

    impl TempState {
        fn new() -> Self {
            let filename = format!(
                "commonline-automod-{}-{}.state",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            );
            Self(std::env::temp_dir().join(filename))
        }
    }

    impl Drop for TempState {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn compound_rules_match_all_or_any_configured_conditions() {
        let mut state = community_fixture();
        let mut compound = rule("Compound flag", "both", "launch", "flag", "Check the offer");
        compound.insert("account_age_under_days".into(), "7".into());
        compound.insert("domains".into(), "Promo.Example, second.example".into());
        state.save_automod_rule(&compound).unwrap();
        state.activate_profile(Some("compound_author"));
        let now = 1_700_000_000;
        let boundary = now - 7 * SECONDS_PER_DAY;
        state.profile_created = boundary + 1;
        let matching = |state: &State, author, title, body, url, markdown| {
            state
                .matching_automod_rule(
                    "automod-test",
                    ContentKind::Post,
                    AutoModContent {
                        author,
                        title,
                        body,
                        url,
                        markdown,
                    },
                    now,
                )
                .is_some()
        };
        let matches_author = |state: &State, title, body, url, markdown| {
            matching(state, "compound_author", title, body, url, markdown)
        };
        assert!(matches_author(
            &state,
            "LAUNCH",
            "",
            "https://shop.promo.example/cart",
            true
        ));
        assert!(matches_author(
            &state,
            "",
            "[Launch](https://second.example/)",
            "",
            true
        ));
        assert!(!matches_author(
            &state,
            "Offer",
            "",
            "https://promo.example/",
            true
        ));
        assert!(!matches_author(
            &state,
            "Launch",
            "",
            "https://promo.example.other.test/",
            true
        ));
        assert!(!matches_author(
            &state,
            "Launch",
            "",
            "https://other.test/promo.example/",
            true
        ));
        assert!(!matches_author(
            &state,
            "Launch",
            "[offer](https://promo.example/)",
            "",
            false
        ));
        assert!(!matching(
            &state,
            "unknown_author",
            "Launch",
            "",
            "https://promo.example/",
            true
        ));
        for invalid_age in [0, now + 1, boundary] {
            state.profile_created = invalid_age;
            assert!(!matches_author(
                &state,
                "Launch",
                "",
                "https://promo.example/",
                true
            ));
        }

        state.automod_rules[0].match_mode = AutoModMatchMode::Any;
        assert!(!matches_author(&state, "Offer", "", "", true));
        assert!(matches_author(&state, "Launch", "", "", true));
        assert!(matches_author(
            &state,
            "Offer",
            "",
            "https://shop.second.example/",
            true
        ));
        state.profile_created = boundary + 1;
        assert!(matches_author(&state, "Offer", "", "", true));
    }

    #[test]
    fn match_mode_survives_reload_and_omitted_edits() {
        let fixture = TempState::new();
        let mut state = community_fixture();
        let mut settings = rule("Review offers", "both", "offer", "flag", "Check the offer");
        settings.insert("match_mode".into(), "any".into());
        state
            .apply("save-automod-rule", &settings, &fixture.0)
            .unwrap();
        let mut state = State::load(&fixture.0).unwrap();
        assert_eq!(state.automod_rules[0].match_mode, AutoModMatchMode::Any);

        settings.insert("id".into(), state.automod_rules[0].id.to_string());
        for invalid in ["", "either"] {
            settings.insert("match_mode".into(), invalid.into());
            assert!(state
                .apply("save-automod-rule", &settings, &fixture.0)
                .is_err());
        }
        settings.remove("match_mode");
        state
            .apply("save-automod-rule", &settings, &fixture.0)
            .unwrap();
        assert_eq!(state.automod_rules[0].match_mode, AutoModMatchMode::Any);

        settings.insert("match_mode".into(), "all".into());
        state
            .apply("save-automod-rule", &settings, &fixture.0)
            .unwrap();
        let state = State::load(&fixture.0).unwrap();
        assert_eq!(state.automod_rules[0].match_mode, AutoModMatchMode::All);
    }

    #[test]
    fn compound_settings_round_trip_and_mode_only_edit_rescreens_comment() {
        let fixture = TempState::new();
        let mut state = community_fixture();
        let legacy = rule(
            "Legacy phrase",
            "post",
            "older phrase",
            "flag",
            "Original reason",
        );
        state
            .apply("save-automod-rule", &legacy, &fixture.0)
            .unwrap();
        let mut compound = rule(
            "Check review links",
            "comment",
            "",
            "remove",
            "Link review needed",
        );
        for (field, invalid) in [
            ("account_age_under_days", "0"),
            ("account_age_under_days", "36501"),
            ("account_age_under_days", "1.5"),
            ("domains", "https://review.example/"),
        ] {
            compound.insert(field.into(), invalid.into());
            assert!(state
                .apply("save-automod-rule", &compound, &fixture.0)
                .is_err());
            compound.remove(field);
        }
        assert!(state
            .apply("save-automod-rule", &compound, &fixture.0)
            .is_err());
        compound.insert("account_age_under_days".into(), "7".into());
        compound.insert("domains".into(), "Review.Example, review.example".into());
        state
            .apply("save-automod-rule", &compound, &fixture.0)
            .unwrap();
        let mut state = State::load(&fixture.0).unwrap();
        assert_eq!(state.automod_rules[0].match_mode, AutoModMatchMode::All);
        assert_eq!(state.automod_rules[1].match_mode, AutoModMatchMode::All);
        assert_eq!(state.automod_rules[0].phrase, "older phrase");
        assert!(state.automod_rules[0].domains.is_empty());
        assert_eq!(state.automod_rules[0].account_age_under_days, None);
        assert!(state.automod_rules[1].phrase.is_empty());
        assert_eq!(state.automod_rules[1].domains, ["review.example"]);
        assert_eq!(state.automod_rules[1].account_age_under_days, Some(7));

        // A legacy editor omitting the new fields must not silently weaken a rule.
        let id = state.automod_rules[1].id.to_string();
        compound.insert("id".into(), id);
        compound.remove("account_age_under_days");
        compound.remove("domains");
        compound.insert("name".into(), "Check links on edit".into());
        state
            .apply("save-automod-rule", &compound, &fixture.0)
            .unwrap();
        assert_eq!(state.automod_rules[1].domains, ["review.example"]);
        assert_eq!(state.automod_rules[1].account_age_under_days, Some(7));

        state
            .create_post(&form(&[
                ("community", "automod-test"),
                ("title", "Comment here"),
            ]))
            .unwrap();
        let post = state.posts.last().unwrap().id.to_string();
        state.activate_profile(Some("new_poster"));
        let body = "[offer](https://review.example/item)";
        assert_eq!(
            state
                .apply(
                    "comment",
                    &form(&[("post", &post), ("body", body)]),
                    &fixture.0
                )
                .unwrap(),
            "Comment posted."
        );
        let comment = state.comments.last().unwrap().id;
        assert_eq!(
            state
                .apply(
                    "edit-comment",
                    &form(&[
                        ("id", &comment.to_string()),
                        ("body", body),
                        ("body_format", "markdown")
                    ]),
                    &fixture.0,
                )
                .unwrap(),
            AUTOMOD_REMOVED_COMMENT
        );
        assert!(state.is_comment_removed(comment));
        assert!(state.notifications.iter().any(|notice| {
            notice.recipient == "new_poster" && notice.body.contains("Link review needed")
        }));
    }

    #[test]
    fn scheduled_post_age_uses_the_author_and_publication_time() {
        let fixture = TempState::new();
        let mut state = community_fixture();
        let mut compound = rule(
            "New poster link",
            "post",
            "schedule",
            "remove",
            "Review new posts",
        );
        compound.insert("account_age_under_days".into(), "7".into());
        compound.insert("domains".into(), "review.example".into());
        state.save_automod_rule(&compound).unwrap();
        let boundary = unix_seconds() + 3_600;
        state.profile_created = boundary - 30 * SECONDS_PER_DAY;
        state.activate_profile(Some("test_mod"));
        state.profile_created = boundary - 7 * SECONDS_PER_DAY;
        for publish_at in [boundary - 1, boundary] {
            state
                .schedule_item(&form(&[
                    ("community", "automod-test"),
                    ("kind", "post"),
                    ("title", "Schedule link"),
                    ("url", "https://review.example/post"),
                    ("publish_at", &publish_at.to_string()),
                ]))
                .unwrap();
        }
        state.activate_profile(Some("test_owner"));
        assert_eq!(state.publish_due(&fixture.0, boundary - 1).unwrap(), 1);
        let first = state.posts.last().unwrap();
        assert_eq!(first.author, "test_mod");
        assert!(state.is_post_removed(first.id));
        assert_eq!(state.publish_due(&fixture.0, boundary).unwrap(), 1);
        let second = state.posts.last().unwrap();
        assert_eq!(second.author, "test_mod");
        assert!(!state.is_post_removed(second.id));
    }

    #[test]
    fn automation_is_scoped_and_flag_snapshots_survive_rule_changes_and_restart() {
        let fixture = TempState::new();
        let mut state = community_fixture();
        let flag_rule = rule(
            "Initial flag",
            "post",
            "Étiquette +",
            "flag",
            "Original report to mods",
        );
        state
            .create_post(&form(&[
                ("community", "automod-test"),
                ("title", "Old étiquette +"),
            ]))
            .unwrap();
        let grandfathered = state.posts.last().unwrap().id.to_string();
        state.activate_profile(Some("test_reviewer"));
        assert!(state
            .apply("save-automod-rule", &flag_rule, &fixture.0)
            .is_err());
        state.activate_profile(Some("test_mod"));
        state
            .apply("save-automod-rule", &flag_rule, &fixture.0)
            .unwrap();
        let rule_id = state.automod_rules[0].id.to_string();
        let mut disabled = rule(
            "Disabled remove",
            "both",
            "étiquette +",
            "remove",
            "Never applied",
        );
        disabled.remove("enabled");
        state
            .apply("save-automod-rule", &disabled, &fixture.0)
            .unwrap();
        state.activate_profile(Some("test_owner"));
        assert!(state
            .edit_post(&form(&[
                ("id", &grandfathered),
                ("title", "Old étiquette +")
            ]))
            .is_ok());
        state
            .create_post(&form(&[
                ("community", "separate-test"),
                ("title", "Old étiquette +"),
            ]))
            .unwrap();
        state
            .create_post(&form(&[
                ("community", "automod-test"),
                ("title", "EtIQUETTE without literal plus"),
            ]))
            .unwrap();
        assert!(state.automod_flags.is_empty());
        state
            .apply(
                "create",
                &form(&[("community", "automod-test"), ("title", "New éTIQUETTE +")]),
                &fixture.0,
            )
            .unwrap();
        let flagged_post = state.posts.last().unwrap().id;
        let flag_id = state.automod_flags[0].id.to_string();
        assert!(!state.is_post_removed(flagged_post));
        let mut updated = flag_rule;
        updated.insert("id".into(), rule_id.clone());
        updated.insert("reason".into(), "Changed rule reason".into());
        state
            .apply("save-automod-rule", &updated, &fixture.0)
            .unwrap();
        let wrong_scope = form(&[("community", "separate-test"), ("id", &rule_id)]);
        assert!(state
            .apply("delete-automod-rule", &wrong_scope, &fixture.0)
            .is_err());
        state
            .apply(
                "delete-automod-rule",
                &form(&[("community", "automod-test"), ("id", &rule_id)]),
                &fixture.0,
            )
            .unwrap();

        let mut state = State::load(&fixture.0).unwrap();
        assert_eq!(state.automod_rules.len(), 1);
        assert!(!state.automod_rules[0].enabled);
        assert_eq!(state.automod_flags[0].reason, "Original report to mods");
        assert_eq!(state.automod_flags[0].rule_name, "Initial flag");
        let dismiss = form(&[("community", "automod-test"), ("id", &flag_id)]);
        state.activate_profile(Some("test_reviewer"));
        assert!(state
            .apply("dismiss-automod-flag", &dismiss, &fixture.0)
            .is_err());
        state.activate_profile(Some("test_mod"));
        state
            .apply("dismiss-automod-flag", &dismiss, &fixture.0)
            .unwrap();
        assert!(state.automod_flags.is_empty());
        assert!(!state.is_post_removed(flagged_post));
        assert!(state.mod_log.last().unwrap().action.contains("dismissed"));
    }

    #[test]
    fn removal_precedes_flags_and_author_notices_use_a_system_actor() {
        let fixture = TempState::new();
        let mut state = community_fixture();
        state
            .create_post(&form(&[
                ("community", "automod-test"),
                ("title", "Comment on this"),
            ]))
            .unwrap();
        let parent_id = state.posts.last().unwrap().id.to_string();
        state
            .save_automod_rule(&rule("First flag", "both", "match me", "flag", "Escalate"))
            .unwrap();
        state
            .save_automod_rule(&rule(
                "First removal",
                "both",
                "match me",
                "remove",
                "First reason\nfor removal",
            ))
            .unwrap();
        state
            .save_automod_rule(&rule(
                "Second removal",
                "both",
                "match me",
                "remove",
                "Later reason",
            ))
            .unwrap();
        state.activate_profile(Some("comment_author"));
        let comment = form(&[("post", &parent_id), ("body", "MATCH ME")]);
        assert_eq!(
            state.apply("comment", &comment, &fixture.0).unwrap(),
            AUTOMOD_REMOVED_COMMENT
        );
        let comment_id = state.comments.last().unwrap().id;
        assert!(state.is_comment_removed(comment_id));
        assert!(state.automod_flags.is_empty());
        let notices = &state.notifications;
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].recipient, "comment_author");
        assert!(notices[0].body.ends_with("First reason\nfor removal"));
        let mut state = State::load(&fixture.0).unwrap();
        let removal = state
            .content_removal(ContentKind::Comment, comment_id)
            .unwrap();
        assert!(removal.automated);
        assert!(removal.moderator.is_empty());
        assert!(state
            .mod_log
            .last()
            .unwrap()
            .action
            .contains("AutoMod (First removal)"));
        let target = form(&[
            ("community", "automod-test"),
            ("kind", "comment"),
            ("id", &comment_id.to_string()),
        ]);
        state.activate_profile(Some("test_mod"));
        state.apply("restore-content", &target, &fixture.0).unwrap();
        assert!(!state.is_comment_removed(comment_id));
        state.activate_profile(Some("comment_author"));
        let edit = form(&[
            ("id", &comment_id.to_string()),
            ("body", "Changed text that will match me again"),
        ]);
        assert_eq!(
            state.apply("edit-comment", &edit, &fixture.0).unwrap(),
            AUTOMOD_REMOVED_COMMENT
        );
        assert!(state.is_comment_removed(comment_id));
    }

    #[test]
    fn post_variants_evaluate_before_becoming_visible_and_scheduled_posts_wait_until_publication() {
        let fixture = TempState::new();
        let mut state = community_fixture();
        state
            .create_post(&form(&[
                ("community", "separate-test"),
                ("title", "Unwanted title"),
            ]))
            .unwrap();
        let source = state.posts.last().unwrap().id.to_string();
        let publish_at = (unix_seconds() + 3_600).to_string();
        state
            .schedule_item(&form(&[
                ("community", "automod-test"),
                ("kind", "post"),
                ("title", "Unwanted scheduled"),
                ("publish_at", &publish_at),
            ]))
            .unwrap();
        state
            .save_automod_rule(&rule(
                "Screen posts",
                "post",
                "unwanted",
                "remove",
                "Rule reason",
            ))
            .unwrap();
        assert!(state.content_removals.is_empty());
        state.activate_profile(Some("post_author"));
        let media = vec![PostMedia {
            key: "a".repeat(48),
            content_type: "image/png".into(),
        }];
        assert_eq!(
            state
                .create_media_post(
                    &form(&[
                        ("community", "automod-test"),
                        ("title", "Media"),
                        ("body", "unwanted body")
                    ]),
                    media,
                    &fixture.0
                )
                .unwrap(),
            AUTOMOD_REMOVED_POST
        );
        assert_eq!(state.posts.last().unwrap().media.len(), 1);
        assert_eq!(
            state
                .create_poll(&form(&[
                    ("community", "automod-test"),
                    ("title", "Unwanted poll"),
                    ("option_1", "A"),
                    ("option_2", "B"),
                    ("duration", "1")
                ]))
                .unwrap(),
            AUTOMOD_REMOVED_POST
        );
        assert_eq!(
            state
                .create_repost(&form(&[("community", "automod-test"), ("source", &source)]))
                .unwrap(),
            AUTOMOD_REMOVED_POST
        );
        state
            .create_post(&form(&[
                ("community", "automod-test"),
                ("title", "Initially clean"),
            ]))
            .unwrap();
        let id = state.posts.last().unwrap().id.to_string();
        assert_eq!(
            state
                .edit_post(&form(&[
                    ("id", &id),
                    ("title", "Initially clean"),
                    ("url", "https://example.invalid/unwanted")
                ]))
                .unwrap(),
            AUTOMOD_REMOVED_POST
        );
        state.activate_profile(None);
        assert_eq!(
            state
                .publish_due(&fixture.0, publish_at.parse().unwrap())
                .unwrap(),
            1
        );
        let published = state.scheduled[0].post_id.unwrap();
        assert!(state.is_post_removed(published));
        assert!(state
            .notifications
            .iter()
            .any(|item| item.recipient == "test_owner"
                && item.moderation
                && item.body.contains(&format!("post #{published}"))));
        assert_eq!(
            state
                .content_removals
                .iter()
                .filter(|item| item.automated)
                .count(),
            5
        );
    }
}

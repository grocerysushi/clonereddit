use std::collections::BTreeMap;
use std::fmt::Write;

use crate::model::{
    AutoModAction, AutoModFlag, AutoModRule, AutoModScope, Community, ModPermission, State,
};
use crate::schedule_time;

use super::super::{action_fields, action_form, display_post_title, escape, field};
use super::{empty, moderate_content, tools_url, QueueFilters};

pub(super) fn rules(
    state: &State,
    item: &Community,
    edit: &str,
    draft: Option<(&BTreeMap<String, String>, &str)>,
) -> String {
    if !state.can_moderate(item, ModPermission::Content) {
        return empty(
            "AutoMod unavailable",
            "Your account needs content moderation access.",
            "shield",
        );
    }
    let base = tools_url(item, "automod");
    let flags_url = escape(&format!("{}&status=automod", tools_url(item, "queue")));
    let mut rules: Vec<_> = state
        .automod_rules
        .iter()
        .filter(|rule| rule.community == item.slug)
        .collect();
    rules.sort_by_key(|rule| rule.id);
    let selected = edit
        .parse::<u64>()
        .ok()
        .and_then(|id| rules.iter().copied().find(|rule| rule.id == id));
    let mut html = draft.map_or_else(String::new, |(_, error)| error_notice(error));
    if !edit.is_empty() && selected.is_none() && draft.is_none() {
        html.push_str(&error_notice("That rule is unavailable in this community."));
    }

    write!(html, "<div class='mod-automod-intro'><p>AutoMod checks posts and comments when they’re published or edited. A rule applies when all the conditions you enter match.</p><a class='button light' href='{flags_url}'>View AutoMod flags</a></div><details class='panel mod-section mod-automod-help'><summary>How rules apply</summary><p>Flag sends content to the queue while it stays visible. Remove hides content and sends the rule reason to its author. If several rules match, removals take priority. Among matching rules with the same action, the earliest rule wins. Scheduled posts are checked when they publish.</p></details>").unwrap();
    let submitted = draft
        .filter(|(form, _)| {
            field(form, "action") == "save-automod-rule" && (edit.is_empty() || selected.is_some())
        })
        .map(|(form, _)| form);
    html.push_str(&rule_form(item, &base, selected, submitted));

    html.push_str("<section class='mod-automod-rules' aria-labelledby='automod-rules-title'><h2 id='automod-rules-title'>Rules</h2>");
    if rules.is_empty() {
        html.push_str("<p class='muted'>No rules yet.</p>");
    }
    for rule in rules {
        let id = rule.id.to_string();
        let name = escape(&rule.name);
        let enabled = if rule.enabled { "Enabled" } else { "Paused" };
        let settings = escape(&format!("{base}&edit={id}#automod-editor"));
        let delete = action_fields(
            "delete-automod-rule",
            &base,
            &[("community", &item.slug), ("id", &id)],
        );
        let conditions = summarize_conditions(rule);
        write!(html, "<article class='panel mod-record mod-automod-rule'><header><span class='flair'>{enabled}</span><span class='muted'>{} · {}</span></header><h3>{name}</h3>{conditions}<p class='muted mod-record-detail'>{}</p><div class='mod-automod-rule-actions'><a class='button light' href='{settings}' aria-label='Edit {name}'>Edit rule</a><details class='mod-automod-delete'><summary class='button light' aria-label='Delete {name}'>Delete rule</summary><form action='/action' method='post'>{delete}<button class='button dark' type='submit'>Confirm deletion</button></form></details></div></article>", rule.scope.label(), rule.action.label(), escape(&rule.reason)).unwrap();
    }
    html.push_str("</section>");
    html
}

fn summarize_conditions(rule: &AutoModRule) -> String {
    let mut html = String::new();
    if !rule.phrase.is_empty() {
        write!(
            html,
            "<p class='mod-record-detail'>Match <q>{}</q></p>",
            escape(&rule.phrase)
        )
        .unwrap();
    }
    if let Some(days) = rule.account_age_under_days {
        let unit = if days == 1 { "day" } else { "days" };
        write!(
            html,
            "<p class='mod-record-detail'>Account younger than {days} {unit}</p>"
        )
        .unwrap();
    }
    if !rule.domains.is_empty() {
        write!(
            html,
            "<p class='mod-record-detail'>Links to {}</p>",
            escape(&rule.domains.join(", "))
        )
        .unwrap();
    }
    html
}

fn rule_form(
    item: &Community,
    back: &str,
    rule: Option<&AutoModRule>,
    submitted: Option<&BTreeMap<String, String>>,
) -> String {
    let id = rule.map_or_else(String::new, |rule| rule.id.to_string());
    let (name, phrase, scope, action, reason, enabled) = if let Some(form) = submitted {
        (
            field(form, "name"),
            field(form, "phrase"),
            field(form, "scope"),
            field(form, "rule_action"),
            field(form, "reason"),
            field(form, "enabled") == "on",
        )
    } else if let Some(rule) = rule {
        (
            rule.name.as_str(),
            rule.phrase.as_str(),
            rule.scope.as_str(),
            rule.action.as_str(),
            rule.reason.as_str(),
            rule.enabled,
        )
    } else {
        (
            "",
            "",
            AutoModScope::Both.as_str(),
            AutoModAction::Flag.as_str(),
            "",
            true,
        )
    };
    let account_age = if let Some(form) = submitted {
        field(form, "account_age_under_days").to_owned()
    } else {
        rule.and_then(|rule| rule.account_age_under_days)
            .map(|days| days.to_string())
            .unwrap_or_default()
    };
    let domains = if let Some(form) = submitted {
        field(form, "domains").to_owned()
    } else {
        rule.map(|rule| rule.domains.join("\n")).unwrap_or_default()
    };
    let check = if enabled { " checked" } else { "" };
    let fields = action_fields(
        "save-automod-rule",
        back,
        &[("community", &item.slug), ("id", &id)],
    );
    let scopes: String = [
        AutoModScope::Both,
        AutoModScope::Post,
        AutoModScope::Comment,
    ]
    .into_iter()
    .map(|value| option(value.as_str(), value.label(), scope))
    .collect();
    let actions: String = [AutoModAction::Flag, AutoModAction::Remove]
        .into_iter()
        .map(|value| option(value.as_str(), value.label(), action))
        .collect();
    let (title, button, cancel) = if rule.is_some() {
        (
            "Edit rule",
            "Save rule",
            format!("<a class='button light' href='{}'>Cancel</a>", escape(back)),
        )
    } else {
        ("Add rule", "Create rule", String::new())
    };
    format!(
        r#"<section id='automod-editor' class='panel mod-section mod-automod-editor'><h2>{title}</h2>
<form class='stack-form' action='/action' method='post'>{fields}
<label>Rule name<input name='name' value='{name}' maxlength='80' required></label>
<fieldset class='mod-automod-conditions'><legend>Conditions <small>Enter at least one</small></legend>
<label>Keyword or phrase<input name='phrase' value='{phrase}' maxlength='240'></label>
<label>Account younger than (days)<input type='number' name='account_age_under_days' min='1' max='36500' step='1' value='{account_age}'></label>
<label>Link domains<textarea name='domains' rows='2' maxlength='4096' aria-describedby='automod-domain-help'>{domains}</textarea></label>
<small id='automod-domain-help' class='muted'>Enter up to 20 host names, such as example.com, separated by commas or new lines. Subdomains also match.</small>
</fieldset><div class='mod-automod-choices'><label>Content type<select name='scope'>{scopes}</select></label><label>Action<select name='rule_action'>{actions}</select></label></div>
<label>Reason<textarea name='reason' rows='3' maxlength='1000' required>{reason}</textarea></label>
<small class='muted'>Authors receive this reason when AutoMod removes their content.</small>
<label class='checkbox'><input type='checkbox' name='enabled'{check}> Enabled</label>
<div class='form-actions'><button class='button dark' type='submit'>{button}</button>{cancel}</div></form></section>"#,
        name = escape(name),
        phrase = escape(phrase),
        account_age = escape(&account_age),
        domains = escape(&domains),
        reason = escape(reason),
    )
}

pub(super) fn flags(
    state: &State,
    item: &Community,
    flags: &mut [&AutoModFlag],
    filters: QueueFilters,
    back: &str,
) -> String {
    flags.sort_by_key(|flag| (flag.created_at, flag.id));
    if filters.sort == "newest" {
        flags.reverse();
    }
    let mut html = String::new();
    for flag in flags {
        let id = flag.id.to_string();
        let kind = flag.kind;
        let url = escape(&moderate_content::detail_url(item, kind, flag.content_id));
        let when = schedule_time::display(flag.created_at, "America/Chicago");
        let timestamp = schedule_time::to_local(flag.created_at, "UTC");
        let title = state
            .posts
            .iter()
            .find(|post| post.id == flag.post_id && post.community == item.slug)
            .map_or("", |post| display_post_title(state, post));
        let dismiss = action_form(
            "dismiss-automod-flag",
            back,
            &[
                ("community", &item.slug),
                ("id", &id),
                ("status", filters.status),
                ("sort", filters.sort),
                ("kind", filters.kind.as_str()),
            ],
            "Dismiss flag",
            "",
            "button light",
        );
        write!(html, "<article class='panel mod-record mod-automod-flag' data-content-kind='{}'><header><span class='flair'>AutoMod flag</span><time class='muted' datetime='{timestamp}Z'>{}</time></header><h2>{}</h2><p class='muted'>{} #{} · {}</p><p class='mod-record-detail'>{}</p><div class='form-actions'><a class='button light' href='{url}'>Review content</a>{dismiss}</div></article>", kind.as_str(), escape(&when), escape(&flag.rule_name), kind.label(), flag.content_id, escape(title), escape(&flag.reason)).unwrap();
    }
    html
}

pub(super) fn error_notice(error: &str) -> String {
    format!(
        "<p class='notice setup-error' role='alert'>{}</p>",
        escape(error)
    )
}

fn option(value: &str, label: &str, chosen: &str) -> String {
    let selected = if value == chosen { " selected" } else { "" };
    format!("<option value='{value}'{selected}>{label}</option>")
}

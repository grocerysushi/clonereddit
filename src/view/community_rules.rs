use std::collections::BTreeMap;
use std::fmt::Write;

use crate::model::{
    Community, CommunityRule, COMMUNITY_RULE_DESCRIPTION_MAX, COMMUNITY_RULE_TITLE_MAX,
};

use super::super::{action_fields, escape, field};
use super::tools_url;

pub(super) fn render(
    item: &Community,
    edit: &str,
    delete: &str,
    rejected: Option<(&BTreeMap<String, String>, &str)>,
) -> String {
    let base = tools_url(item, "rules");
    let rules = item.rules();
    let find_rule = |value: &str| {
        value
            .parse::<u64>()
            .ok()
            .and_then(|id| rules.iter().find(|rule| rule.id == id))
    };
    let edit = if delete.is_empty() { edit } else { "" };
    let selected = find_rule(edit);
    let deleting = find_rule(delete);
    let submitted = rejected
        .filter(|(form, _)| {
            field(form, "action") == "save-community-rule"
                && (edit.is_empty() || selected.is_some())
        })
        .map(|(form, _)| form);
    let mut html = String::new();
    let error = rejected.map_or_else(
        || {
            if (!edit.is_empty() && selected.is_none())
                || (!delete.is_empty() && deleting.is_none())
            {
                "That rule is unavailable in this community."
            } else {
                ""
            }
        },
        |(_, error)| error,
    );
    if !error.is_empty() {
        write!(
            html,
            "<p class='notice setup-error' role='alert'>{}</p>",
            escape(error)
        )
        .unwrap();
    }
    if let Some(rule) = deleting {
        html.push_str(&confirm_delete(item, &base, rule));
    } else {
        html.push_str(&editor(item, &base, selected, submitted));
    }
    html.push_str("<section class='panel mod-section' aria-labelledby='community-rule-list-heading'><h2 id='community-rule-list-heading'>Reporting rules</h2><p class='muted'>People see rules in this order when reporting posts or comments.</p>");
    if rules.is_empty() {
        html.push_str("<p class='mod-rule-empty muted'>No reporting rules yet.</p></section>");
        return html;
    }

    html.push_str("<ol class='mod-community-rule-list'>");
    for (index, rule) in rules.iter().enumerate() {
        let id = rule.id;
        let title = escape(&rule.title);
        let description = if rule.description.is_empty() {
            String::new()
        } else {
            format!(
                "<p class='mod-community-rule-description'>{}</p>",
                escape(&rule.description)
            )
        };
        let edit_url = escape(&format!("{base}&edit={id}#community-rule-editor"));
        let delete_url = escape(&format!("{base}&delete={id}#community-rule-delete"));
        let previous = index.checked_sub(1).and_then(|i| rules.get(i));
        let up = move_button(item, &base, rule, "up", "Move up", previous);
        let down = move_button(item, &base, rule, "down", "Move down", rules.get(index + 1));
        write!(html, "<li id='community-rule-{id}' data-rule-id='{id}'><strong>{title}</strong>{description}<div class='mod-community-rule-actions'><a class='button light' href='{edit_url}' aria-label='Edit rule {title}'>Edit rule</a>{up}{down}<a class='button light' href='{delete_url}' aria-label='Delete rule {title}'>Delete rule</a></div></li>").unwrap();
    }
    html.push_str("</ol></section>");
    html
}

fn confirm_delete(item: &Community, base: &str, rule: &CommunityRule) -> String {
    let id = rule.id;
    let fields = action_fields(
        "delete-community-rule",
        base,
        &[("community", &item.slug), ("id", &id.to_string())],
    );
    let title = escape(&rule.title);
    let cancel = escape(&format!("{base}#community-rule-{id}"));
    format!(
        "<section id='community-rule-delete' class='panel mod-section' tabindex='-1' aria-labelledby='community-rule-delete-heading'><h2 id='community-rule-delete-heading'>Delete this rule?</h2><p class='mod-record-detail'><strong>{title}</strong></p><p class='muted'>New reports can no longer use this rule. Existing reports keep the original rule.</p><form class='form-actions mod-community-rule-delete-form' method='post' action='/action'>{fields}<button class='button dark' type='submit'>Delete rule</button><a class='button light' href='{cancel}'>Cancel</a></form></section>"
    )
}

fn editor(
    item: &Community,
    base: &str,
    rule: Option<&CommunityRule>,
    submitted: Option<&BTreeMap<String, String>>,
) -> String {
    let (title, description) = if let Some(form) = submitted {
        (field(form, "title"), field(form, "description"))
    } else if let Some(rule) = rule {
        (rule.title.as_str(), rule.description.as_str())
    } else {
        ("", "")
    };
    let (id, heading, button, cancel) = if let Some(rule) = rule {
        (
            rule.id.to_string(),
            "Edit rule",
            "Save rule",
            format!("<a class='button light' href='{}'>Cancel</a>", escape(base)),
        )
    } else {
        (String::new(), "Add rule", "Create rule", String::new())
    };
    let fields = action_fields(
        "save-community-rule",
        base,
        &[("community", &item.slug), ("id", &id)],
    );
    format!(
        "<section id='community-rule-editor' class='panel mod-section' aria-labelledby='community-rule-editor-heading'><h2 id='community-rule-editor-heading'>{heading}</h2><form class='stack-form mod-community-rule-form' method='post' action='/action'>{fields}<label>Rule title<input name='title' value='{}' required maxlength='{COMMUNITY_RULE_TITLE_MAX}'></label><label>Description <small class='muted'>Optional</small><textarea name='description' rows='3' maxlength='{COMMUNITY_RULE_DESCRIPTION_MAX}'>{}</textarea></label><div class='form-actions'><button type='submit' class='button dark'>{button}</button>{cancel}</div></form></section>",
        escape(title),
        escape(description)
    )
}

fn move_button(
    item: &Community,
    base: &str,
    rule: &CommunityRule,
    direction: &str,
    label: &str,
    neighbor: Option<&CommunityRule>,
) -> String {
    let id = rule.id.to_string();
    let expected = neighbor.map_or_else(String::new, |rule| rule.id.to_string());
    let disabled = if neighbor.is_none() { " disabled" } else { "" };
    let fields = action_fields(
        "move-community-rule",
        base,
        &[
            ("community", &item.slug),
            ("id", &id),
            ("direction", direction),
            ("neighbor", &expected),
        ],
    );
    format!(
        "<form class='inline' method='post' action='/action'>{fields}<button class='button light' type='submit' aria-label='{label} {}'{disabled}>{label}</button></form>",
        escape(&rule.title)
    )
}

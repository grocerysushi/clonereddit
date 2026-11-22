use std::fmt::Write;

use crate::model::{CustomFeed, State};

use super::{action_fields, action_form, escape};

pub(super) fn editor(state: &State, existing: Option<&CustomFeed>) -> String {
    let (title, submit, cancel, fields, max_name_length) = match existing {
        Some(feed) => {
            let id = feed.id.to_string();
            let location = format!("/?view=custom%3A{id}");
            let settings = format!("/feeds/settings?id={id}");
            (
                "Custom feed settings",
                "Save changes",
                location,
                action_fields(
                    "edit-feed",
                    &settings,
                    &[("id", &id), ("visibility_present", "1")],
                ),
                60,
            )
        }
        None => (
            "Create custom feed",
            "Create custom feed",
            "/".to_owned(),
            action_fields("create-feed", "/", &[("visibility_present", "1")]),
            50,
        ),
    };
    let mut choices = String::new();
    for community in state
        .communities
        .iter()
        .filter(|item| state.can_view_community(item))
    {
        let checked = if existing.is_some_and(|feed| feed.communities.contains(&community.slug)) {
            " checked"
        } else {
            ""
        };
        write!(
            choices,
            "<label class='checkbox'><input type='checkbox' name='community-{}' value='on'{checked}> {}</label>",
            escape(&community.slug),
            escape(&community.name),
        )
        .unwrap();
    }
    let delete = existing.map_or_else(String::new, |feed| {
        let id = feed.id.to_string();
        format!(
            "<div class='preference-list'>{}</div>",
            action_form(
                "delete-feed",
                "/",
                &[("id", &id)],
                "Delete custom feed",
                "",
                "button light",
            ),
        )
    });
    let private_checked = if existing.is_some_and(|feed| feed.private) {
        " checked"
    } else {
        ""
    };
    let profile_checked = if existing.is_none_or(|feed| feed.show_on_profile) {
        " checked"
    } else {
        ""
    };
    format!(
        r#"<section class="panel standalone custom-editor"><h1>{title}</h1>
<form class="stack-form" method="post" action="/action">{fields}
<label>Feed name<input name="name" required maxlength="{max_name_length}" value="{name}"></label>
<label>Description <small>Optional</small><textarea name="description" rows="3" maxlength="500">{description}</textarea></label>
<fieldset><legend>Visibility</legend>
<label class="checkbox"><input type="checkbox" name="private" value="on"{private_checked}> Private</label>
<label class="checkbox"><input type="checkbox" name="show_on_profile" value="on"{profile_checked}> Show on profile</label>
</fieldset>
<fieldset><legend>Communities <small>Optional</small></legend>{choices}</fieldset>
<div class="form-actions"><a class="button light" href="{cancel}">Cancel</a><button class="button dark" type="submit">{submit}</button></div>
</form>{delete}</section>"#,
        name = escape(existing.map_or("", |feed| feed.name.as_str())),
        description = escape(existing.map_or("", |feed| feed.description.as_str())),
        cancel = escape(&cancel),
    )
}

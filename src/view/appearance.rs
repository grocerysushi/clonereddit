use std::path::Path;

use super::{escape, field, layout};
use crate::http::{Request, Response};
use crate::model::{Appearance, State};

pub(super) const PATH: &str = "/appearance";

pub(super) fn theme_color(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Light => "#f6f6f0",
        Appearance::Dark => "#161b16",
    }
}

pub(super) fn route(request: &Request, state: &mut State, path: &Path) -> Response {
    if request.method == "GET" {
        let saved = field(&request.query, "saved") == "1";
        return page(state, saved, None);
    }

    let Some(appearance) = Appearance::parse(field(&request.form, "appearance")) else {
        return page(state, false, Some("Choose Light or Dark."));
    };
    let redirect = Response::redirect("/appearance?saved=1");
    if state.username.is_empty() {
        return redirect.with_appearance_cookie(appearance);
    }

    match state.apply("set-appearance", &request.form, path) {
        Ok(_) => redirect,
        Err(error) => page(state, false, Some(&error)),
    }
}

fn page(state: &State, saved: bool, error: Option<&str>) -> Response {
    let (light, dark) = match state.appearance {
        Appearance::Light => (" checked", ""),
        Appearance::Dark => ("", " checked"),
    };
    let info = if state.username.is_empty() {
        "Your choice is saved in this browser."
    } else {
        "Your choice follows your account."
    };
    let feedback = error.map_or_else(String::new, |message| {
        format!(
            "<p class='notice setup-error' role='alert'>{}</p>",
            escape(message)
        )
    });
    let body = format!(
        r#"<section class="panel standalone appearance-settings"><h1>Appearance</h1>
{feedback}
<form class="stack-form" action="{PATH}" method="post">
<fieldset aria-describedby="appearance-help"><legend>Color theme</legend>
<label class="checkbox"><input type="radio" name="appearance" value="light"{light}> Light</label>
<label class="checkbox"><input type="radio" name="appearance" value="dark"{dark}> Dark</label>
<p id="appearance-help" class="muted">{info}</p>
</fieldset>
<button type="submit" class="button dark">Save appearance</button></form></section>"#
    );
    let notice = if saved { "Appearance saved." } else { "" };
    Response::html(layout(state, PATH, "Appearance", &body, "", notice))
}

use super::{account, field, layout, missing};
use crate::auth::{session_token, AuthStore};
use crate::http::{Request, Response};
use crate::model::State;

pub(super) fn requires_account(request: &Request) -> bool {
    matches!(
        request.path.as_str(),
        "/new"
            | "/edit"
            | "/settings"
            | "/report"
            | "/reports"
            | "/creator"
            | "/messages"
            | "/notifications"
            | "/moderation"
            | "/profile"
            | "/communities/start"
            | "/communities/new"
            | "/communities/settings"
            | "/communities/appearance/review"
            | "/communities/preview-image"
            | "/feeds/new"
            | "/feeds/settings"
    ) || request.path.starts_with("/moderation/")
        || request.path.starts_with("/profile/")
        || request.path == "/" && matches!(field(&request.query, "view"), "saved" | "following")
}

pub(super) fn route(request: &Request, state: &mut State, auth: &mut AuthStore) -> Response {
    let (kind, title) = match request.path.as_str() {
        "/account/login" => ("login", "Sign in"),
        "/account/register" => ("register", "Create account"),
        "/account/recover" => ("recover", "Reset your password"),
        "/account/logout" if request.method == "POST" => {
            if let Some(token) = request.cookie.as_deref().and_then(session_token) {
                if let Err(error) = auth.logout(token) {
                    return super::form_error(state, &error, "/profile");
                }
            }
            return Response::redirect("/").with_session_cookie(None);
        }
        _ => return missing(state),
    };
    let page = |state: &State, username: &str, error: &str| {
        Response::html(layout(
            state,
            "account",
            title,
            &account::auth_form(kind, username, error),
            "",
            "",
        ))
    };
    if request.method == "GET" {
        return page(state, "", "");
    }
    let username = field(&request.form, "username").trim().to_ascii_lowercase();
    let password = field(&request.form, "password");
    let result = match kind {
        "register" if state.reserved_handle(&username) => {
            return page(state, &username, "That username is unavailable.");
        }
        "register" => auth.register(&username, password),
        "recover" => auth.recover(&username, field(&request.form, "recovery_code"), password),
        _ => auth.login(&username, password),
    };
    match result {
        Ok(grant) => {
            let response = if let Some(code) = grant.recovery_code {
                state.activate_profile(Some(&username));
                Response::html(layout(
                    state,
                    "account",
                    "Save your recovery code",
                    &account::recovery_code(&username, &code),
                    "",
                    "",
                ))
            } else {
                Response::redirect("/")
            };
            response.with_session_cookie(Some(&grant.session))
        }
        Err(error) => page(state, &username, &error),
    }
}

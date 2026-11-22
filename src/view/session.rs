use super::{account, account_security, field, layout, missing};
use crate::auth::{session_token, AuthGrant, AuthStore, LoginProgress};
use crate::http::{Request, Response};
use crate::mail::AccountMail;
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
            | "/appeals"
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

pub(super) fn route(
    request: &Request,
    state: &mut State,
    auth: &mut AuthStore,
    mail: &AccountMail,
) -> Response {
    if request.path == "/account/security" || request.path.starts_with("/account/security/") {
        return security_route(request, state, auth, mail);
    }
    if matches!(
        request.path.as_str(),
        "/account/reset" | "/account/reset/request"
    ) {
        return reset_route(request, state, auth, mail);
    }
    if request.path == "/account/login/otp" && request.method == "POST" {
        let challenge = field(&request.form, "challenge");
        return match auth.finish_login(challenge, field(&request.form, "otp")) {
            Ok(grant) => signed_in(state, auth, grant, "/"),
            Err(error) => page(
                state,
                "Two-factor sign-in",
                &account_security::challenge(challenge, &error),
            ),
        };
    }
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
    let form_page = |state: &State, username: &str, error: &str| {
        page(state, title, &account::auth_form(kind, username, error))
    };
    if request.method == "GET" {
        return form_page(state, "", "");
    }
    let username = field(&request.form, "username").trim().to_ascii_lowercase();
    let password = field(&request.form, "password");
    if kind == "login" {
        return match auth.begin_login(&username, password) {
            Ok(LoginProgress::SignedIn(grant)) => signed_in(state, auth, grant, "/"),
            Ok(LoginProgress::SecondFactor(challenge)) => page(
                state,
                "Two-factor sign-in",
                &account_security::challenge(&challenge, ""),
            ),
            Err(error) => form_page(state, &username, &error),
        };
    }
    let result = match kind {
        "register" if state.reserved_handle(&username) => {
            return form_page(state, &username, "That username is unavailable.");
        }
        "register" => auth.register(&username, password),
        _ => auth.recover_with_otp(
            &username,
            field(&request.form, "recovery_code"),
            password,
            field(&request.form, "otp"),
        ),
    };
    match result {
        Ok(grant) => signed_in(state, auth, grant, "/"),
        Err(error) => form_page(state, &username, &error),
    }
}

fn reset_route(
    request: &Request,
    state: &mut State,
    auth: &mut AuthStore,
    mail: &AccountMail,
) -> Response {
    const DELIVERY_DISABLED: &str =
        "Email delivery is unavailable. You can use a saved recovery code.";
    let input = |key| field(&request.form, key);
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/account/reset/request") => {
            let error = if mail.configured() {
                ""
            } else {
                DELIVERY_DISABLED
            };
            page(
                state,
                "Reset by email",
                &account_security::reset_request("", error),
            )
        }
        ("POST", "/account/reset/request") => {
            if !mail.configured() {
                return page(
                    state,
                    "Reset by email",
                    &account_security::reset_request("", DELIVERY_DISABLED),
                );
            }
            if let Ok(Some(issue)) = auth.request_email_reset(input("account")) {
                let body = format!("Use this one-time code to reset your Commonline password:\n\n{}\n\nOpen {}/account/reset and enter your username or verified email. If you did not request a reset, you can ignore this email.", issue.code, request.origin);
                mail.queue_reset(issue.address, body);
            }
            page(state, "Reset by email", &account_security::reset_request(
                "If the account has a verified email, we’ll send a reset code. Check your inbox and spam folder.", "",
            ))
        }
        ("GET", "/account/reset") => page(
            state,
            "Reset your password",
            &account_security::reset_form("", ""),
        ),
        ("POST", "/account/reset") => match auth.reset_by_email(
            input("account"),
            input("code"),
            input("password"),
            input("otp"),
        ) {
            Ok(grant) => signed_in(state, auth, grant, "/profile"),
            Err(error) => page(
                state,
                "Reset your password",
                &account_security::reset_form(input("account"), &error),
            ),
        },
        _ => missing(state),
    }
}

fn security_route(
    request: &Request,
    state: &mut State,
    auth: &mut AuthStore,
    mail: &AccountMail,
) -> Response {
    let username = state.username.clone();
    if username.is_empty() {
        return Response::redirect("/account/login");
    }
    let input = |key| field(&request.form, key);
    let result = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/account/security") => {
            return settings_page(state, auth, field(&request.query, "notice"), "")
        }
        ("GET", "/account/security/totp/confirm")
            if auth
                .security(&username)
                .is_some_and(|info| info.pending_two_factor) =>
        {
            return page(
                state,
                "Set up two-factor authentication",
                &account_security::setup("", ""),
            );
        }
        ("POST", "/account/security/email") => {
            if !mail.configured() {
                return settings_page(
                    state,
                    auth,
                    "",
                    "Email delivery is unavailable on this server.",
                );
            }
            auth.request_email_verification(&username, input("password"), input("otp"), input("email")).and_then(|issue| {
                let body = format!("Use this one-time code to verify this email address for Commonline:\n\n{}\n\nEnter it in Security at {}/account/security. If you did not request verification, you can ignore this email.", issue.code, request.origin);
                mail.send_verification(&issue.address, &body)
                    .map(|()| "Verification code sent.")
                    .map_err(|_| "Could not send the code. Try again shortly.".into())
            })
        }
        ("POST", "/account/security/verify") => auth
            .verify_email(&username, input("code"))
            .map(|()| "Email verified."),
        ("POST", "/account/security/totp/start") => {
            return match auth.start_two_factor(&username, input("password")) {
                Ok(secret) => page(
                    state,
                    "Set up two-factor authentication",
                    &account_security::setup(&secret, ""),
                ),
                Err(error) => settings_page(state, auth, "", &error),
            };
        }
        ("POST", "/account/security/totp/confirm") => {
            return match auth.confirm_two_factor(&username, input("otp")) {
                Ok(grant) => signed_in(
                    state,
                    auth,
                    grant,
                    "/account/security?notice=Two-factor%20authentication%20enabled.",
                ),
                Err(error) => page(
                    state,
                    "Set up two-factor authentication",
                    &account_security::setup("", &error),
                ),
            };
        }
        ("POST", "/account/security/totp/disable") => {
            return match auth.disable_two_factor(&username, input("password"), input("otp")) {
                Ok(grant) => signed_in(
                    state,
                    auth,
                    grant,
                    "/account/security?notice=Two-factor%20authentication%20disabled.",
                ),
                Err(error) => settings_page(state, auth, "", &error),
            };
        }
        ("POST", "/account/security/password") => {
            return match auth.change_password(
                &username,
                input("current_password"),
                input("password"),
                input("otp"),
            ) {
                Ok(grant) => signed_in(
                    state,
                    auth,
                    grant,
                    "/account/security?notice=Password%20changed.",
                ),
                Err(error) => settings_page(state, auth, "", &error),
            };
        }
        _ => return missing(state),
    };
    match result {
        Ok(notice) => Response::redirect(&format!(
            "/account/security?notice={}",
            super::encode(notice)
        )),
        Err(error) => settings_page(state, auth, "", &error),
    }
}

fn settings_page(state: &State, auth: &AuthStore, notice: &str, error: &str) -> Response {
    let Some(info) = auth.security(&state.username) else {
        return Response::redirect("/account/login");
    };
    Response::html(layout(
        state,
        "account",
        "Account security",
        &account_security::settings(&info, error),
        "",
        notice,
    ))
}

fn page(state: &State, title: &str, body: &str) -> Response {
    Response::html(layout(state, "account", title, body, "", ""))
}

fn signed_in(state: &mut State, auth: &AuthStore, grant: AuthGrant, destination: &str) -> Response {
    let response = match (&grant.recovery_code, auth.username(&grant.session)) {
        (Some(code), Some(username)) => {
            state.activate_profile(Some(username));
            page(
                state,
                "Save your recovery code",
                &account::recovery_code(username, code),
            )
        }
        _ => Response::redirect(destination),
    };
    response.with_session_cookie(Some(&grant.session))
}

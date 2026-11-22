use std::path::Path;

use super::{account, account_security, field, layout, missing};
use crate::account_deletion;
use crate::auth::{session_token, AuthGrant, AuthStore, LoginProgress, SessionDevice};
use crate::http::{Request, Response};
use crate::mail::AccountMail;
use crate::model::{ChatRequestPermission, NotificationPreferences, State};

pub(super) fn requires_account(request: &Request) -> bool {
    matches!(
        request.path.as_str(),
        "/new"
            | "/edit"
            | "/settings"
            | "/report"
            | "/report/content"
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
    path: &Path,
    auth: &mut AuthStore,
    mail: &AccountMail,
) -> Response {
    if matches!(
        request.path.as_str(),
        "/account/settings" | "/account/settings/chat"
    ) {
        return account_settings(request, state, path);
    }
    if request.path == "/account/security" || request.path.starts_with("/account/security/") {
        return security_route(request, state, path, auth, mail);
    }
    if matches!(
        request.path.as_str(),
        "/account/reset" | "/account/reset/request"
    ) {
        return reset_route(request, state, auth, mail);
    }
    if request.path == "/account/login/otp" && request.method == "POST" {
        let challenge = field(&request.form, "challenge");
        return match auth.finish_login(challenge, field(&request.form, "otp"), device(request)) {
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
        return match auth.begin_login(&username, password, device(request)) {
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
        "register" => auth.register(&username, password, device(request)),
        _ => auth.recover_with_otp(
            &username,
            field(&request.form, "recovery_code"),
            password,
            field(&request.form, "otp"),
            device(request),
        ),
    };
    match result {
        Ok(grant) => signed_in(state, auth, grant, "/"),
        Err(error) => form_page(state, &username, &error),
    }
}

fn account_settings(request: &Request, state: &mut State, path: &Path) -> Response {
    if state.username.is_empty() {
        return Response::redirect("/account/login");
    }

    let previous_notifications = state.notification_preferences();
    let previous_chat = state.chat_request_permission();
    if request.method == "GET" {
        if request.path == "/account/settings/chat" {
            return Response::redirect("/account/settings");
        }
        let saved = match field(&request.query, "saved") {
            "1" => Some("Notification preferences saved."),
            "chat" => Some("Chat request settings saved."),
            _ => None,
        };
        return settings_form(state, previous_notifications, previous_chat, saved, "");
    }
    let saving_chat = request.path == "/account/settings/chat";
    let (notifications, chat) = if saving_chat {
        let Some(chat) = ChatRequestPermission::parse(field(&request.form, "chat_requests")) else {
            return settings_form(
                state,
                previous_notifications,
                previous_chat,
                None,
                "Choose who can send you chat requests.",
            );
        };
        (previous_notifications, chat)
    } else {
        if ["replies", "messages", "moderation"]
            .iter()
            .any(|key| request.form.get(*key).is_some_and(|value| value != "on"))
        {
            return settings_form(
                state,
                previous_notifications,
                previous_chat,
                None,
                "Choose valid notification preferences.",
            );
        }
        (
            NotificationPreferences {
                replies: request.form.contains_key("replies"),
                messages: request.form.contains_key("messages"),
                moderation: request.form.contains_key("moderation"),
            },
            previous_chat,
        )
    };
    state.set_notification_preferences(notifications);
    state.set_chat_request_permission(chat);
    match state.save(path) {
        Ok(()) if saving_chat => Response::redirect("/account/settings?saved=chat"),
        Ok(()) => Response::redirect("/account/settings?saved=1"),
        Err(error) => {
            state.set_notification_preferences(previous_notifications);
            state.set_chat_request_permission(previous_chat);
            eprintln!("ThreadPorch could not save account settings. {error}");
            let error = if saving_chat {
                "Could not save chat request settings. Try again."
            } else {
                "Could not save notification preferences. Try again."
            };
            settings_form(state, notifications, chat, None, error)
        }
    }
}

fn settings_form(
    state: &State,
    notifications: NotificationPreferences,
    chat: ChatRequestPermission,
    saved: Option<&str>,
    error: &str,
) -> Response {
    page(
        state,
        "Account settings",
        &account::settings(notifications, chat, saved, error),
    )
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
                let body = format!("Use this one-time code to reset your ThreadPorch password:\n\n{}\n\nOpen {}/account/reset and enter your username or verified email. If you did not request a reset, you can ignore this email.", issue.code, request.origin);
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
            device(request),
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
    path: &Path,
    auth: &mut AuthStore,
    mail: &AccountMail,
) -> Response {
    let Some(current_token) = request.cookie.as_deref().and_then(session_token) else {
        return Response::redirect("/account/login");
    };
    let Some(username) = auth.username(current_token).map(str::to_owned) else {
        return Response::redirect("/account/login");
    };
    let input = |key| field(&request.form, key);
    let result = match (request.method.as_str(), request.path.as_str()) {
        ("GET" | "POST", "/account/security/delete") => {
            return delete_account(request, state, path, auth, current_token, &username)
        }
        ("GET", "/account/security") => {
            return settings_page(
                state,
                auth,
                current_token,
                field(&request.query, "notice"),
                "",
            )
        }
        ("POST", "/account/security/sessions/revoke") => auth
            .revoke_session(current_token, input("session_id"))
            .map(|()| "Session signed out."),
        ("POST", "/account/security/sessions/revoke-others") => {
            auth.revoke_other_sessions(current_token).map(|count| {
                if count == 0 {
                    "No other active sessions."
                } else {
                    "Signed out everywhere else."
                }
            })
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
                    current_token,
                    "",
                    "Email delivery is unavailable on this server.",
                );
            }
            auth.request_email_verification(&username, input("password"), input("otp"), input("email")).and_then(|issue| {
                let body = format!("Use this one-time code to verify this email address for ThreadPorch:\n\n{}\n\nEnter it in Security at {}/account/security. If you did not request verification, you can ignore this email.", issue.code, request.origin);
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
                Err(error) => settings_page(state, auth, current_token, "", &error),
            };
        }
        ("POST", "/account/security/totp/confirm") => {
            return match auth.confirm_two_factor(&username, input("otp"), device(request)) {
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
            return match auth.disable_two_factor(
                &username,
                input("password"),
                input("otp"),
                device(request),
            ) {
                Ok(grant) => signed_in(
                    state,
                    auth,
                    grant,
                    "/account/security?notice=Two-factor%20authentication%20disabled.",
                ),
                Err(error) => settings_page(state, auth, current_token, "", &error),
            };
        }
        ("POST", "/account/security/password") => {
            return match auth.change_password(
                &username,
                input("current_password"),
                input("password"),
                input("otp"),
                device(request),
            ) {
                Ok(grant) => signed_in(
                    state,
                    auth,
                    grant,
                    "/account/security?notice=Password%20changed.",
                ),
                Err(error) => settings_page(state, auth, current_token, "", &error),
            };
        }
        _ => return missing(state),
    };
    match result {
        Ok(notice) => Response::redirect(&format!(
            "/account/security?notice={}",
            super::encode(notice)
        )),
        Err(error) => settings_page(state, auth, current_token, "", &error),
    }
}

fn delete_account(
    request: &Request,
    state: &mut State,
    path: &Path,
    auth: &mut AuthStore,
    current_token: &str,
    username: &str,
) -> Response {
    let Some(security) = auth.security(username) else {
        return Response::redirect("/account/login");
    };
    let two_factor = security.two_factor;
    let input = |key| field(&request.form, key);
    let error = if request.method == "GET" {
        String::new()
    } else if input("confirm") != "yes" {
        "Confirm that you want to permanently delete your account.".into()
    } else {
        match auth.begin_account_deletion(current_token, input("password"), input("otp")) {
            Ok(()) => {
                let cleanup_pending = account_deletion::finish_pending(state, path, auth)
                    .inspect_err(|error| {
                        eprintln!("ThreadPorch will retry account deletion cleanup. {error}")
                    })
                    .is_err();
                let title = if cleanup_pending {
                    "Account closed"
                } else {
                    "Account deleted"
                };
                return page(
                    state,
                    title,
                    &account_security::account_deleted(cleanup_pending),
                )
                .with_session_cookie(None);
            }
            Err(error) => error,
        }
    };
    page(
        state,
        "Delete your account",
        &account_security::delete_account(username, two_factor, &error),
    )
}

fn settings_page(
    state: &State,
    auth: &AuthStore,
    current_token: &str,
    notice: &str,
    error: &str,
) -> Response {
    let (Some(info), Some(sessions)) = (
        auth.security(&state.username),
        auth.active_sessions(current_token),
    ) else {
        return Response::redirect("/account/login");
    };
    Response::html(layout(
        state,
        "account",
        "Account security",
        &account_security::settings(&info, &sessions, error),
        "",
        notice,
    ))
}

fn device(request: &Request) -> SessionDevice {
    SessionDevice::from_user_agent(request.user_agent.as_deref().unwrap_or_default())
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

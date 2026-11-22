use std::fmt::Write;

use super::{escape, hidden};
use crate::auth::{AccountSecurity, SessionSummary};
use crate::schedule_time;

pub(super) fn settings(
    info: &AccountSecurity<'_>,
    sessions: &[SessionSummary],
    error: &str,
) -> String {
    let alert = error_notice(error);
    let otp = if info.two_factor {
        otp_field(true)
    } else {
        String::new()
    };
    let email = recovery_email(info, &otp);
    let authenticator = authenticator_settings(info.two_factor, info.pending_two_factor, &otp);
    let sessions = active_sessions(sessions);
    let current = current_password("current_password");
    let password = new_password();
    format!(
        r#"<section class="panel standalone account-security"><a class="back-link" href="/profile">← Your profile</a><h1>Account security</h1>{alert}{sessions}
<section class="account-security-section" aria-labelledby="account-password-heading"><h2 id="account-password-heading">Change password</h2>
<form class="stack-form account-security-form" method="post" action="/account/security/password" aria-labelledby="account-password-heading">{current}{password}{otp}<button type="submit" class="button dark">Change password</button></form></section>
{email}{authenticator}
<section class="account-security-section account-delete-entry" aria-labelledby="account-delete-heading"><h2 id="account-delete-heading">Delete account</h2><p class="muted">Permanently delete your account and sign out everywhere.</p><a class="button light account-delete-link" href="/account/security/delete">Delete account</a></section></section>"#
    )
}

pub(super) fn delete_account(username: &str, two_factor: bool, error: &str) -> String {
    let username = escape(username);
    let alert = error_notice(error);
    let password = current_password("password");
    let otp = if two_factor {
        otp_field(true)
    } else {
        String::new()
    };
    format!(
        r#"<section class="panel standalone account-auth account-security account-delete-confirm"><h1 id="delete-account-title">Delete your account</h1>{alert}
<p>Deleting <strong>u/{username}</strong> is permanent.</p>
<ul class="account-delete-consequences">
<li>Your profile, saved activity, drafts, and recovery data will be deleted. All sessions will end.</li>
<li>Your existing posts and comments will stay in their discussions with the author marked as deleted. Copies of messages others received and moderation records can remain.</li>
<li>Communities you own keep their moderators and access settings. Owner-only settings become unavailable.</li>
</ul>
<form class="stack-form account-delete-form" method="post" action="/account/security/delete" aria-labelledby="delete-account-title">{password}{otp}
<label class="checkbox account-delete-consent"><input type="checkbox" name="confirm" value="yes" required><span>I understand that deleting my account is permanent.</span></label>
<div class="form-actions"><a class="button light" href="/account/security">Cancel</a><button type="submit" class="button account-delete-danger">Permanently delete account</button></div></form></section>"#
    )
}

pub(super) fn account_deleted(cleanup_pending: bool) -> String {
    let (title, detail) = if cleanup_pending {
        (
            "Account closed",
            "All sessions have ended. Your account cleanup is still in progress.",
        )
    } else {
        (
            "Account deleted",
            "Your account has been deleted, and all sessions have ended.",
        )
    };
    format!(
        r#"<section class="panel standalone account-auth account-security account-delete-result"><h1>{title}</h1><p role="status">{detail}</p><a class="button light" href="/">Return home</a></section>"#
    )
}

fn active_sessions(sessions: &[SessionSummary]) -> String {
    let mut html = String::from("<section class='account-security-section account-active-sessions' aria-labelledby='account-sessions-heading'><h2 id='account-sessions-heading'>Active sessions</h2>");
    if !sessions.is_empty() {
        html.push_str("<ul class='account-session-list'>");
        for session in sessions {
            let device = escape(&session.device);
            let (class, badge, action) = if session.current {
                (
                    " account-session-current",
                    "<span class='flair'>This session</span>",
                    "<form method='post' action='/account/logout'><button type='submit' class='button light'>Sign out here</button></form>".to_owned(),
                )
            } else {
                let session_id = hidden("session_id", &session.id);
                (
                    "",
                    "",
                    format!("<form method='post' action='/account/security/sessions/revoke'>{session_id}<button type='submit' class='button light' aria-label='Sign out {device} session'>Sign out session</button></form>"),
                )
            };
            let started = session
                .created_at
                .map_or_else(|| "Unavailable".to_owned(), utc_time);
            let expiry = utc_time(session.expires_at);
            write!(html, "<li class='account-session-row{class}'><div class='account-session-detail'><div class='account-session-device'><strong>{device}</strong>{badge}</div><dl class='account-session-meta'><div><dt>Signed in</dt><dd>{started}</dd></div><div><dt>Expires</dt><dd>{expiry}</dd></div></dl></div>{action}</li>").unwrap();
        }
        html.push_str("</ul>");
    }
    if sessions.iter().any(|session| !session.current) {
        html.push_str("<details class='account-session-bulk'><summary class='button light'>Sign out everywhere else</summary><form method='post' action='/account/security/sessions/revoke-others'><p class='muted'>Other browsers and devices will need to sign in again.</p><button type='submit' class='button dark'>Confirm sign out</button></form></details>");
    } else {
        html.push_str("<p class='muted account-session-empty'>No other active sessions.</p>");
    }
    html.push_str("</section>");
    html
}

fn utc_time(timestamp: u64) -> String {
    let local = schedule_time::to_local(timestamp, "UTC");
    if local.is_empty() {
        return "Unavailable".to_owned();
    }
    let seconds = timestamp % 60;
    format!(
        "<time datetime='{local}:{seconds:02}Z'>{}</time>",
        escape(&schedule_time::display(timestamp, "UTC"))
    )
}

fn recovery_email(info: &AccountSecurity<'_>, otp: &str) -> String {
    let verified = info.email.map_or_else(
        || {
            "<p class='muted'>Verify a recovery email to reset your password by email.</p>"
                .to_owned()
        },
        |email| {
            format!(
                "<p class='account-security-email'><span class='flair'>Verified</span> {}</p>",
                escape(email)
            )
        },
    );
    let verify = info.pending_email.map_or_else(String::new, |email| {
        let email = escape(email);
        format!(
            r#"<div class="account-security-pending"><p class="account-security-email"><span class="flair">Awaiting verification</span> {email}</p>
<form class="stack-form account-security-form" method="post" action="/account/security/verify" aria-label="Verify recovery email">
<label>Verification code<input name="code" autocomplete="one-time-code" autocapitalize="none" spellcheck="false" maxlength="64" pattern="[A-Fa-f0-9]{{64}}" required></label><button type="submit" class="button dark">Verify email</button></form></div>"#
        )
    });
    let password = current_password("password");
    let label = if info.email.is_some() {
        "New recovery email"
    } else {
        "Email address"
    };
    format!(
        r#"<section class="account-security-section" aria-labelledby="account-email-heading"><h2 id="account-email-heading">Recovery email</h2>{verified}{verify}
<form class="stack-form account-security-form" method="post" action="/account/security/email" aria-labelledby="account-email-heading">
<label>{label}<input type="email" name="email" autocomplete="email" autocapitalize="none" spellcheck="false" maxlength="254" required></label>{password}{otp}<button type="submit" class="button dark">Send verification code</button></form></section>"#
    )
}

fn authenticator_settings(enabled: bool, pending: bool, otp: &str) -> String {
    let password = current_password("password");
    let body = if enabled {
        format!(
            r#"<p class="muted">Sign-in asks for a code from your authenticator app.</p>
<form class="stack-form account-security-form" method="post" action="/account/security/totp/disable" aria-label="Turn off two-factor authentication">{password}{otp}<button type="submit" class="button light">Turn off two-factor authentication</button></form>"#
        )
    } else {
        let button = if pending {
            "Restart authenticator setup"
        } else {
            "Set up authenticator app"
        };
        let continue_setup = if pending {
            "<a class='button light' href='/account/security/totp/confirm'>Continue setup</a>"
        } else {
            ""
        };
        format!(
            r#"<p class="muted">Add an authenticator app to require a code when you sign in.</p>
<form class="stack-form account-security-form" method="post" action="/account/security/totp/start" aria-label="Set up an authenticator app">{password}<div class="form-actions"><button type="submit" class="button dark">{button}</button>{continue_setup}</div></form>"#
        )
    };
    let status = if enabled {
        "On"
    } else if pending {
        "Setup started"
    } else {
        "Off"
    };
    format!(
        r#"<section id="two-factor" class="account-security-section" aria-labelledby="account-totp-heading"><header class="account-security-section-title"><h2 id="account-totp-heading">Two-factor authentication</h2><span class="flair">{status}</span></header>{body}</section>"#
    )
}

pub(super) fn setup(secret: &str, error: &str) -> String {
    let alert = error_notice(error);
    let instructions = if secret.is_empty() {
        "<p>Enter a current code from the authenticator entry you added. Return to Account security to start again.</p>".to_owned()
    } else {
        format!(
            r#"<p>Add a time-based account named ThreadPorch in your authenticator app, then paste this setup key. Choose six digits, a 30-second period, and SHA-1 if your app asks. Enter its code within 10 minutes.</p>
<label class="account-secret account-totp-key">Setup key<input readonly autocomplete="off" autocapitalize="none" spellcheck="false" value="{}"></label>"#,
            escape(secret)
        )
    };
    let otp = otp_field(true);
    let back = if secret.is_empty() {
        "Restart setup in Account security"
    } else {
        "Back to Account security"
    };
    format!(
        r#"<section class="panel standalone account-auth account-security"><h1>Set up your authenticator app</h1>{alert}{instructions}
<form class="stack-form account-auth-form" method="post" action="/account/security/totp/confirm">{otp}<button type="submit" class="button dark">Turn on two-factor authentication</button></form>
<div class="account-auth-links"><a class="subtle-link" href="/account/security#two-factor">{back}</a></div></section>"#
    )
}

pub(super) fn challenge(token: &str, error: &str) -> String {
    let alert = error_notice(error);
    let token = hidden("challenge", token);
    let otp = otp_field(true);
    format!(
        r#"<section class="panel standalone account-auth account-security"><h1>Enter your authenticator code</h1>{alert}<p>Open your authenticator app and enter the six-digit code for the account you set up.</p>
<form class="stack-form account-auth-form" method="post" action="/account/login/otp">{token}{otp}<button type="submit" class="button dark">Verify and sign in</button></form>
<div class="account-auth-links"><a class="subtle-link" href="/account/login">Back to sign in</a></div></section>"#
    )
}

pub(super) fn reset_request(notice: &str, error: &str) -> String {
    let alert = error_notice(error);
    let notice = if notice.is_empty() {
        String::new()
    } else {
        format!("<p class='notice' role='status'>{}</p>", escape(notice))
    };
    let account = account_input("");
    format!(
        r#"<section class="panel standalone account-auth account-security"><h1>Reset your password by email</h1>{notice}{alert}<p>Enter your username or recovery email to request a reset code.</p>
<form class="stack-form account-auth-form" method="post" action="/account/reset/request">{account}<button type="submit" class="button dark">Send reset code</button></form>
<div class="account-auth-links"><a class="subtle-link" href="/account/reset">Enter a reset code</a><a class="subtle-link" href="/account/recover">Use a recovery code</a><a class="subtle-link" href="/account/login">Back to sign in</a></div></section>"#
    )
}

pub(super) fn reset_form(account: &str, error: &str) -> String {
    let alert = error_notice(error);
    let account = account_input(account);
    let password = new_password();
    let otp = otp_field(false);
    format!(
        r#"<section class="panel standalone account-auth account-security"><h1>Enter your email reset code</h1>{alert}
<form class="stack-form account-auth-form" method="post" action="/account/reset">{account}<label>Email reset code<input name="code" autocomplete="one-time-code" autocapitalize="none" spellcheck="false" maxlength="64" pattern="[A-Fa-f0-9]{{64}}" required></label>{password}{otp}<button type="submit" class="button dark">Reset password</button></form>
<div class="account-auth-links"><a class="subtle-link" href="/account/reset/request">Request a reset code</a><a class="subtle-link" href="/account/recover">Use a recovery code</a></div></section>"#
    )
}

pub(super) fn otp_field(required: bool) -> String {
    let required_attribute = if required { " required" } else { "" };
    let hint = if required {
        ""
    } else {
        "<small>Enter a code if you use an authenticator app.</small>"
    };
    format!(
        "<label>Authenticator code<input class='account-otp-input' name='otp' inputmode='numeric' autocomplete='one-time-code' minlength='6' maxlength='6' pattern='[0-9]{{6}}'{required_attribute}>{hint}</label>"
    )
}

fn current_password(field: &str) -> String {
    format!("<label>Current password<input type='password' name='{field}' autocomplete='current-password' maxlength='128' required></label>")
}

fn new_password() -> &'static str {
    "<label>New password<input type='password' name='password' autocomplete='new-password' minlength='12' maxlength='128' required><small>Use at least 12 characters.</small></label>"
}

fn account_input(account: &str) -> String {
    format!("<label>Username or recovery email<input name='account' autocomplete='username' autocapitalize='none' spellcheck='false' maxlength='254' value='{}' required></label>", escape(account))
}

fn error_notice(error: &str) -> String {
    if error.is_empty() {
        String::new()
    } else {
        format!(
            "<p class='notice account-error' role='alert'>{}</p>",
            escape(error)
        )
    }
}

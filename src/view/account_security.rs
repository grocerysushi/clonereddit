use super::{escape, hidden};
use crate::auth::AccountSecurity;

pub(super) fn settings(info: &AccountSecurity<'_>, error: &str) -> String {
    let alert = error_notice(error);
    let otp = if info.two_factor {
        otp_field(true)
    } else {
        String::new()
    };
    let email = recovery_email(info, &otp);
    let authenticator = authenticator_settings(info.two_factor, info.pending_two_factor, &otp);
    let current = current_password("current_password");
    let password = new_password();
    format!(
        r#"<section class="panel standalone account-security"><a class="back-link" href="/profile">← Your profile</a><h1>Account security</h1>{alert}
<section class="account-security-section" aria-labelledby="account-password-heading"><h2 id="account-password-heading">Change password</h2>
<form class="stack-form account-security-form" method="post" action="/account/security/password" aria-labelledby="account-password-heading">{current}{password}{otp}<button type="submit" class="button dark">Change password</button></form></section>
{email}{authenticator}</section>"#
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
            r#"<p>Add a time-based account named Commonline in your authenticator app, then paste this setup key. Choose six digits, a 30-second period, and SHA-1 if your app asks. Enter its code within 10 minutes.</p>
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
        r#"<section class="panel standalone account-auth account-security"><h1>Enter your authenticator code</h1>{alert}<p>Open your authenticator app and enter the six-digit code for Commonline.</p>
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

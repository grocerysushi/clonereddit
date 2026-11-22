use std::collections::BTreeSet;
use std::io::{self, Write};

use subtle::ConstantTimeEq;

use super::{
    canonical_username, checked_username, decode_hash, hex, invalid_store, random_secret,
    secret_hash, unix_seconds, AuthGrant, AuthStore, SecretHash, SessionDevice,
};
use crate::{http::valid_session_token, mail::valid_address, totp};

const EMAIL_CODE_LIFETIME: u64 = 15 * 60;
const EMAIL_COOLDOWN: u64 = 60;
const SETUP_LIFETIME: u64 = 10 * 60;
const LOGIN_LIFETIME: u64 = 5 * 60;
const OTP_WINDOW: u64 = 5 * 60;
const OTP_ATTEMPTS: u8 = 5;
const MAX_CHALLENGES: usize = 256;
const CHALLENGE_ERROR: &str = "This sign-in attempt expired. Sign in again.";
const OTP_ERROR: &str = "Enter a current, unused authenticator code.";
const OTP_LIMIT_ERROR: &str = "Too many authenticator attempts. Try again in a few minutes.";
const EMAIL_RESET_ERROR: &str = "The account or email reset code is incorrect or expired.";

pub enum LoginProgress {
    SignedIn(AuthGrant),
    SecondFactor(String),
}

pub struct AccountSecurity<'a> {
    pub email: Option<&'a str>,
    pub pending_email: Option<&'a str>,
    pub two_factor: bool,
    pub pending_two_factor: bool,
}

pub struct EmailIssue {
    pub address: String,
    pub code: String,
}

#[derive(Clone)]
struct EmailCode {
    address: String,
    hash: SecretHash,
    expires_at: u64,
}

impl EmailCode {
    fn issue(address: String, now: u64) -> Result<(Self, EmailIssue), String> {
        let code = random_secret()?;
        Ok((
            Self {
                address: address.clone(),
                hash: secret_hash(&code),
                expires_at: now.saturating_add(EMAIL_CODE_LIFETIME),
            },
            EmailIssue { address, code },
        ))
    }

    fn matches(&self, code: &str, now: u64) -> bool {
        let code = code.trim().to_ascii_lowercase();
        self.expires_at > now
            && valid_session_token(&code)
            && bool::from(self.hash.ct_eq(&secret_hash(&code)))
    }
}

#[derive(Clone)]
struct Factor {
    secret: String,
    last_counter: u64,
}

#[derive(Clone)]
struct FactorSetup {
    secret: String,
    expires_at: u64,
}

#[derive(Clone, Default)]
pub(super) struct SecurityData {
    email: Option<String>,
    pending_email: Option<EmailCode>,
    email_reset: Option<EmailCode>,
    verification_sent_at: u64,
    reset_sent_at: u64,
    factor: Option<Factor>,
    pending_factor: Option<FactorSetup>,
}

#[derive(Clone)]
pub(super) struct LoginChallenge {
    pub(super) username: String,
    expires_at: u64,
    attempts_left: u8,
}

#[derive(Clone)]
pub(super) struct OtpFailures {
    attempts: u8,
    expires_at: u64,
}

impl AuthStore {
    pub fn security(&self, username: &str) -> Option<AccountSecurity<'_>> {
        let name = checked_username(username).ok()?;
        let data = &self.accounts.get(&name)?.security;
        let now = unix_seconds();
        Some(AccountSecurity {
            email: data.email.as_deref(),
            pending_email: data
                .pending_email
                .as_ref()
                .filter(|pending| pending.expires_at > now)
                .map(|pending| pending.address.as_str()),
            two_factor: data.factor.is_some(),
            pending_two_factor: data
                .pending_factor
                .as_ref()
                .is_some_and(|setup| setup.expires_at > now),
        })
    }

    pub fn begin_login(
        &mut self,
        username: &str,
        password: &str,
        device: SessionDevice,
    ) -> Result<LoginProgress, String> {
        let username = self.verify_password(username, password)?;
        if self.accounts[&username].security.factor.is_none() {
            let mut updated = self.clone();
            let grant = updated.add_session(&username, None, device)?;
            self.commit(updated)?;
            return Ok(LoginProgress::SignedIn(grant));
        }

        let now = unix_seconds();
        let token = random_secret()?;
        let hash = secret_hash(&token);
        self.login_challenges
            .retain(|_, challenge| challenge.expires_at > now && challenge.username != username);
        if self.login_challenges.len() >= MAX_CHALLENGES
            || self.login_challenges.contains_key(&hash)
        {
            return Err("Could not start a new sign-in attempt. Try again shortly.".into());
        }
        self.login_challenges.insert(
            hash,
            LoginChallenge {
                username,
                expires_at: now.saturating_add(LOGIN_LIFETIME),
                attempts_left: OTP_ATTEMPTS,
            },
        );
        Ok(LoginProgress::SecondFactor(token))
    }

    pub fn finish_login(
        &mut self,
        challenge: &str,
        otp: &str,
        device: SessionDevice,
    ) -> Result<AuthGrant, String> {
        if !valid_session_token(challenge) {
            return Err(CHALLENGE_ERROR.into());
        }
        let hash = secret_hash(challenge);
        let now = unix_seconds();
        self.login_challenges
            .retain(|_, pending| pending.expires_at > now);
        let username = self
            .login_challenges
            .get(&hash)
            .ok_or(CHALLENGE_ERROR)?
            .username
            .clone();
        let counter = match self.verify_active_factor(&username, otp, now) {
            Ok(Some(counter)) => counter,
            Ok(None) => return Err(CHALLENGE_ERROR.into()),
            Err(error) => {
                let Some(pending) = self.login_challenges.get_mut(&hash) else {
                    return Err(CHALLENGE_ERROR.into());
                };
                pending.attempts_left -= 1;
                if pending.attempts_left == 0 {
                    self.login_challenges.remove(&hash);
                }
                return Err(error);
            }
        };
        let mut updated = self.clone();
        updated.login_challenges.remove(&hash);
        updated
            .accounts
            .get_mut(&username)
            .ok_or(CHALLENGE_ERROR)?
            .security
            .accept_counter(Some(counter));
        updated.otp_failures.remove(&username);
        let grant = updated.add_session(&username, None, device)?;
        self.commit(updated)?;
        Ok(grant)
    }

    pub fn change_password(
        &mut self,
        username: &str,
        current_password: &str,
        new_password: &str,
        otp: &str,
        device: SessionDevice,
    ) -> Result<AuthGrant, String> {
        let username = self.verify_password(username, current_password)?;
        if current_password == new_password {
            return Err("Choose a different password.".into());
        }
        let counter = self.verify_active_factor(&username, otp, unix_seconds())?;
        self.replace_credentials(username, new_password, counter, device)
    }

    pub fn request_email_verification(
        &mut self,
        username: &str,
        password: &str,
        otp: &str,
        address: &str,
    ) -> Result<EmailIssue, String> {
        let username = self.verify_password(username, password)?;
        let address = checked_address(address).ok_or("Enter a valid email address.")?;
        let now = unix_seconds();
        let data = &self.accounts[&username].security;
        if data.email.as_deref() == Some(address.as_str()) {
            return Err("This address is already verified for your account.".into());
        }
        if in_cooldown(data.verification_sent_at, now) {
            return Err("Wait a minute between email verification requests.".into());
        }
        let counter = self.verify_active_factor(&username, otp, now)?;
        let (record, issue) = EmailCode::issue(address, now)?;
        let mut updated = self.clone();
        let data = &mut updated
            .accounts
            .get_mut(&username)
            .ok_or("Account unavailable.")?
            .security;
        data.pending_email = Some(record);
        data.verification_sent_at = now;
        data.accept_counter(counter);
        updated.otp_failures.remove(&username);
        self.commit(updated)?;
        Ok(issue)
    }

    pub fn verify_email(&mut self, username: &str, code: &str) -> Result<(), String> {
        const ERROR: &str = "The email verification code is incorrect or expired.";
        let username = checked_username(username).map_err(|_| ERROR)?;
        let now = unix_seconds();
        let address = self
            .accounts
            .get(&username)
            .and_then(|account| account.security.pending_email.as_ref())
            .filter(|pending| pending.matches(code, now))
            .map(|pending| pending.address.clone())
            .ok_or(ERROR)?;
        if self.accounts.iter().any(|(name, account)| {
            name != &username && same_address(account.security.email.as_deref(), &address)
        }) {
            return Err("This email address is not available for verification.".into());
        }
        let mut updated = self.clone();
        let data = &mut updated.accounts.get_mut(&username).ok_or(ERROR)?.security;
        data.email = Some(address);
        data.pending_email = None;
        data.email_reset = None;
        self.commit(updated)
    }

    pub fn request_email_reset(
        &mut self,
        username_or_email: &str,
    ) -> Result<Option<EmailIssue>, String> {
        let Some(username) = self.verified_email_account(username_or_email) else {
            return Ok(None);
        };
        let data = &self.accounts[&username].security;
        let now = unix_seconds();
        if in_cooldown(data.reset_sent_at, now) {
            return Ok(None);
        }
        let address = data.email.clone().ok_or("Verified email unavailable.")?;
        let (record, issue) = EmailCode::issue(address, now)?;
        let mut updated = self.clone();
        let data = &mut updated
            .accounts
            .get_mut(&username)
            .ok_or("Account unavailable.")?
            .security;
        data.email_reset = Some(record);
        data.reset_sent_at = now;
        self.commit(updated)?;
        Ok(Some(issue))
    }

    pub fn reset_by_email(
        &mut self,
        username_or_email: &str,
        code: &str,
        new_password: &str,
        otp: &str,
        device: SessionDevice,
    ) -> Result<AuthGrant, String> {
        let username = self
            .verified_email_account(username_or_email)
            .ok_or(EMAIL_RESET_ERROR)?;
        let now = unix_seconds();
        let data = &self.accounts[&username].security;
        if !data.email_reset.as_ref().is_some_and(|pending| {
            data.email.as_deref() == Some(pending.address.as_str()) && pending.matches(code, now)
        }) {
            return Err(EMAIL_RESET_ERROR.into());
        }
        let counter = self.verify_active_factor(&username, otp, now)?;
        self.replace_credentials(username, new_password, counter, device)
    }

    fn verified_email_account(&self, key: &str) -> Option<String> {
        if let Some(address) = checked_address(key) {
            return self
                .accounts
                .iter()
                .find(|(_, account)| same_address(account.security.email.as_deref(), &address))
                .map(|(username, _)| username.clone());
        }
        let username = checked_username(key).ok()?;
        self.accounts.get(&username)?.security.email.as_ref()?;
        Some(username)
    }

    pub fn start_two_factor(&mut self, username: &str, password: &str) -> Result<String, String> {
        let username = self.verify_password(username, password)?;
        if self.accounts[&username].security.factor.is_some() {
            return Err("Two-factor authentication is already enabled.".into());
        }
        let secret = totp::generate_secret()?;
        let mut updated = self.clone();
        let data = &mut updated
            .accounts
            .get_mut(&username)
            .ok_or("Account unavailable.")?
            .security;
        data.pending_factor = Some(FactorSetup {
            secret: secret.clone(),
            expires_at: unix_seconds().saturating_add(SETUP_LIFETIME),
        });
        self.commit(updated)?;
        Ok(secret)
    }

    pub fn confirm_two_factor(
        &mut self,
        username: &str,
        otp: &str,
        device: SessionDevice,
    ) -> Result<AuthGrant, String> {
        const ERROR: &str = "Authenticator setup expired. Start setup again.";
        let username = checked_username(username).map_err(|_| ERROR)?;
        let now = unix_seconds();
        let data = &self.accounts.get(&username).ok_or(ERROR)?.security;
        let setup = data
            .pending_factor
            .as_ref()
            .filter(|item| item.expires_at > now && data.factor.is_none())
            .ok_or(ERROR)?;
        let secret = setup.secret.clone();
        let counter =
            self.check_otp_attempt(&username, totp::match_counter(&secret, otp, now, None), now)?;
        let mut updated = self.clone();
        let data = &mut updated.accounts.get_mut(&username).ok_or(ERROR)?.security;
        data.invalidate_pending();
        data.factor = Some(Factor {
            secret,
            last_counter: counter,
        });
        updated.revoke_authentication(&username);
        let grant = updated.add_session(&username, None, device)?;
        self.commit(updated)?;
        Ok(grant)
    }

    pub fn disable_two_factor(
        &mut self,
        username: &str,
        password: &str,
        otp: &str,
        device: SessionDevice,
    ) -> Result<AuthGrant, String> {
        let username = self.verify_password(username, password)?;
        if self
            .verify_active_factor(&username, otp, unix_seconds())?
            .is_none()
        {
            return Err("Two-factor authentication is already off.".into());
        }
        let mut updated = self.clone();
        let data = &mut updated
            .accounts
            .get_mut(&username)
            .ok_or("Account unavailable.")?
            .security;
        data.factor = None;
        data.invalidate_pending();
        updated.revoke_authentication(&username);
        let grant = updated.add_session(&username, None, device)?;
        self.commit(updated)?;
        Ok(grant)
    }

    pub(super) fn verify_active_factor(
        &mut self,
        username: &str,
        otp: &str,
        now: u64,
    ) -> Result<Option<u64>, String> {
        let Some(factor) = self
            .accounts
            .get(username)
            .and_then(|account| account.security.factor.as_ref())
        else {
            return Ok(None);
        };
        let result = totp::match_counter(&factor.secret, otp, now, Some(factor.last_counter));
        self.check_otp_attempt(username, result, now).map(Some)
    }

    fn check_otp_attempt(
        &mut self,
        username: &str,
        counter: Option<u64>,
        now: u64,
    ) -> Result<u64, String> {
        self.otp_failures
            .retain(|_, failures| failures.expires_at > now);
        if self
            .otp_failures
            .get(username)
            .is_some_and(|failures| failures.attempts >= OTP_ATTEMPTS)
        {
            return Err(OTP_LIMIT_ERROR.into());
        }
        if let Some(counter) = counter {
            return Ok(counter);
        }
        let failures = self
            .otp_failures
            .entry(username.to_owned())
            .or_insert(OtpFailures {
                attempts: 0,
                expires_at: now.saturating_add(OTP_WINDOW),
            });
        failures.attempts += 1;
        Err(OTP_ERROR.into())
    }

    pub(super) fn validate_security_records(&mut self, now: u64) -> io::Result<()> {
        let mut verified = BTreeSet::new();
        for account in self.accounts.values_mut() {
            let data = &mut account.security;
            if data
                .email
                .as_deref()
                .is_some_and(|address| !verified.insert(address.to_ascii_lowercase()))
                || data
                    .email_reset
                    .as_ref()
                    .is_some_and(|pending| data.email.as_deref() != Some(pending.address.as_str()))
                || data.factor.is_some() && data.pending_factor.is_some()
            {
                return Err(invalid_store());
            }
            data.pending_email = data
                .pending_email
                .take()
                .filter(|pending| pending.expires_at > now);
            data.email_reset = data
                .email_reset
                .take()
                .filter(|pending| pending.expires_at > now);
            data.pending_factor = data
                .pending_factor
                .take()
                .filter(|pending| pending.expires_at > now);
        }
        Ok(())
    }

    pub(super) fn load_security_record(&mut self, fields: &[&str]) -> io::Result<()> {
        let [kind, username, values @ ..] = fields else {
            return Err(invalid_store());
        };
        if !canonical_username(username) {
            return Err(invalid_store());
        }
        let data = &mut self
            .accounts
            .get_mut(*username)
            .ok_or_else(invalid_store)?
            .security;
        match (*kind, values) {
            ("verified-email", [address]) if data.email.is_none() => {
                data.email = Some(canonical_address(address)?);
            }
            (kind @ ("verify-email" | "reset-email"), [address, hash, expiration]) => {
                let slot = if kind == "verify-email" {
                    &mut data.pending_email
                } else {
                    &mut data.email_reset
                };
                if slot.is_some() {
                    return Err(invalid_store());
                }
                *slot = Some(EmailCode {
                    address: canonical_address(address)?,
                    hash: decode_hash(hash).ok_or_else(invalid_store)?,
                    expires_at: parse_number(expiration)?,
                });
            }
            ("email-issuance", [verification, reset])
                if data.verification_sent_at == 0 && data.reset_sent_at == 0 =>
            {
                data.verification_sent_at = verification.parse().map_err(|_| invalid_store())?;
                data.reset_sent_at = reset.parse().map_err(|_| invalid_store())?;
                if data.verification_sent_at == 0 && data.reset_sent_at == 0 {
                    return Err(invalid_store());
                }
            }
            ("totp", [secret, counter]) if data.factor.is_none() && totp::valid_secret(secret) => {
                let last_counter = counter.parse().map_err(|_| invalid_store())?;
                data.factor = Some(Factor {
                    secret: (*secret).into(),
                    last_counter,
                });
            }
            ("totp-setup", [secret, expiration])
                if data.pending_factor.is_none() && totp::valid_secret(secret) =>
            {
                data.pending_factor = Some(FactorSetup {
                    secret: (*secret).into(),
                    expires_at: parse_number(expiration)?,
                });
            }
            _ => return Err(invalid_store()),
        }
        Ok(())
    }
}

impl SecurityData {
    pub(super) fn invalidate_pending(&mut self) {
        self.pending_email = None;
        self.email_reset = None;
        self.pending_factor = None;
    }

    pub(super) fn accept_counter(&mut self, counter: Option<u64>) {
        if let (Some(factor), Some(counter)) = (&mut self.factor, counter) {
            factor.last_counter = counter;
        }
    }

    pub(super) fn write_records(
        &self,
        file: &mut impl Write,
        username: &str,
        now: u64,
    ) -> io::Result<()> {
        if let Some(email) = &self.email {
            writeln!(file, "verified-email\t{username}\t{email}")?;
        }
        if self.verification_sent_at != 0 || self.reset_sent_at != 0 {
            writeln!(
                file,
                "email-issuance\t{username}\t{}\t{}",
                self.verification_sent_at, self.reset_sent_at
            )?;
        }
        for (kind, record) in [
            ("verify-email", &self.pending_email),
            ("reset-email", &self.email_reset),
        ] {
            if let Some(record) = record.as_ref().filter(|record| record.expires_at > now) {
                writeln!(
                    file,
                    "{kind}\t{username}\t{}\t{}\t{}",
                    record.address,
                    hex(&record.hash),
                    record.expires_at
                )?;
            }
        }
        if let Some(factor) = &self.factor {
            writeln!(
                file,
                "totp\t{username}\t{}\t{}",
                factor.secret, factor.last_counter
            )?;
        }
        if let Some(setup) = self
            .pending_factor
            .as_ref()
            .filter(|setup| setup.expires_at > now)
        {
            writeln!(
                file,
                "totp-setup\t{username}\t{}\t{}",
                setup.secret, setup.expires_at
            )?;
        }
        Ok(())
    }
}

fn parse_number(input: &str) -> io::Result<u64> {
    input
        .parse()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(invalid_store)
}

fn canonical_address(value: &str) -> io::Result<String> {
    checked_address(value)
        .filter(|address| address == value)
        .ok_or_else(invalid_store)
}

fn checked_address(value: &str) -> Option<String> {
    let value = value.trim();
    if !valid_address(value) {
        return None;
    }
    let (local, domain) = value.split_once('@')?;
    Some(format!("{local}@{}", domain.to_ascii_lowercase()))
}

fn same_address(stored: Option<&str>, requested: &str) -> bool {
    stored.is_some_and(|address| address.eq_ignore_ascii_case(requested))
}

fn in_cooldown(sent_at: u64, now: u64) -> bool {
    sent_at != 0 && now.saturating_sub(sent_at) < EMAIL_COOLDOWN
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    const USER: &str = "security_test";
    const OTHER: &str = "security_other";
    const PASSWORD: &str = "fixture current password";
    const REPLACEMENT: &str = "fixture replacement password";
    const EMAIL: &str = "Security.Test@example.invalid";

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("commonline-security-{}", random_secret().unwrap()));
            fs::create_dir(&dir).unwrap();
            Self(dir)
        }

        fn load(&self) -> AuthStore {
            AuthStore::load(&self.0.join("site.state")).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.0.join("site.state.accounts"));
            let _ = fs::remove_dir(&self.0);
        }
    }

    fn challenge(auth: &mut AuthStore) -> String {
        let LoginProgress::SecondFactor(challenge) = auth
            .begin_login(USER, PASSWORD, SessionDevice::default())
            .unwrap()
        else {
            panic!("an enrolled account cannot sign in with its password alone");
        };
        challenge
    }

    fn enroll(auth: &mut AuthStore) -> (String, String, AuthGrant) {
        let secret = auth.start_two_factor(USER, PASSWORD).unwrap();
        let previous = unix_seconds() / 30 - 1;
        let previous_code = totp::test_code(&secret, previous);
        let grant = auth
            .confirm_two_factor(USER, &previous_code, SessionDevice::default())
            .unwrap();
        (secret, previous_code, grant)
    }

    #[test]
    fn email_proof_is_one_time_unique_and_keeps_the_verified_mailbox_until_confirmed() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        let signup = auth
            .register(USER, PASSWORD, SessionDevice::default())
            .unwrap();
        let second = auth.login(USER, PASSWORD).unwrap();
        auth.register(OTHER, PASSWORD, SessionDevice::default())
            .unwrap();
        assert!(auth
            .request_email_reset("unknown@example.invalid")
            .unwrap()
            .is_none());
        assert!(auth.request_email_reset(USER).unwrap().is_none());
        assert!(auth
            .request_email_verification(USER, "wrong", "", EMAIL)
            .is_err());

        let issue = auth
            .request_email_verification(USER, PASSWORD, "", EMAIL)
            .unwrap();
        assert_eq!(issue.address, EMAIL);
        assert_eq!(auth.security(USER).unwrap().email, None);
        assert!(auth
            .request_email_verification(USER, PASSWORD, "", EMAIL)
            .is_err());
        auth.verify_email(USER, &issue.code.to_ascii_uppercase())
            .unwrap();
        assert!(auth.verify_email(USER, &issue.code).is_err());
        let rival = auth
            .request_email_verification(OTHER, PASSWORD, "", &EMAIL.to_ascii_lowercase())
            .unwrap();
        assert!(auth.verify_email(OTHER, &rival.code).is_err());
        assert_eq!(auth.security(OTHER).unwrap().email, None);

        auth.accounts
            .get_mut(USER)
            .unwrap()
            .security
            .verification_sent_at = 0;
        let pending = auth
            .request_email_verification(USER, PASSWORD, "", "new@example.invalid")
            .unwrap();
        let reset = auth
            .request_email_reset(&EMAIL.to_ascii_uppercase())
            .unwrap()
            .unwrap();
        assert_eq!(reset.address, EMAIL);
        assert!(auth.request_email_reset(USER).unwrap().is_none());
        assert!(auth
            .request_email_reset(&pending.address)
            .unwrap()
            .is_none());
        let records = fs::read_to_string(&auth.path).unwrap();
        for secret in [&issue.code, &rival.code, &pending.code, &reset.code] {
            assert!(!records.contains(secret));
        }

        let mut auth = fixture.load();
        let security = auth.security(USER).unwrap();
        assert_eq!(security.email, Some(EMAIL));
        assert_eq!(security.pending_email, Some(pending.address.as_str()));
        assert!(auth.request_email_reset(USER).unwrap().is_none());
        let grant = auth
            .reset_by_email(USER, &reset.code, REPLACEMENT, "", SessionDevice::default())
            .unwrap();
        assert_eq!(auth.username(&grant.session), Some(USER));
        assert_eq!(auth.username(&signup.session), None);
        assert_eq!(auth.username(&second.session), None);
        assert!(auth
            .reset_by_email(USER, &reset.code, PASSWORD, "", SessionDevice::default())
            .is_err());
        assert!(auth.verify_email(USER, &pending.code).is_err());
        assert_eq!(auth.security(USER).unwrap().email, Some(EMAIL));
        assert!(auth.login(USER, PASSWORD).is_err());
        assert!(auth.login(USER, REPLACEMENT).is_ok());
    }

    #[test]
    fn email_codes_expire_and_verified_address_changes_invalidate_old_resets() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        auth.register(USER, PASSWORD, SessionDevice::default())
            .unwrap();
        let first = auth
            .request_email_verification(USER, PASSWORD, "", EMAIL)
            .unwrap();
        auth.accounts
            .get_mut(USER)
            .unwrap()
            .security
            .pending_email
            .as_mut()
            .unwrap()
            .expires_at = unix_seconds();
        assert!(auth.verify_email(USER, &first.code).is_err());
        assert_eq!(auth.security(USER).unwrap().pending_email, None);
        auth.accounts
            .get_mut(USER)
            .unwrap()
            .security
            .verification_sent_at = 0;
        let first = auth
            .request_email_verification(USER, PASSWORD, "", EMAIL)
            .unwrap();
        auth.verify_email(USER, &first.code).unwrap();
        let reset = auth.request_email_reset(USER).unwrap().unwrap();
        auth.accounts
            .get_mut(USER)
            .unwrap()
            .security
            .email_reset
            .as_mut()
            .unwrap()
            .expires_at = unix_seconds();
        assert!(auth
            .reset_by_email(USER, &reset.code, REPLACEMENT, "", SessionDevice::default())
            .is_err());
        let data = &mut auth.accounts.get_mut(USER).unwrap().security;
        data.reset_sent_at = 0;
        data.verification_sent_at = 0;
        let reset = auth.request_email_reset(USER).unwrap().unwrap();
        let changed = auth
            .request_email_verification(USER, PASSWORD, "", "updated@example.invalid")
            .unwrap();
        auth.verify_email(USER, &changed.code).unwrap();
        assert!(auth.request_email_reset(EMAIL).unwrap().is_none());
        assert!(auth
            .reset_by_email(USER, &reset.code, REPLACEMENT, "", SessionDevice::default())
            .is_err());
    }

    #[test]
    fn authenticator_enrollment_gates_login_and_replay_remains_blocked_after_restart() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        let signup = auth
            .register(USER, PASSWORD, SessionDevice::default())
            .unwrap();
        let recovery = signup.recovery_code.unwrap();
        let (secret, previous_code, enrolled) = enroll(&mut auth);
        assert_eq!(auth.username(&signup.session), None);
        assert_eq!(auth.username(&enrolled.session), Some(USER));
        assert!(auth.start_two_factor(USER, PASSWORD).is_err());
        assert!(auth.recover(USER, &recovery, REPLACEMENT).is_err());
        assert!(auth
            .change_password(USER, PASSWORD, REPLACEMENT, "", SessionDevice::default())
            .is_err());
        assert!(auth
            .request_email_verification(USER, PASSWORD, "", EMAIL)
            .is_err());

        let attempt = challenge(&mut auth);
        assert!(auth
            .finish_login(&attempt, &previous_code, SessionDevice::default())
            .is_err());
        let current_code = totp::test_code(&secret, unix_seconds() / 30);
        let signed_in = auth
            .finish_login(&attempt, &current_code, SessionDevice::default())
            .unwrap();
        assert_eq!(auth.username(&signed_in.session), Some(USER));
        assert!(auth
            .finish_login(&attempt, &current_code, SessionDevice::default())
            .is_err());
        let pending = challenge(&mut auth);
        let mut auth = fixture.load();
        assert!(auth.security(USER).unwrap().two_factor);
        assert!(auth
            .finish_login(&pending, &current_code, SessionDevice::default())
            .is_err());
        let fresh = challenge(&mut auth);
        assert!(auth
            .finish_login(&fresh, &current_code, SessionDevice::default())
            .is_err());
        assert!(auth.start_two_factor(USER, PASSWORD).is_err());
    }

    #[test]
    fn replacing_login_challenges_does_not_reset_the_account_attempt_limit() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        auth.register(USER, PASSWORD, SessionDevice::default())
            .unwrap();
        let (secret, _, _) = enroll(&mut auth);
        for _ in 0..OTP_ATTEMPTS {
            let old = challenge(&mut auth);
            let new = challenge(&mut auth);
            assert!(auth
                .finish_login(&old, "invalid", SessionDevice::default())
                .is_err());
            assert!(auth
                .finish_login(&new, "invalid", SessionDevice::default())
                .is_err());
        }
        assert_eq!(auth.login_challenges.len(), 1);
        let attempt = challenge(&mut auth);
        let code = totp::test_code(&secret, unix_seconds() / 30);
        match auth.finish_login(&attempt, &code, SessionDevice::default()) {
            Err(error) => assert_eq!(error, OTP_LIMIT_ERROR),
            Ok(_) => panic!("issuing a new challenge bypassed the account attempt limit"),
        }
    }

    #[test]
    fn revoking_other_sessions_discards_a_pending_challenge_without_ending_the_current_session() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        auth.register(USER, PASSWORD, SessionDevice::default())
            .unwrap();
        let (_, _, current) = enroll(&mut auth);
        let pending = challenge(&mut auth);
        assert_eq!(auth.login_challenges.len(), 1);
        assert_eq!(auth.revoke_other_sessions(&current.session).unwrap(), 0);
        assert!(auth.login_challenges.is_empty());
        assert_eq!(auth.username(&current.session), Some(USER));
        assert!(auth
            .finish_login(&pending, "invalid", SessionDevice::default())
            .is_err());
    }

    #[test]
    fn email_reset_requires_the_enrolled_factor_and_revokes_old_sessions() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        auth.register(USER, PASSWORD, SessionDevice::default())
            .unwrap();
        let email = auth
            .request_email_verification(USER, PASSWORD, "", EMAIL)
            .unwrap();
        auth.verify_email(USER, &email.code).unwrap();
        let (secret, _, enrolled) = enroll(&mut auth);
        let reset = auth.request_email_reset(USER).unwrap().unwrap();
        assert!(auth
            .reset_by_email(USER, &reset.code, REPLACEMENT, "", SessionDevice::default())
            .is_err());
        assert!(auth
            .disable_two_factor(USER, PASSWORD, "", SessionDevice::default())
            .is_err());
        let otp = totp::test_code(&secret, unix_seconds() / 30);
        let grant = auth
            .reset_by_email(
                USER,
                &reset.code,
                REPLACEMENT,
                &otp,
                SessionDevice::default(),
            )
            .unwrap();
        let auth = fixture.load();
        assert!(auth.security(USER).unwrap().two_factor);
        assert_eq!(auth.username(&grant.session), Some(USER));
        assert_eq!(auth.username(&enrolled.session), None);
    }
}

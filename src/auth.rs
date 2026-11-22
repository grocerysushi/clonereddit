use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Params,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::http::{valid_session_token, SESSION_COOKIE_NAME, SESSION_MAX_AGE_SECS};

#[path = "auth_deletion.rs"]
mod deletion;
use deletion::AccountDeletions;

#[path = "auth_sessions.rs"]
mod sessions;
use sessions::Session;
pub use sessions::{SessionDevice, SessionSummary};

#[path = "auth_security.rs"]
mod security;
pub use security::{AccountSecurity, LoginProgress};
use security::{LoginChallenge, OtpFailures, SecurityData};

const FORMAT: &str = "commonline-accounts-v1";
const SIGN_IN_ERROR: &str = "The username or password is incorrect.";
const RECOVERY_ERROR: &str = "The username or recovery code is incorrect.";
const PASSWORD_MIN: usize = 12;
const PASSWORD_MAX: usize = 128;
const SECRET_BYTES: usize = 32;

type SecretHash = [u8; SECRET_BYTES];

pub struct AuthGrant {
    pub session: String,
    pub recovery_code: Option<String>,
}

#[derive(Clone)]
struct Account {
    password_hash: String,
    recovery_hash: SecretHash,
    security: SecurityData,
}

#[derive(Clone)]
pub struct AuthStore {
    path: PathBuf,
    accounts: BTreeMap<String, Account>,
    sessions: BTreeMap<SecretHash, Session>,
    legacy_claims: BTreeMap<String, SecretHash>,
    deletions: AccountDeletions,
    login_challenges: BTreeMap<SecretHash, LoginChallenge>,
    otp_failures: BTreeMap<String, OtpFailures>,
    dummy_hash: String,
}

impl AuthStore {
    pub fn load(state_path: &Path) -> io::Result<Self> {
        let mut path = state_path.as_os_str().to_owned();
        path.push(".accounts");
        let path = PathBuf::from(path);
        let dummy_hash =
            password_hash("Commonline invalid account", &[0; 16]).map_err(io::Error::other)?;
        let mut store = Self {
            path,
            accounts: BTreeMap::new(),
            sessions: BTreeMap::new(),
            legacy_claims: BTreeMap::new(),
            deletions: AccountDeletions::default(),
            login_challenges: BTreeMap::new(),
            otp_failures: BTreeMap::new(),
            dummy_hash,
        };
        let file = match File::open(&store.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(store),
            Err(error) => return Err(error),
        };
        let mut lines = BufReader::new(file).lines();
        if lines.next().transpose()?.as_deref() != Some(FORMAT) {
            return Err(invalid_store());
        }
        for line in lines {
            store.load_record(&line?)?;
        }
        if store
            .sessions
            .values()
            .any(|session| !store.accounts.contains_key(&session.username))
            || store
                .legacy_claims
                .keys()
                .any(|username| store.accounts.contains_key(username))
        {
            return Err(invalid_store());
        }
        store
            .deletions
            .validate(store.accounts.keys().chain(store.legacy_claims.keys()))?;
        let now = unix_seconds();
        store.sessions.retain(|_, session| session.expires_at > now);
        store.validate_security_records(now)?;
        Ok(store)
    }

    pub fn register(
        &mut self,
        username: &str,
        password: &str,
        device: SessionDevice,
    ) -> Result<AuthGrant, String> {
        let username = checked_username(username)?;
        if self.accounts.contains_key(&username)
            || self.legacy_claims.contains_key(&username)
            || self.deletions.is_reserved(&username)
        {
            return Err("That username is unavailable.".into());
        }
        self.replace_credentials(username, password, None, device)
    }

    #[cfg(test)]
    fn login(&mut self, username: &str, password: &str) -> Result<AuthGrant, String> {
        match self.begin_login(username, password, SessionDevice::default())? {
            LoginProgress::SignedIn(grant) => Ok(grant),
            LoginProgress::SecondFactor(_) => {
                Err("Complete sign-in with your authenticator.".into())
            }
        }
    }

    fn verify_password(&self, username: &str, password: &str) -> Result<String, &'static str> {
        if !valid_password(password) {
            return Err(SIGN_IN_ERROR);
        }
        let username = checked_username(username).ok();
        let account = username.as_ref().and_then(|name| self.accounts.get(name));
        let hash = account.map_or(self.dummy_hash.as_str(), |account| &account.password_hash);
        let verified = PasswordHash::new(hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        });
        if !verified || account.is_none() {
            return Err(SIGN_IN_ERROR);
        }
        username.ok_or(SIGN_IN_ERROR)
    }

    #[cfg(test)]
    fn recover(
        &mut self,
        username: &str,
        recovery_code: &str,
        new_password: &str,
    ) -> Result<AuthGrant, String> {
        self.recover_with_otp(
            username,
            recovery_code,
            new_password,
            "",
            SessionDevice::default(),
        )
    }

    pub fn recover_with_otp(
        &mut self,
        username: &str,
        recovery_code: &str,
        new_password: &str,
        otp: &str,
        device: SessionDevice,
    ) -> Result<AuthGrant, String> {
        let username = checked_username(username).map_err(|_| RECOVERY_ERROR)?;
        if self.deletions.is_reserved(&username) {
            return Err(RECOVERY_ERROR.into());
        }
        let code = recovery_code.trim().to_ascii_lowercase();
        if !valid_session_token(&code) {
            return Err(RECOVERY_ERROR.into());
        }
        let supplied = secret_hash(&code);
        let expected = self
            .accounts
            .get(&username)
            .map(|account| &account.recovery_hash)
            .or_else(|| self.legacy_claims.get(&username));
        if !expected.is_some_and(|expected| bool::from(expected.ct_eq(&supplied))) {
            return Err(RECOVERY_ERROR.into());
        }
        let counter = self.verify_active_factor(&username, otp, unix_seconds())?;
        self.replace_credentials(username, new_password, counter, device)
    }

    pub fn logout(&mut self, token: &str) -> Result<(), String> {
        if !valid_session_token(token) {
            return Ok(());
        }
        let hash = secret_hash(token);
        if !self.sessions.contains_key(&hash) {
            return Ok(());
        }
        let mut updated = self.clone();
        updated.sessions.remove(&hash);
        self.commit(updated)
    }

    pub fn username(&self, token: &str) -> Option<&str> {
        self.active_session(token, unix_seconds())
            .map(|(_, session)| session.username.as_str())
    }

    pub fn has_account(&self, username: &str) -> bool {
        checked_username(username).is_ok_and(|name| self.accounts.contains_key(&name))
    }

    pub fn usernames(&self) -> impl Iterator<Item = &str> {
        self.accounts.keys().map(String::as_str)
    }

    /// The operator caller verifies this handle against the existing local site's owner.
    pub fn issue_legacy_recovery(&mut self, username: &str) -> Result<String, String> {
        let username = checked_username(username)?;
        if self.deletions.is_reserved(&username) {
            return Err("That username is unavailable.".into());
        }
        if self.accounts.contains_key(&username) {
            return Err("This account is already registered. Use its recovery code.".into());
        }
        let code = random_secret()?;
        let mut updated = self.clone();
        updated.legacy_claims.insert(username, secret_hash(&code));
        self.commit(updated)?;
        Ok(code)
    }

    fn replace_credentials(
        &mut self,
        username: String,
        password: &str,
        counter: Option<u64>,
        device: SessionDevice,
    ) -> Result<AuthGrant, String> {
        if !valid_password(password) {
            return Err("Use a password with 12 to 128 characters.".into());
        }
        let mut salt = [0; 16];
        getrandom::getrandom(&mut salt)
            .map_err(|_| "Could not generate a password salt. Try again.")?;
        let code = random_secret()?;
        let mut updated = self.clone();
        let password_hash = password_hash(password, &salt)?;
        let recovery_hash = secret_hash(&code);
        if let Some(account) = updated.accounts.get_mut(&username) {
            account.password_hash = password_hash;
            account.recovery_hash = recovery_hash;
            account.security.accept_counter(counter);
            account.security.invalidate_pending();
        } else {
            updated.accounts.insert(
                username.clone(),
                Account {
                    password_hash,
                    recovery_hash,
                    security: SecurityData::default(),
                },
            );
        }
        updated.legacy_claims.remove(&username);
        updated.revoke_authentication(&username);
        let grant = updated.add_session(&username, Some(code), device)?;
        self.commit(updated)?;
        Ok(grant)
    }

    fn revoke_authentication(&mut self, username: &str) {
        self.sessions
            .retain(|_, session| session.username != username);
        self.login_challenges
            .retain(|_, challenge| challenge.username != username);
        self.otp_failures.remove(username);
    }

    fn add_session(
        &mut self,
        username: &str,
        recovery_code: Option<String>,
        device: SessionDevice,
    ) -> Result<AuthGrant, String> {
        let session = random_secret()?;
        let hash = secret_hash(&session);
        let now = unix_seconds();
        self.sessions.retain(|_, record| record.expires_at > now);
        if self.sessions.contains_key(&hash) {
            return Err("Could not create a distinct session. Try again.".into());
        }
        self.sessions.insert(
            hash,
            Session {
                username: username.to_owned(),
                expires_at: now.saturating_add(SESSION_MAX_AGE_SECS),
                created_at: Some(now),
                device,
            },
        );
        Ok(AuthGrant {
            session,
            recovery_code,
        })
    }

    fn load_record(&mut self, line: &str) -> io::Result<()> {
        let pieces: Vec<_> = line.split('\t').collect();
        match pieces.as_slice() {
            ["account", username, password_hash, recovery_hash]
                if canonical_username(username) && checked_password_hash(password_hash) =>
            {
                let recovery_hash = decode_hash(recovery_hash).ok_or_else(invalid_store)?;
                let old = self.accounts.insert(
                    (*username).into(),
                    Account {
                        password_hash: (*password_hash).into(),
                        recovery_hash,
                        security: SecurityData::default(),
                    },
                );
                if old.is_some() {
                    return Err(invalid_store());
                }
            }
            ["session", hash, username, expiration, metadata @ ..]
                if canonical_username(username) =>
            {
                let hash = decode_hash(hash).ok_or_else(invalid_store)?;
                let session = Session::from_record(username, expiration, metadata)?;
                let old = self.sessions.insert(hash, session);
                if old.is_some() {
                    return Err(invalid_store());
                }
            }
            ["legacy-recovery", username, hash] if canonical_username(username) => {
                let hash = decode_hash(hash).ok_or_else(invalid_store)?;
                if self
                    .legacy_claims
                    .insert((*username).into(), hash)
                    .is_some()
                {
                    return Err(invalid_store());
                }
            }
            fields @ (["deleted-handle", ..] | ["pending-account-deletion", ..]) => {
                self.deletions.read_record(fields)?
            }
            _ => self.load_security_record(&pieces)?,
        }
        Ok(())
    }

    fn commit(&mut self, updated: Self) -> Result<(), String> {
        updated
            .save()
            .map_err(|error| format!("Could not save accounts. {error}"))?;
        *self = updated;
        Ok(())
    }

    fn save(&self) -> io::Result<()> {
        static TEMP_ID: AtomicU64 = AtomicU64::new(0);
        let mut temp_path = self.path.as_os_str().to_owned();
        temp_path.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let temp_path = PathBuf::from(temp_path);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path)?;
        let result = self.write_records(&mut file).and_then(|()| file.sync_all());
        drop(file);
        let result = result.and_then(|()| fs::rename(&temp_path, &self.path));
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }

    fn write_records(&self, file: &mut impl Write) -> io::Result<()> {
        writeln!(file, "{FORMAT}")?;
        let now = unix_seconds();
        for (username, account) in &self.accounts {
            writeln!(
                file,
                "account\t{username}\t{}\t{}",
                account.password_hash,
                hex(&account.recovery_hash)
            )?;
            account.security.write_records(file, username, now)?;
        }
        for (hash, session) in self
            .sessions
            .iter()
            .filter(|(_, session)| session.expires_at > now)
        {
            session.write_record(file, hash)?;
        }
        for (username, hash) in &self.legacy_claims {
            writeln!(file, "legacy-recovery\t{username}\t{}", hex(hash))?;
        }
        self.deletions.write_records(file)
    }
}

pub fn session_token(cookie: &str) -> Option<&str> {
    let mut session = None;
    for part in cookie.split(';') {
        let part = part.trim_matches([' ', '\t']);
        let Some((name, value)) = part.split_once('=') else {
            if part == SESSION_COOKIE_NAME {
                return None;
            }
            continue;
        };
        if name.trim_matches([' ', '\t']) == SESSION_COOKIE_NAME {
            let value = value.trim_matches([' ', '\t']);
            if session.is_some() || !valid_session_token(value) {
                return None;
            }
            session = Some(value);
        }
    }
    session
}

fn checked_username(username: &str) -> Result<String, &'static str> {
    let username = username.trim().to_ascii_lowercase();
    if !canonical_username(&username) {
        return Err("Use a username with 2 to 24 letters, numbers, or underscores.");
    }
    Ok(username)
}

fn canonical_username(username: &str) -> bool {
    (2..=24).contains(&username.len())
        && username
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_password(password: &str) -> bool {
    password.len() <= 4 * PASSWORD_MAX
        && (PASSWORD_MIN..=PASSWORD_MAX).contains(&password.chars().count())
}

fn password_hash(password: &str, salt: &[u8; 16]) -> Result<String, &'static str> {
    let salt = SaltString::encode_b64(salt).map_err(|_| "Could not encode the password salt.")?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "Could not protect the password. Try again.")
}

fn checked_password_hash(value: &str) -> bool {
    PasswordHash::new(value).is_ok_and(|hash| {
        hash.algorithm.as_str() == "argon2id"
            && hash.version == Some(19)
            && hash.salt.is_some()
            && hash
                .hash
                .is_some_and(|output| output.len() == Params::DEFAULT_OUTPUT_LEN)
            && hash.params.iter().count() == 3
            && hash.params.get_decimal("m") == Some(Params::DEFAULT_M_COST)
            && hash.params.get_decimal("t") == Some(Params::DEFAULT_T_COST)
            && hash.params.get_decimal("p") == Some(Params::DEFAULT_P_COST)
    })
}

fn random_secret() -> Result<String, &'static str> {
    let mut bytes = [0; SECRET_BYTES];
    getrandom::getrandom(&mut bytes)
        .map_err(|_| "Could not generate secure credentials. Try again.")?;
    Ok(hex(&bytes))
}

fn secret_hash(value: &str) -> SecretHash {
    Sha256::digest(value.as_bytes()).into()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    encoded
}

fn decode_hash(value: &str) -> Option<SecretHash> {
    if !valid_session_token(value) {
        return None;
    }
    let mut bytes = [0; SECRET_BYTES];
    for (target, pair) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let high = char::from(pair[0]).to_digit(16)?;
        let low = char::from(pair[1]).to_digit(16)?;
        *target = ((high << 4) | low) as u8;
    }
    Some(bytes)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn invalid_store() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "The accounts file contains invalid records.",
    )
}

#[cfg(test)]
mod tests {
    use super::{random_secret, AuthStore, SessionDevice};
    use std::{fs, path::PathBuf};

    #[test]
    fn recovery_rotates_credentials_and_revokes_prior_sessions_after_reload() {
        const USER: &str = "recover_test";
        const OLD_PASSWORD: &str = "first fixture password";
        const NEW_PASSWORD: &str = "replacement fixture password";

        struct TempState(PathBuf);
        impl Drop for TempState {
            fn drop(&mut self) {
                let _ = fs::remove_file(self.0.join("site.state.accounts"));
                let _ = fs::remove_dir(&self.0);
            }
        }

        let folder =
            std::env::temp_dir().join(format!("commonline-recovery-{}", random_secret().unwrap()));
        fs::create_dir(&folder).unwrap();
        let fixture = TempState(folder);
        let state = fixture.0.join("site.state");
        let mut auth = AuthStore::load(&state).unwrap();
        let signup = auth
            .register(USER, OLD_PASSWORD, SessionDevice::default())
            .unwrap();
        let second_login = auth.login(USER, OLD_PASSWORD).unwrap();
        let old_code = signup.recovery_code.unwrap();

        let recovered = auth.recover(USER, &old_code, NEW_PASSWORD).unwrap();
        let new_code = recovered.recovery_code.unwrap();
        assert!(new_code != old_code);
        drop(auth);

        let mut reloaded = AuthStore::load(&state).unwrap();
        assert_eq!(reloaded.username(&signup.session), None);
        assert_eq!(reloaded.username(&second_login.session), None);
        assert_eq!(reloaded.username(&recovered.session), Some(USER));
        assert!(reloaded.login(USER, OLD_PASSWORD).is_err());
        assert!(reloaded.login(USER, NEW_PASSWORD).is_ok());
        assert!(reloaded.recover(USER, &old_code, OLD_PASSWORD).is_err());
        assert!(reloaded.recover(USER, &new_code, OLD_PASSWORD).is_ok());
        assert!(reloaded.recover(USER, &new_code, NEW_PASSWORD).is_err());
    }
}

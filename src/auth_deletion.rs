use std::{
    collections::BTreeSet,
    io::{self, Write},
};

use sha2::{Digest, Sha256};

use super::{
    canonical_username, decode_hash, hex, invalid_store, unix_seconds, AuthStore, SecretHash,
};

#[derive(Clone, Default)]
pub(super) struct AccountDeletions {
    reserved: BTreeSet<SecretHash>,
    pending: BTreeSet<String>,
}

impl AuthStore {
    pub fn begin_account_deletion(
        &mut self,
        current_token: &str,
        password: &str,
        otp: &str,
    ) -> Result<(), String> {
        let (_, session) = self
            .active_session(current_token, unix_seconds())
            .ok_or("Sign in to delete your account.")?;
        let username = self.verify_password(&session.username, password)?;
        self.verify_active_factor(&username, otp, unix_seconds())?;

        let mut updated = self.clone();
        updated.accounts.remove(&username);
        updated.legacy_claims.remove(&username);
        updated.revoke_authentication(&username);
        updated.deletions.reserved.insert(handle_digest(&username));
        updated.deletions.pending.insert(username);
        self.commit(updated)
    }

    pub fn pending_account_deletions(&self) -> impl Iterator<Item = &str> {
        self.deletions.pending.iter().map(String::as_str)
    }

    pub fn finish_account_deletions(&mut self, handles: &[String]) -> Result<(), String> {
        let mut updated = self.clone();
        let mut changed = false;
        for handle in handles {
            changed |= updated.deletions.pending.remove(handle);
        }
        if changed {
            self.commit(updated)?;
        }
        Ok(())
    }
}

impl AccountDeletions {
    pub(super) fn is_reserved(&self, username: &str) -> bool {
        self.reserved.contains(&handle_digest(username))
    }

    pub(super) fn validate<'a>(
        &self,
        mut active: impl Iterator<Item = &'a String>,
    ) -> io::Result<()> {
        if active.any(|username| self.is_reserved(username))
            || self
                .pending
                .iter()
                .any(|username| !self.is_reserved(username))
        {
            return Err(invalid_store());
        }
        Ok(())
    }

    pub(super) fn read_record(&mut self, fields: &[&str]) -> io::Result<()> {
        let inserted = match fields {
            ["deleted-handle", digest] => self
                .reserved
                .insert(decode_hash(digest).ok_or_else(invalid_store)?),
            ["pending-account-deletion", username] if canonical_username(username) => {
                self.pending.insert((*username).into())
            }
            _ => false,
        };
        if !inserted {
            return Err(invalid_store());
        }
        Ok(())
    }

    pub(super) fn write_records(&self, file: &mut impl Write) -> io::Result<()> {
        for digest in &self.reserved {
            writeln!(file, "deleted-handle\t{}", hex(digest))?;
        }
        for username in &self.pending {
            writeln!(file, "pending-account-deletion\t{username}")?;
        }
        Ok(())
    }
}

fn handle_digest(username: &str) -> SecretHash {
    let mut digest = Sha256::new();
    digest.update(b"commonline-deleted-handle-v1\0");
    digest.update(username.as_bytes());
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::{random_secret, AuthGrant, LoginProgress, SessionDevice},
        totp,
    };
    use std::{fs, path::PathBuf};

    const USER: &str = "deletion_subject";
    const OTHER: &str = "deletion_other";
    const PASSWORD: &str = "fixture deletion password";
    const OTHER_PASSWORD: &str = "another fixture password";
    const EMAIL: &str = "deleted@example.invalid";

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir()
                .join(format!("commonline-deletion-{}", random_secret().unwrap()));
            fs::create_dir(&directory).unwrap();
            Self(directory)
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

    fn sign_in(auth: &mut AuthStore) -> AuthGrant {
        let LoginProgress::SignedIn(grant) = auth
            .begin_login(USER, PASSWORD, SessionDevice::default())
            .unwrap()
        else {
            panic!("test account should have no authenticator yet");
        };
        grant
    }

    #[test]
    fn deletion_removes_credentials_revokes_sessions_and_reserves_the_handle_across_restarts() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        let signup = auth
            .register(USER, PASSWORD, SessionDevice::default())
            .unwrap();
        let other = auth
            .register(OTHER, OTHER_PASSWORD, SessionDevice::default())
            .unwrap();
        let second = sign_in(&mut auth);
        let email = auth
            .request_email_verification(USER, PASSWORD, "", EMAIL)
            .unwrap();
        auth.verify_email(USER, &email.code).unwrap();
        let reset = auth.request_email_reset(USER).unwrap().unwrap();

        assert!(auth
            .begin_account_deletion("not-a-cookie", PASSWORD, "")
            .is_err());
        assert!(auth
            .begin_account_deletion(&other.session, PASSWORD, "")
            .is_err());
        assert!(auth
            .begin_account_deletion(&signup.session, OTHER_PASSWORD, "")
            .is_err());
        assert_eq!(auth.username(&signup.session), Some(USER));
        assert_eq!(auth.pending_account_deletions().count(), 0);
        auth.begin_account_deletion(&signup.session, PASSWORD, "")
            .unwrap();
        assert!(auth
            .begin_account_deletion(&signup.session, PASSWORD, "")
            .is_err());
        assert!(auth
            .register(
                &USER.to_ascii_uppercase(),
                PASSWORD,
                SessionDevice::default()
            )
            .is_err());
        let recovery = signup.recovery_code.unwrap();
        assert!(auth
            .recover_with_otp(USER, &recovery, PASSWORD, "", SessionDevice::default())
            .is_err());
        assert!(auth
            .reset_by_email(USER, &reset.code, PASSWORD, "", SessionDevice::default())
            .is_err());
        assert!(auth.issue_legacy_recovery(USER).is_err());

        let mut auth = fixture.load();
        assert_eq!(auth.pending_account_deletions().collect::<Vec<_>>(), [USER]);
        assert_eq!(auth.username(&signup.session), None);
        assert_eq!(auth.username(&second.session), None);
        assert_eq!(auth.username(&other.session), Some(OTHER));
        assert!(auth
            .begin_login(USER, PASSWORD, SessionDevice::default())
            .is_err());
        assert!(auth.request_email_reset(EMAIL).unwrap().is_none());
        let pending = [USER.to_owned()];
        auth.finish_account_deletions(&pending).unwrap();
        auth.finish_account_deletions(&pending).unwrap();
        let mut auth = fixture.load();
        assert_eq!(auth.pending_account_deletions().count(), 0);
        assert!(auth
            .register(USER, PASSWORD, SessionDevice::default())
            .is_err());
        assert!(auth.issue_legacy_recovery(USER).is_err());
        let records = fs::read_to_string(&auth.path).unwrap();
        assert!(!records.contains(USER));
        assert!(!records.contains(EMAIL));
        assert!(records.contains(&format!("deleted-handle\t{}", hex(&handle_digest(USER)))));
    }

    #[test]
    fn deleting_a_two_factor_account_requires_a_fresh_code_and_invalidates_pending_login() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        auth.register(USER, PASSWORD, SessionDevice::default())
            .unwrap();
        let secret = auth.start_two_factor(USER, PASSWORD).unwrap();
        let old_otp = totp::test_code(&secret, unix_seconds() / 30 - 1);
        let current = auth
            .confirm_two_factor(USER, &old_otp, SessionDevice::default())
            .unwrap();
        let LoginProgress::SecondFactor(pending) = auth
            .begin_login(USER, PASSWORD, SessionDevice::default())
            .unwrap()
        else {
            panic!("expected a pending authenticator challenge");
        };
        assert!(auth
            .begin_account_deletion(&current.session, PASSWORD, "")
            .is_err());
        assert!(auth
            .begin_account_deletion(&current.session, PASSWORD, &old_otp)
            .is_err());
        assert!(auth.has_account(USER));
        let fresh_otp = totp::test_code(&secret, unix_seconds() / 30);
        auth.begin_account_deletion(&current.session, PASSWORD, &fresh_otp)
            .unwrap();
        assert!(auth.login_challenges.is_empty());
        assert!(auth
            .finish_login(&pending, &fresh_otp, SessionDevice::default())
            .is_err());
        assert_eq!(auth.username(&current.session), None);
        assert!(!fs::read_to_string(&auth.path).unwrap().contains(&secret));
    }

    #[test]
    fn deletion_records_cannot_overlap_an_account_or_omit_its_permanent_reservation() {
        let mut deletions = AccountDeletions::default();
        assert!(deletions
            .read_record(&["pending-account-deletion", "Invalid"])
            .is_err());
        deletions
            .read_record(&["pending-account-deletion", USER])
            .unwrap();
        assert!(deletions.validate(std::iter::empty()).is_err());
        let digest = hex(&handle_digest(USER));
        deletions.read_record(&["deleted-handle", &digest]).unwrap();
        assert!(deletions.read_record(&["deleted-handle", &digest]).is_err());
        assert!(deletions
            .validate(std::iter::once(&USER.to_owned()))
            .is_err());
        assert!(deletions.validate(std::iter::empty()).is_ok());
    }
}

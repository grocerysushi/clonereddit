use std::io::{self, Write};

use sha2::{Digest, Sha256};

use super::{hex, invalid_store, secret_hash, unix_seconds, AuthStore, SecretHash};
use crate::http::valid_session_token;

const INACTIVE_SESSION: &str = "Sign in to manage your sessions.";
const MISSING_SESSION: &str = "This session is unavailable.";
const BROWSERS: &[&str] = &[
    "",
    "Chrome",
    "Chromium",
    "Edge",
    "Firefox",
    "Opera",
    "Safari",
    "Samsung Internet",
];
const PLATFORMS: &[&str] = &[
    "", "Android", "ChromeOS", "iOS", "Linux", "macOS", "Windows",
];

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub struct SessionDevice {
    browser: &'static str,
    platform: &'static str,
}

impl SessionDevice {
    pub fn from_user_agent(user_agent: &str) -> Self {
        let user_agent = user_agent
            .chars()
            .take(1024)
            .collect::<String>()
            .to_ascii_lowercase();
        let contains_any = |parts: &[&str]| parts.iter().any(|part| user_agent.contains(part));
        let platform = if contains_any(&["iphone", "ipad", "ipod"])
            || user_agent.contains("macintosh") && user_agent.contains("mobile/")
        {
            "iOS"
        } else if user_agent.contains("android") {
            "Android"
        } else if user_agent.contains("cros ") {
            "ChromeOS"
        } else if contains_any(&["macintosh", "mac os x"]) {
            "macOS"
        } else if user_agent.contains("windows") {
            "Windows"
        } else if user_agent.contains("linux") {
            "Linux"
        } else {
            ""
        };
        let browser = if contains_any(&["edg/", "edga/", "edgios/", "edge/"]) {
            "Edge"
        } else if contains_any(&["opr/", "opera/", "opios/"]) {
            "Opera"
        } else if user_agent.contains("samsungbrowser/") {
            "Samsung Internet"
        } else if contains_any(&["firefox/", "fxios/"]) {
            "Firefox"
        } else if contains_any(&["chrome/", "crios/"]) {
            "Chrome"
        } else if user_agent.contains("chromium/") {
            "Chromium"
        } else if user_agent.contains("safari/") && matches!(platform, "iOS" | "macOS") {
            "Safari"
        } else {
            ""
        };
        Self { browser, platform }
    }

    fn from_record(browser: &str, platform: &str) -> Option<Self> {
        Some(Self {
            browser: BROWSERS.iter().copied().find(|label| *label == browser)?,
            platform: PLATFORMS.iter().copied().find(|label| *label == platform)?,
        })
    }

    fn description(self) -> String {
        match (self.browser, self.platform) {
            ("", "") => "Device unavailable".into(),
            (browser, "") => browser.into(),
            ("", platform) => format!("Browser on {platform}"),
            (browser, platform) => format!("{browser} on {platform}"),
        }
    }
}

#[derive(Clone)]
pub(super) struct Session {
    pub(super) username: String,
    pub(super) expires_at: u64,
    pub(super) created_at: Option<u64>,
    pub(super) device: SessionDevice,
}

impl Session {
    pub(super) fn from_record(
        username: &str,
        expiration: &str,
        metadata: &[&str],
    ) -> io::Result<Self> {
        let expires_at = expiration.parse::<u64>().map_err(|_| invalid_store())?;
        let (created_at, device) = match metadata {
            [] => (None, SessionDevice::default()),
            [created, browser, platform] => {
                let created = created.parse::<u64>().map_err(|_| invalid_store())?;
                if created >= expires_at {
                    return Err(invalid_store());
                }
                (
                    Some(created),
                    SessionDevice::from_record(browser, platform).ok_or_else(invalid_store)?,
                )
            }
            _ => return Err(invalid_store()),
        };
        Ok(Self {
            username: username.into(),
            expires_at,
            created_at,
            device,
        })
    }

    pub(super) fn write_record(&self, file: &mut impl Write, hash: &SecretHash) -> io::Result<()> {
        write!(
            file,
            "session\t{}\t{}\t{}",
            hex(hash),
            self.username,
            self.expires_at
        )?;
        if let Some(created_at) = self.created_at {
            write!(
                file,
                "\t{created_at}\t{}\t{}",
                self.device.browser, self.device.platform
            )?;
        }
        writeln!(file)
    }
}

pub struct SessionSummary {
    pub id: String,
    pub device: String,
    pub created_at: Option<u64>,
    pub expires_at: u64,
    pub current: bool,
}

impl AuthStore {
    pub(super) fn active_session(&self, token: &str, now: u64) -> Option<(SecretHash, &Session)> {
        if !valid_session_token(token) {
            return None;
        }
        let hash = secret_hash(token);
        let session = self.sessions.get(&hash)?;
        (session.expires_at > now).then_some((hash, session))
    }

    pub fn active_sessions(&self, current_token: &str) -> Option<Vec<SessionSummary>> {
        let now = unix_seconds();
        let (current_hash, current) = self.active_session(current_token, now)?;
        let mut sessions: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, item)| item.username == current.username && item.expires_at > now)
            .map(|(hash, item)| SessionSummary {
                id: session_id(hash),
                device: item.device.description(),
                created_at: item.created_at,
                expires_at: item.expires_at,
                current: *hash == current_hash,
            })
            .collect();
        sessions.sort_by(|a, b| {
            b.current
                .cmp(&a.current)
                .then_with(|| b.created_at.cmp(&a.created_at))
                .then_with(|| b.expires_at.cmp(&a.expires_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        Some(sessions)
    }

    pub fn revoke_session(&mut self, current_token: &str, id: &str) -> Result<(), String> {
        let now = unix_seconds();
        let (current_hash, current) = self
            .active_session(current_token, now)
            .ok_or(INACTIVE_SESSION)?;
        if !valid_session_token(id) {
            return Err(MISSING_SESSION.into());
        }
        let (target_hash, _) = self
            .sessions
            .iter()
            .find(|(hash, item)| {
                item.username == current.username && item.expires_at > now && session_id(hash) == id
            })
            .ok_or(MISSING_SESSION)?;
        if *target_hash == current_hash {
            return Err("Use Sign out to end your current session.".into());
        }
        let mut updated = self.clone();
        updated.sessions.remove(target_hash);
        self.commit(updated)
    }

    pub fn revoke_other_sessions(&mut self, current_token: &str) -> Result<usize, String> {
        let now = unix_seconds();
        let (current_hash, current) = self
            .active_session(current_token, now)
            .ok_or(INACTIVE_SESSION)?;
        let mut revoked = 0;
        let mut updated = self.clone();
        updated.sessions.retain(|hash, item| {
            let keep = item.username != current.username || *hash == current_hash;
            revoked += usize::from(!keep && item.expires_at > now);
            keep
        });
        let pending = updated.login_challenges.len();
        updated
            .login_challenges
            .retain(|_, challenge| challenge.username != current.username);
        if revoked > 0 || updated.login_challenges.len() != pending {
            self.commit(updated)?;
        }
        Ok(revoked)
    }
}

fn session_id(hash: &SecretHash) -> String {
    let mut digest = Sha256::new();
    digest.update(b"commonline-session-control-id-v1\0");
    digest.update(hash);
    hex(&digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    use crate::auth::{random_secret, AuthGrant, LoginProgress};

    const USER: &str = "session_owner";
    const OTHER: &str = "session_other";
    const PASSWORD: &str = "fixture session password";
    const UA: &str = "Mozilla/5.0 (Windows NT 10.0; UA fixture unique suffix) Chrome/151.0";
    const PHONE: &str =
        "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) Version/18.0 Safari/605.1.15";

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("commonline-sessions-{}", random_secret().unwrap()));
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

    fn sign_in(auth: &mut AuthStore, user: &str, device: SessionDevice) -> AuthGrant {
        let LoginProgress::SignedIn(grant) = auth.begin_login(user, PASSWORD, device).unwrap()
        else {
            panic!("expected this test account to sign in without TOTP");
        };
        grant
    }

    #[test]
    fn session_controls_are_scoped_and_persist_without_exposing_tokens_or_raw_devices() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        let first = auth
            .register(USER, PASSWORD, SessionDevice::from_user_agent(UA))
            .unwrap();
        let second = sign_in(&mut auth, USER, SessionDevice::from_user_agent(PHONE));
        let third = sign_in(&mut auth, USER, SessionDevice::default());
        let other = auth
            .register(OTHER, PASSWORD, SessionDevice::default())
            .unwrap();
        let mut auth = fixture.load();
        let list = auth.active_sessions(&first.session).unwrap();
        assert_eq!(list.len(), 3);
        assert!(list[0].current);
        assert_eq!(list[0].device, "Chrome on Windows");
        assert!(list.iter().all(|item| item
            .created_at
            .is_some_and(|created| item.expires_at > created)));
        let mobile = list
            .iter()
            .find(|item| item.device == "Safari on iOS")
            .unwrap();
        let mobile_hash = hex(&secret_hash(&second.session));
        assert_ne!(mobile.id, second.session);
        assert_ne!(mobile.id, mobile_hash);
        let other_id = &auth.active_sessions(&other.session).unwrap()[0].id;
        assert_eq!(auth.active_sessions(&other.session).unwrap().len(), 1);
        assert!(auth.active_sessions("invalid").is_none());
        assert!(auth.revoke_session(&first.session, other_id).is_err());
        assert!(auth.revoke_session(&other.session, &mobile.id).is_err());
        assert!(auth.revoke_session(&first.session, &list[0].id).is_err());
        assert!(auth
            .revoke_session(&first.session, &second.session)
            .is_err());
        assert!(auth.revoke_session(&first.session, &mobile_hash).is_err());
        let records = fs::read_to_string(&auth.path).unwrap();
        assert!(!records.contains(&second.session));
        assert!(!records.contains("UA fixture unique suffix"));
        assert!(!records.contains("151.0"));

        auth.revoke_session(&first.session, &mobile.id).unwrap();
        assert!(auth.revoke_session(&first.session, &mobile.id).is_err());
        assert_eq!(auth.revoke_other_sessions(&first.session).unwrap(), 1);
        let auth = fixture.load();
        assert_eq!(auth.username(&second.session), None);
        assert_eq!(auth.username(&third.session), None);
        assert_eq!(auth.username(&first.session), Some(USER));
        assert_eq!(auth.username(&other.session), Some(OTHER));
        assert_eq!(auth.active_sessions(&first.session).unwrap().len(), 1);
    }

    #[test]
    fn the_old_session_record_remains_active_without_invented_device_or_sign_in_time() {
        let fixture = Fixture::new();
        let mut auth = fixture.load();
        let grant = auth
            .register(USER, PASSWORD, SessionDevice::from_user_agent(UA))
            .unwrap();
        let records = fs::read_to_string(&auth.path).unwrap();
        let mut old_records = String::new();
        for line in records.lines() {
            let record = if line.starts_with("session\t") {
                line.split('\t').take(4).collect::<Vec<_>>().join("\t")
            } else {
                line.to_owned()
            };
            old_records.push_str(&record);
            old_records.push('\n');
        }
        let expired = random_secret().unwrap();
        old_records.push_str(&format!(
            "session\t{}\t{USER}\t1\n",
            hex(&secret_hash(&expired))
        ));
        let path = auth.path.clone();
        fs::write(path, old_records).unwrap();

        let mut auth = fixture.load();
        assert!(auth.active_sessions(&expired).is_none());
        assert!(auth.revoke_other_sessions(&expired).is_err());
        let old = auth.active_sessions(&grant.session).unwrap();
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].created_at, None);
        assert_eq!(old[0].device, "Device unavailable");
        assert!(old[0].current && old[0].expires_at > unix_seconds());
        let newer = sign_in(
            &mut auth,
            USER,
            SessionDevice::from_user_agent("Mozilla/5.0 (Android 15; private build) Firefox/154"),
        );
        let auth = fixture.load();
        let sessions = auth.active_sessions(&newer.session).unwrap();
        assert_eq!(sessions[0].device, "Firefox on Android");
        assert_eq!(sessions[1].expires_at, old[0].expires_at);
        assert_eq!(sessions[1].id, old[0].id);
        assert_eq!(sessions[1].created_at, None);
    }
}

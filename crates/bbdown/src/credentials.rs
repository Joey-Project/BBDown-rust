use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_CREDENTIAL_PROFILE: &str = "default";

#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub cookie: Option<String>,
    pub access_key: Option<String>,
    #[serde(default)]
    pub tv_access_key: Option<String>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let summary = self.redacted_summary();
        formatter
            .debug_struct("Credentials")
            .field("has_cookie", &summary.has_cookie)
            .field("has_access_key", &summary.has_access_key)
            .field("has_tv_access_key", &summary.has_tv_access_key)
            .finish()
    }
}

impl Credentials {
    #[must_use]
    pub fn with_cookie(mut self, cookie: impl Into<String>) -> Self {
        self.cookie = Some(cookie.into());
        self
    }

    #[must_use]
    pub fn with_access_key(mut self, access_key: impl Into<String>) -> Self {
        self.access_key = Some(access_key.into());
        self
    }

    #[must_use]
    pub fn with_tv_access_key(mut self, tv_access_key: impl Into<String>) -> Self {
        self.tv_access_key = Some(tv_access_key.into());
        self
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cookie.as_deref().unwrap_or_default().is_empty()
            && self.access_key.as_deref().unwrap_or_default().is_empty()
            && self.tv_access_key.as_deref().unwrap_or_default().is_empty()
    }

    #[must_use]
    pub fn redacted_summary(&self) -> CredentialSource {
        CredentialSource {
            has_cookie: self
                .cookie
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            has_access_key: self
                .access_key
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            has_tv_access_key: self
                .tv_access_key
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialProfiles {
    #[serde(default = "credential_profiles_version")]
    pub version: u32,
    #[serde(default = "default_credential_profile")]
    pub default_profile: String,
    #[serde(default)]
    pub profiles: BTreeMap<String, Credentials>,
}

impl fmt::Debug for CredentialProfiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialProfiles")
            .field("version", &self.version)
            .field("default_profile", &self.default_profile)
            .field("profiles", &self.profiles)
            .finish()
    }
}

impl Default for CredentialProfiles {
    fn default() -> Self {
        Self {
            version: 1,
            default_profile: DEFAULT_CREDENTIAL_PROFILE.to_owned(),
            profiles: BTreeMap::new(),
        }
    }
}

impl CredentialProfiles {
    #[must_use]
    pub fn from_credentials(credentials: Credentials) -> Self {
        let mut profiles = BTreeMap::new();
        if !credentials.is_empty() {
            profiles.insert(DEFAULT_CREDENTIAL_PROFILE.to_owned(), credentials);
        }
        Self {
            profiles,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn default_credentials(&self) -> Credentials {
        self.profile(&self.default_profile).unwrap_or_default()
    }

    pub fn profile(&self, name: &str) -> Result<Credentials> {
        let name = normalize_profile_name(name)?;
        Ok(self.profiles.get(&name).cloned().unwrap_or_default())
    }

    pub fn set_profile(&mut self, name: &str, credentials: Credentials) -> Result<()> {
        let name = normalize_profile_name(name)?;
        if credentials.is_empty() {
            self.profiles.remove(&name);
        } else {
            self.profiles.insert(name, credentials);
        }
        Ok(())
    }

    pub fn remove_profile(&mut self, name: &str) -> Result<Option<Credentials>> {
        let name = normalize_profile_name(name)?;
        Ok(self.profiles.remove(&name))
    }

    pub fn set_default_profile(&mut self, name: &str) -> Result<()> {
        self.default_profile = normalize_profile_name(name)?;
        Ok(())
    }

    pub fn profile_names(&self) -> impl Iterator<Item = &str> {
        self.profiles.keys().map(String::as_str)
    }

    fn normalize(mut self) -> Result<Self> {
        self.version = 1;
        self.default_profile = normalize_profile_name(&self.default_profile)
            .unwrap_or_else(|_| DEFAULT_CREDENTIAL_PROFILE.to_owned());
        let mut profiles = BTreeMap::new();
        for (name, credentials) in self.profiles {
            let name = normalize_profile_name(&name)?;
            if !credentials.is_empty() {
                profiles.insert(name, credentials);
            }
        }
        self.profiles = profiles;
        Ok(self)
    }
}

fn credential_profiles_version() -> u32 {
    1
}

fn default_credential_profile() -> String {
    DEFAULT_CREDENTIAL_PROFILE.to_owned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialSource {
    pub has_cookie: bool,
    pub has_access_key: bool,
    pub has_tv_access_key: bool,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialHealthReport {
    pub credentials: CredentialSource,
    pub probes: Vec<CredentialHealthProbe>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialHealthProbe {
    pub kind: CredentialKind,
    pub scope: CredentialHealthScope,
    pub status: CredentialHealthStatus,
    pub endpoint: Option<String>,
    pub api_code: Option<i64>,
    pub message: Option<String>,
}

impl CredentialHealthProbe {
    #[must_use]
    pub fn missing(kind: CredentialKind, scope: CredentialHealthScope) -> Self {
        Self {
            kind,
            scope,
            status: CredentialHealthStatus::Missing,
            endpoint: None,
            api_code: None,
            message: Some("credential is not configured".to_owned()),
        }
    }

    #[must_use]
    pub fn valid(
        kind: CredentialKind,
        scope: CredentialHealthScope,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            scope,
            status: CredentialHealthStatus::Valid,
            endpoint: Some(endpoint.into()),
            api_code: Some(0),
            message: None,
        }
    }

    #[must_use]
    pub fn rejected(
        kind: CredentialKind,
        scope: CredentialHealthScope,
        endpoint: impl Into<String>,
        api_code: Option<i64>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            scope,
            status: CredentialHealthStatus::Rejected,
            endpoint: Some(endpoint.into()),
            api_code,
            message: Some(message.into()),
        }
    }

    #[must_use]
    pub fn request_failed(
        kind: CredentialKind,
        scope: CredentialHealthScope,
        endpoint: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            scope,
            status: CredentialHealthStatus::RequestFailed,
            endpoint: Some(endpoint.into()),
            api_code: None,
            message: Some(message.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Cookie,
    AccessKey,
    TvAccessKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialHealthScope {
    WebCookie,
    IntlBstar,
    Tv,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialHealthStatus {
    Missing,
    Valid,
    Rejected,
    RequestFailed,
}

#[derive(Clone, Debug)]
pub struct CredentialStore {
    path: PathBuf,
}

impl CredentialStore {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Credentials> {
        Ok(self.load_profiles()?.default_credentials())
    }

    pub fn save(&self, credentials: &Credentials) -> Result<()> {
        if self.path.exists() && self.file_uses_profiles()? {
            let mut profiles = self.load_profiles()?;
            let default_profile = profiles.default_profile.clone();
            profiles.set_profile(&default_profile, credentials.clone())?;
            self.save_profiles(&profiles)
        } else {
            self.write_credentials(credentials)
        }
    }

    pub fn load_profiles(&self) -> Result<CredentialProfiles> {
        if !self.path.exists() {
            return Ok(CredentialProfiles::default());
        }
        let raw = fs::read_to_string(&self.path)?;
        parse_credential_profiles(&raw)
    }

    pub fn save_profiles(&self, profiles: &CredentialProfiles) -> Result<()> {
        let profiles = profiles.clone().normalize()?;
        self.write_bytes(&serde_json::to_vec_pretty(&profiles)?)
    }

    pub fn load_profile(&self, profile: &str) -> Result<Credentials> {
        self.load_profiles()?.profile(profile)
    }

    pub fn save_profile(&self, profile: &str, credentials: &Credentials) -> Result<()> {
        let mut profiles = self.load_profiles()?;
        profiles.set_profile(profile, credentials.clone())?;
        self.save_profiles(&profiles)
    }

    pub fn remove_profile(&self, profile: &str) -> Result<Option<Credentials>> {
        let mut profiles = self.load_profiles()?;
        let removed = profiles.remove_profile(profile)?;
        self.save_profiles(&profiles)?;
        Ok(removed)
    }

    fn write_credentials(&self, credentials: &Credentials) -> Result<()> {
        self.write_bytes(&serde_json::to_vec_pretty(credentials)?)
    }

    fn write_bytes(&self, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        write_private_file(&self.path, bytes)
    }

    fn file_uses_profiles(&self) -> Result<bool> {
        if !self.path.exists() {
            return Ok(false);
        }
        let raw = fs::read_to_string(&self.path)?;
        let Ok(value) = serde_json::from_str(&raw) else {
            return Ok(false);
        };
        Ok(is_profile_document(&value))
    }

    pub fn clear(&self) -> Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::Io(error)),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn parse_credential_profiles(raw: &str) -> Result<CredentialProfiles> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    if is_profile_document(&value) {
        serde_json::from_value::<CredentialProfiles>(value)
            .map_err(Error::from)?
            .normalize()
    } else {
        let credentials = serde_json::from_value::<Credentials>(value)?;
        Ok(CredentialProfiles::from_credentials(credentials))
    }
}

fn is_profile_document(value: &serde_json::Value) -> bool {
    value.get("profiles").is_some() || value.get("default_profile").is_some()
}

fn normalize_profile_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        Err(Error::InvalidInput(
            "credential profile name must not be empty".to_owned(),
        ))
    } else {
        Ok(name.to_owned())
    }
}

#[cfg(unix)]
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let tmp_path = private_temp_path(path);
    match fs::remove_file(&tmp_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::Io(error)),
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&tmp_path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp_path, path).map_err(|error| {
        let _ = fs::remove_file(&tmp_path);
        Error::Io(error)
    })?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(unix)]
fn private_temp_path(path: &Path) -> std::path::PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("credentials");
    path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id()))
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CredentialProfiles, CredentialStore, Credentials, DEFAULT_CREDENTIAL_PROFILE};

    #[test]
    fn stores_credentials_without_leaking_values_in_summary() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        store.save(&Credentials {
            cookie: Some("SESSDATA=secret".to_owned()),
            access_key: Some("token".to_owned()),
            tv_access_key: Some("tv-token".to_owned()),
        })?;

        let loaded = store.load()?;
        assert_eq!(loaded.cookie.as_deref(), Some("SESSDATA=secret"));
        assert_eq!(loaded.tv_access_key.as_deref(), Some("tv-token"));
        assert_eq!(
            loaded.redacted_summary(),
            super::CredentialSource {
                has_cookie: true,
                has_access_key: true,
                has_tv_access_key: true,
            }
        );
        Ok(())
    }

    #[test]
    fn credentials_debug_is_redacted() {
        let debug = format!(
            "{:?}",
            Credentials {
                cookie: Some("SESSDATA=secret".to_owned()),
                access_key: Some("access-token".to_owned()),
                tv_access_key: Some("tv-access-token".to_owned()),
            }
        );

        assert!(debug.contains("has_cookie: true"));
        assert!(debug.contains("has_access_key: true"));
        assert!(debug.contains("has_tv_access_key: true"));
        assert!(!debug.contains("SESSDATA=secret"));
        assert!(!debug.contains("access-token"));
        assert!(!debug.contains("tv-access-token"));
    }

    #[test]
    fn credential_profiles_debug_is_redacted() -> anyhow::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: Some("SESSDATA=secret".to_owned()),
                access_key: Some("access-token".to_owned()),
                tv_access_key: Some("tv-access-token".to_owned()),
            },
        )?;

        let debug = format!("{profiles:?}");

        assert!(debug.contains("intl"));
        assert!(debug.contains("has_cookie: true"));
        assert!(debug.contains("has_access_key: true"));
        assert!(debug.contains("has_tv_access_key: true"));
        assert!(!debug.contains("SESSDATA=secret"));
        assert!(!debug.contains("access-token"));
        assert!(!debug.contains("tv-access-token"));
        Ok(())
    }

    #[test]
    fn load_profiles_wraps_legacy_flat_credentials() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&Credentials {
                cookie: Some("SESSDATA=secret".to_owned()),
                access_key: Some("access-token".to_owned()),
                tv_access_key: Some("tv-access-token".to_owned()),
            })?,
        )?;
        let store = CredentialStore::new(path);

        let profiles = store.load_profiles()?;

        assert_eq!(profiles.default_profile, DEFAULT_CREDENTIAL_PROFILE);
        assert_eq!(
            profiles
                .profile(DEFAULT_CREDENTIAL_PROFILE)?
                .cookie
                .as_deref(),
            Some("SESSDATA=secret")
        );
        assert_eq!(
            store
                .load_profile(DEFAULT_CREDENTIAL_PROFILE)?
                .access_key
                .as_deref(),
            Some("access-token")
        );
        assert_eq!(
            store.load()?.tv_access_key.as_deref(),
            Some("tv-access-token")
        );
        Ok(())
    }

    #[test]
    fn version_only_legacy_flat_credentials_are_not_profiles() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "cookie": "SESSDATA=secret",
  "access_key": "access-token",
  "tv_access_key": "tv-access-token"
}"#,
        )?;
        let store = CredentialStore::new(path);

        let profiles = store.load_profiles()?;

        assert_eq!(
            profiles
                .profile(DEFAULT_CREDENTIAL_PROFILE)?
                .cookie
                .as_deref(),
            Some("SESSDATA=secret")
        );
        assert_eq!(store.load()?.access_key.as_deref(), Some("access-token"));
        assert_eq!(
            store.load()?.tv_access_key.as_deref(),
            Some("tv-access-token")
        );
        Ok(())
    }

    #[test]
    fn save_without_profiles_keeps_legacy_flat_format() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        store.save(&Credentials {
            cookie: Some("SESSDATA=secret".to_owned()),
            access_key: None,
            tv_access_key: None,
        })?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;

        assert!(value.get("profiles").is_none());
        assert!(value.get("default_profile").is_none());
        assert_eq!(
            value.get("cookie").and_then(serde_json::Value::as_str),
            Some("SESSDATA=secret")
        );
        Ok(())
    }

    #[test]
    fn save_profile_migrates_legacy_flat_file() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        store.save(&Credentials {
            cookie: Some("SESSDATA=default".to_owned()),
            access_key: None,
            tv_access_key: None,
        })?;

        store.save_profile(
            "intl",
            &Credentials {
                cookie: None,
                access_key: Some("access-token".to_owned()),
                tv_access_key: None,
            },
        )?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profiles").is_some());
        let profiles = store.load_profiles()?;
        assert_eq!(
            profiles
                .profile(DEFAULT_CREDENTIAL_PROFILE)?
                .cookie
                .as_deref(),
            Some("SESSDATA=default")
        );
        assert_eq!(
            profiles.profile("intl")?.access_key.as_deref(),
            Some("access-token")
        );
        assert_eq!(store.load()?.cookie.as_deref(), Some("SESSDATA=default"));
        Ok(())
    }

    #[test]
    fn profile_document_default_profile_controls_load_and_save() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            DEFAULT_CREDENTIAL_PROFILE,
            Credentials {
                cookie: Some("SESSDATA=default".to_owned()),
                access_key: None,
                tv_access_key: None,
            },
        )?;
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        profiles.set_default_profile("intl")?;
        store.save_profiles(&profiles)?;

        assert_eq!(store.load()?.access_key.as_deref(), Some("access-token"));

        store.save(&Credentials {
            cookie: Some("SESSDATA=updated-intl".to_owned()),
            access_key: None,
            tv_access_key: None,
        })?;

        let profiles = store.load_profiles()?;
        assert_eq!(
            profiles
                .profile(DEFAULT_CREDENTIAL_PROFILE)?
                .cookie
                .as_deref(),
            Some("SESSDATA=default")
        );
        assert_eq!(
            profiles.profile("intl")?.cookie.as_deref(),
            Some("SESSDATA=updated-intl")
        );
        Ok(())
    }

    #[test]
    fn profile_names_reject_blank_values() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        let credentials = Credentials {
            cookie: Some("SESSDATA=secret".to_owned()),
            access_key: None,
            tv_access_key: None,
        };
        let mut profiles = CredentialProfiles::default();

        assert!(profiles.set_profile("  ", credentials.clone()).is_err());
        assert!(profiles.set_default_profile("\n").is_err());
        assert!(store.save_profile("", &credentials).is_err());
        assert!(store.load_profile("\t").is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn save_tightens_existing_file_permissions() -> anyhow::Result<()> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o644)
            .open(&path)?;

        let store = CredentialStore::new(path.clone());
        store.save(&Credentials {
            cookie: Some("SESSDATA=secret".to_owned()),
            access_key: None,
            tv_access_key: None,
        })?;

        let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        Ok(())
    }

    #[test]
    fn save_allows_bare_relative_path() -> anyhow::Result<()> {
        use std::sync::{Mutex, OnceLock};

        static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = CWD_LOCK.get_or_init(|| Mutex::new(()));
        let _guard = lock
            .lock()
            .map_err(|error| anyhow::anyhow!("cwd lock poisoned: {error}"))?;

        let original = std::env::current_dir()?;
        let temp = tempfile::tempdir()?;
        std::env::set_current_dir(temp.path())?;

        let save_result =
            CredentialStore::new(std::path::PathBuf::from("credentials.json")).save(&Credentials {
                cookie: Some("SESSDATA=secret".to_owned()),
                access_key: None,
                tv_access_key: None,
            });
        std::env::set_current_dir(original)?;
        save_result?;

        assert!(temp.path().join("credentials.json").exists());
        Ok(())
    }
}

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub cookie: Option<String>,
    pub access_key: Option<String>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let summary = self.redacted_summary();
        formatter
            .debug_struct("Credentials")
            .field("has_cookie", &summary.has_cookie)
            .field("has_access_key", &summary.has_access_key)
            .finish()
    }
}

impl Credentials {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cookie.as_deref().unwrap_or_default().is_empty()
            && self.access_key.as_deref().unwrap_or_default().is_empty()
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
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialSource {
    pub has_cookie: bool,
    pub has_access_key: bool,
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
        if !self.path.exists() {
            return Ok(Credentials::default());
        }
        let raw = fs::read_to_string(&self.path)?;
        serde_json::from_str(&raw).map_err(Error::from)
    }

    pub fn save(&self, credentials: &Credentials) -> Result<()> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        write_private_file(&self.path, &serde_json::to_vec_pretty(credentials)?)
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
    use super::{CredentialStore, Credentials};

    #[test]
    fn stores_credentials_without_leaking_values_in_summary() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        store.save(&Credentials {
            cookie: Some("SESSDATA=secret".to_owned()),
            access_key: Some("token".to_owned()),
        })?;

        let loaded = store.load()?;
        assert_eq!(loaded.cookie.as_deref(), Some("SESSDATA=secret"));
        assert_eq!(
            loaded.redacted_summary(),
            super::CredentialSource {
                has_cookie: true,
                has_access_key: true,
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
            }
        );

        assert!(debug.contains("has_cookie: true"));
        assert!(debug.contains("has_access_key: true"));
        assert!(!debug.contains("SESSDATA=secret"));
        assert!(!debug.contains("access-token"));
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
            });
        std::env::set_current_dir(original)?;
        save_result?;

        assert!(temp.path().join("credentials.json").exists());
        Ok(())
    }
}

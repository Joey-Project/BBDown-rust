use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_CREDENTIAL_PROFILE: &str = "default";
const CREDENTIAL_PROFILES_VERSION: u32 = 1;
const CREDENTIAL_LOCK_STALE_AFTER_MILLIS: u64 = 30 * 60 * 1_000;
const CREDENTIAL_LOCK_RELEASE_RETRY_MILLIS: u64 = 10;
static CREDENTIAL_LOCK_COUNTER: AtomicU64 = AtomicU64::new(1);

#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum CredentialProfileSelection {
    #[default]
    Default,
    Named(String),
}

impl CredentialProfileSelection {
    #[must_use]
    pub fn default_profile() -> Self {
        Self::Default
    }

    pub fn named(name: impl AsRef<str>) -> Result<Self> {
        Ok(Self::Named(normalize_profile_name(name.as_ref())?))
    }

    #[must_use]
    pub fn profile_name(&self) -> Option<&str> {
        match self {
            Self::Default => None,
            Self::Named(name) => Some(name.as_str()),
        }
    }

    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }
}

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
        self.cookie
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
            && self
                .access_key
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            && self
                .tv_access_key
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
    }

    #[must_use]
    pub fn redacted_summary(&self) -> CredentialSource {
        CredentialSource {
            has_cookie: self
                .cookie
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()),
            has_access_key: self
                .access_key
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()),
            has_tv_access_key: self
                .tv_access_key
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()),
        }
    }
}

impl CredentialKind {
    pub const ALL: [Self; 3] = [Self::Cookie, Self::AccessKey, Self::TvAccessKey];

    fn from_metadata_key(key: &str) -> Option<Self> {
        match key {
            "cookie" => Some(Self::Cookie),
            "access_key" => Some(Self::AccessKey),
            "tv_access_key" => Some(Self::TvAccessKey),
            _ => None,
        }
    }

    fn is_present_in(self, credentials: &Credentials) -> bool {
        self.value_in(credentials).is_some()
    }

    fn is_unchanged_between(self, old: &Credentials, new: &Credentials) -> bool {
        match (self.value_in(old), self.value_in(new)) {
            (Some(old), Some(new)) => old == new,
            _ => false,
        }
    }

    fn value_in(self, credentials: &Credentials) -> Option<&str> {
        match self {
            Self::Cookie => credentials
                .cookie
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
            Self::AccessKey => credentials
                .access_key
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
            Self::TvAccessKey => credentials
                .tv_access_key
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
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
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "deserialize_profile_metadata"
    )]
    pub profile_metadata: BTreeMap<String, CredentialProfileMetadata>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "deserialize_profile_secrets"
    )]
    pub profile_secrets: BTreeMap<String, CredentialProfileSecrets>,
}

impl fmt::Debug for CredentialProfiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialProfiles")
            .field("version", &self.version)
            .field("default_profile", &self.default_profile)
            .field("profiles", &self.profiles)
            .field("profile_metadata", &self.profile_metadata)
            .field("profile_secrets", &self.profile_secrets)
            .finish()
    }
}

impl Default for CredentialProfiles {
    fn default() -> Self {
        Self {
            version: CREDENTIAL_PROFILES_VERSION,
            default_profile: DEFAULT_CREDENTIAL_PROFILE.to_owned(),
            profiles: BTreeMap::new(),
            profile_metadata: BTreeMap::new(),
            profile_secrets: BTreeMap::new(),
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
            self.profile_metadata.remove(&name);
            self.profile_secrets.remove(&name);
        } else {
            let metadata = self.profile_metadata.remove(&name);
            if let (Some(metadata), Some(previous)) = (metadata, self.profiles.get(&name)) {
                let metadata = metadata.normalize_for_unchanged_credentials(previous, &credentials);
                if !metadata.is_empty() {
                    self.profile_metadata.insert(name.clone(), metadata);
                }
            }
            let secrets = self.profile_secrets.remove(&name);
            if let (Some(secrets), Some(previous)) = (secrets, self.profiles.get(&name)) {
                let secrets = secrets.normalize_for_unchanged_credentials(previous, &credentials);
                if !secrets.is_empty() {
                    self.profile_secrets.insert(name.clone(), secrets);
                }
            }
            self.profiles.insert(name, credentials);
        }
        Ok(())
    }

    pub fn remove_profile(&mut self, name: &str) -> Result<Option<Credentials>> {
        let name = normalize_profile_name(name)?;
        self.profile_metadata.remove(&name);
        self.profile_secrets.remove(&name);
        Ok(self.profiles.remove(&name))
    }

    pub fn set_default_profile(&mut self, name: &str) -> Result<()> {
        self.default_profile = normalize_profile_name(name)?;
        Ok(())
    }

    pub fn profile_names(&self) -> impl Iterator<Item = &str> {
        self.profiles.keys().map(String::as_str)
    }

    pub fn profile_metadata(&self, name: &str) -> Result<CredentialProfileMetadata> {
        let name = normalize_profile_name(name)?;
        Ok(self
            .profile_metadata
            .get(&name)
            .cloned()
            .unwrap_or_default())
    }

    pub fn set_profile_metadata(
        &mut self,
        name: &str,
        metadata: CredentialProfileMetadata,
    ) -> Result<()> {
        let name = normalize_profile_name(name)?;
        let metadata = self
            .profiles
            .get(&name)
            .map_or_else(CredentialProfileMetadata::default, |credentials| {
                metadata.normalize_for_credentials(credentials)
            });
        if metadata.is_empty() {
            self.profile_metadata.remove(&name);
        } else {
            self.profile_metadata.insert(name, metadata);
        }
        Ok(())
    }

    pub fn profile_secrets(&self, name: &str) -> Result<CredentialProfileSecrets> {
        let name = normalize_profile_name(name)?;
        Ok(self.profile_secrets.get(&name).cloned().unwrap_or_default())
    }

    pub fn set_profile_secrets(
        &mut self,
        name: &str,
        secrets: CredentialProfileSecrets,
    ) -> Result<()> {
        let name = normalize_profile_name(name)?;
        let secrets = self
            .profiles
            .get(&name)
            .map_or_else(CredentialProfileSecrets::default, |credentials| {
                secrets.normalize_for_credentials(credentials)
            });
        if secrets.is_empty() {
            self.profile_secrets.remove(&name);
        } else {
            self.profile_secrets.insert(name, secrets);
        }
        Ok(())
    }

    pub fn profile_lifecycle_status(
        &self,
        name: &str,
        policy: &CredentialLifecyclePolicy,
    ) -> Result<CredentialProfileLifecycleStatus> {
        let name = normalize_profile_name(name)?;
        let credentials = self.profile(&name)?;
        let metadata = self.profile_metadata(&name)?;
        let secrets = self.profile_secrets(&name)?;
        Ok(CredentialProfileLifecycleStatus::from_parts(
            name.clone(),
            name == self.default_profile,
            &credentials,
            &metadata,
            &secrets,
            policy,
        ))
    }

    pub fn lifecycle_statuses(
        &self,
        policy: &CredentialLifecyclePolicy,
    ) -> Result<Vec<CredentialProfileLifecycleStatus>> {
        let mut names = BTreeSet::new();
        names.insert(self.default_profile.clone());
        names.extend(self.profiles.keys().cloned());
        names
            .into_iter()
            .map(|name| self.profile_lifecycle_status(&name, policy))
            .collect()
    }

    fn normalize(mut self) -> Result<Self> {
        if self.version != CREDENTIAL_PROFILES_VERSION {
            return Err(Error::InvalidInput(format!(
                "unsupported credential profile document version {}; expected {CREDENTIAL_PROFILES_VERSION}",
                self.version
            )));
        }
        self.version = CREDENTIAL_PROFILES_VERSION;
        self.default_profile = normalize_profile_name(&self.default_profile)?;
        let mut profiles = BTreeMap::new();
        for (name, credentials) in self.profiles {
            let name = normalize_profile_name(&name)?;
            if !credentials.is_empty() {
                profiles.insert(name, credentials);
            }
        }
        self.profiles = profiles;
        let mut profile_metadata = BTreeMap::new();
        for (name, metadata) in self.profile_metadata {
            if name.trim().is_empty() {
                continue;
            }
            let name = normalize_profile_name(&name)?;
            if let Some(credentials) = self.profiles.get(&name) {
                let metadata = metadata.normalize_for_credentials(credentials);
                if !metadata.is_empty() {
                    profile_metadata.insert(name, metadata);
                }
            }
        }
        self.profile_metadata = profile_metadata;
        let mut profile_secrets = BTreeMap::new();
        for (name, secrets) in self.profile_secrets {
            if name.trim().is_empty() {
                continue;
            }
            let name = normalize_profile_name(&name)?;
            if let Some(credentials) = self.profiles.get(&name) {
                let secrets = secrets.normalize_for_credentials(credentials);
                if !secrets.is_empty() {
                    profile_secrets.insert(name, secrets);
                }
            }
        }
        self.profile_secrets = profile_secrets;
        Ok(self)
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialProfileMetadata {
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "deserialize_credential_lifecycle_metadata"
    )]
    pub credentials: BTreeMap<CredentialKind, CredentialLifecycleMetadata>,
}

impl CredentialProfileMetadata {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.credentials
            .values()
            .all(CredentialLifecycleMetadata::is_empty)
    }

    #[must_use]
    pub fn credential(&self, kind: CredentialKind) -> Option<&CredentialLifecycleMetadata> {
        self.credentials.get(&kind)
    }

    pub fn set_credential(&mut self, kind: CredentialKind, metadata: CredentialLifecycleMetadata) {
        if metadata.is_empty() {
            self.credentials.remove(&kind);
        } else {
            self.credentials.insert(kind, metadata);
        }
    }

    fn normalize_for_credentials(mut self, credentials: &Credentials) -> Self {
        self.credentials
            .retain(|kind, metadata| !metadata.is_empty() && kind.is_present_in(credentials));
        self
    }

    fn normalize_for_unchanged_credentials(mut self, old: &Credentials, new: &Credentials) -> Self {
        self.credentials
            .retain(|kind, metadata| !metadata.is_empty() && kind.is_unchanged_between(old, new));
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialProfileSecrets {
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "deserialize_access_key_provider_secrets"
    )]
    pub access_key: BTreeMap<AccessKeyProvider, AccessKeyProviderSecret>,
}

impl fmt::Debug for CredentialProfileSecrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialProfileSecrets")
            .field("access_key", &self.access_key)
            .finish()
    }
}

impl CredentialProfileSecrets {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.access_key
            .values()
            .all(AccessKeyProviderSecret::is_empty)
    }

    #[must_use]
    pub fn access_key_provider(
        &self,
        provider: AccessKeyProvider,
    ) -> Option<&AccessKeyProviderSecret> {
        self.access_key.get(&provider)
    }

    pub fn set_access_key_provider(
        &mut self,
        provider: AccessKeyProvider,
        secret: AccessKeyProviderSecret,
    ) {
        if secret.is_empty() {
            self.access_key.remove(&provider);
        } else {
            self.access_key.insert(provider, secret);
        }
    }

    fn normalize_for_credentials(mut self, credentials: &Credentials) -> Self {
        if CredentialKind::AccessKey.is_present_in(credentials) {
            self.access_key
                .retain(|_, secret| !AccessKeyProviderSecret::is_empty(secret));
        } else {
            self.access_key.clear();
        }
        self
    }

    fn normalize_for_unchanged_credentials(mut self, old: &Credentials, new: &Credentials) -> Self {
        if !CredentialKind::AccessKey.is_unchanged_between(old, new) {
            self.access_key.clear();
        }
        self.normalize_for_credentials(new)
    }
}

#[non_exhaustive]
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccessKeyProviderSecret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_provider: Option<AccessKeyRefreshProvider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_keypair: Option<AccessKeyRefreshKeypair>,
}

impl fmt::Debug for AccessKeyProviderSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccessKeyProviderSecret")
            .field("has_refresh_token", &self.has_refresh_token())
            .field("refresh_provider", &self.refresh_provider)
            .field("refresh_keypair", &self.refresh_keypair)
            .finish_non_exhaustive()
    }
}

impl AccessKeyProviderSecret {
    #[must_use]
    pub fn with_refresh_token(mut self, refresh_token: impl Into<String>) -> Self {
        self.refresh_token = Some(refresh_token.into());
        self
    }

    #[must_use]
    pub fn with_refresh_provider(mut self, provider: AccessKeyRefreshProvider) -> Self {
        self.refresh_provider = Some(provider);
        self
    }

    #[must_use]
    pub fn with_refresh_keypair(mut self, keypair: AccessKeyRefreshKeypair) -> Self {
        self.refresh_keypair = Some(keypair);
        self
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.refresh_token
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
            && self.refresh_provider.is_none()
            && self.refresh_keypair.is_none()
    }

    #[must_use]
    pub fn has_refresh_token(&self) -> bool {
        self.refresh_token
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }
}

fn deserialize_credential_lifecycle_metadata<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<CredentialKind, CredentialLifecycleMetadata>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let Some(raw_metadata) = value.as_object() else {
        return Ok(BTreeMap::new());
    };

    Ok(raw_metadata
        .iter()
        .filter_map(|(key, value)| {
            let kind = CredentialKind::from_metadata_key(key)?;
            let metadata = serde_json::from_value(value.clone()).ok()?;
            Some((kind, metadata))
        })
        .collect())
}

fn deserialize_access_key_provider_secrets<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<AccessKeyProvider, AccessKeyProviderSecret>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let Some(raw_secrets) = value.as_object() else {
        return Ok(BTreeMap::new());
    };

    Ok(raw_secrets
        .iter()
        .filter_map(|(key, value)| {
            let provider = AccessKeyProvider::from_storage_key(key)?;
            let secret = serde_json::from_value(value.clone()).ok()?;
            Some((provider, secret))
        })
        .collect())
}

fn deserialize_profile_metadata<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, CredentialProfileMetadata>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let Some(raw_metadata) = value.as_object() else {
        return Ok(BTreeMap::new());
    };

    Ok(raw_metadata
        .iter()
        .filter_map(|(name, value)| {
            let metadata = serde_json::from_value(value.clone()).ok()?;
            Some((name.clone(), metadata))
        })
        .collect())
}

fn deserialize_profile_secrets<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, CredentialProfileSecrets>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let Some(raw_secrets) = value.as_object() else {
        return Ok(BTreeMap::new());
    };

    Ok(raw_secrets
        .iter()
        .filter_map(|(name, value)| {
            let secrets = serde_json::from_value(value.clone()).ok()?;
            Some((name.clone(), secrets))
        })
        .collect())
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialLifecycleMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<CredentialLifecycleSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key_provider: Option<AccessKeyProvider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acquired_at_unix_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at_unix_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_unix_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token_present: Option<bool>,
}

impl CredentialLifecycleMetadata {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.source.is_none()
            && self.access_key_provider.is_none()
            && self.acquired_at_unix_millis.is_none()
            && self.checked_at_unix_millis.is_none()
            && self.expires_at_unix_millis.is_none()
            && self.refresh_token_present.is_none()
    }

    #[must_use]
    pub fn with_source(mut self, source: CredentialLifecycleSource) -> Self {
        self.source = Some(source);
        self
    }

    #[must_use]
    pub fn with_access_key_provider(mut self, provider: AccessKeyProvider) -> Self {
        self.access_key_provider = Some(provider);
        self
    }

    #[must_use]
    pub fn with_acquired_at_unix_millis(mut self, value: u64) -> Self {
        self.acquired_at_unix_millis = Some(value);
        self
    }

    #[must_use]
    pub fn with_checked_at_unix_millis(mut self, value: u64) -> Self {
        self.checked_at_unix_millis = Some(value);
        self
    }

    #[must_use]
    pub fn with_expires_at_unix_millis(mut self, value: u64) -> Self {
        self.expires_at_unix_millis = Some(value);
        self
    }

    #[must_use]
    pub fn with_refresh_token_present(mut self, value: bool) -> Self {
        self.refresh_token_present = Some(value);
        self
    }
}

const DEFAULT_LIFECYCLE_STALE_AFTER_MILLIS: u64 = 7 * 24 * 60 * 60 * 1_000;
const DEFAULT_LIFECYCLE_EXPIRING_WITHIN_MILLIS: u64 = 24 * 60 * 60 * 1_000;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialLifecyclePolicy {
    pub now_unix_millis: u64,
    pub stale_after_millis: Option<u64>,
    pub expiring_within_millis: Option<u64>,
}

impl CredentialLifecyclePolicy {
    #[must_use]
    pub fn at_unix_millis(now_unix_millis: u64) -> Self {
        Self {
            now_unix_millis,
            stale_after_millis: Some(DEFAULT_LIFECYCLE_STALE_AFTER_MILLIS),
            expiring_within_millis: Some(DEFAULT_LIFECYCLE_EXPIRING_WITHIN_MILLIS),
        }
    }

    #[must_use]
    pub fn with_stale_after_millis(mut self, value: Option<u64>) -> Self {
        self.stale_after_millis = value;
        self
    }

    #[must_use]
    pub fn with_expiring_within_millis(mut self, value: Option<u64>) -> Self {
        self.expiring_within_millis = value;
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialProfileLifecycleStatus {
    pub profile: String,
    pub is_default_profile: bool,
    pub credentials: CredentialSource,
    pub status: CredentialLifecycleStatus,
    pub credential_statuses: Vec<CredentialLifecycleCredentialStatus>,
}

impl CredentialProfileLifecycleStatus {
    fn from_parts(
        profile: String,
        is_default_profile: bool,
        credentials: &Credentials,
        metadata: &CredentialProfileMetadata,
        secrets: &CredentialProfileSecrets,
        policy: &CredentialLifecyclePolicy,
    ) -> Self {
        let credential_statuses = CredentialKind::ALL
            .iter()
            .copied()
            .map(|kind| {
                CredentialLifecycleCredentialStatus::from_parts(
                    kind,
                    credentials,
                    metadata.credential(kind),
                    secrets,
                    policy,
                )
            })
            .collect::<Vec<_>>();
        let status = CredentialLifecycleStatus::overall(
            credential_statuses
                .iter()
                .filter(|status| status.present)
                .map(|status| status.status),
        );
        Self {
            profile,
            is_default_profile,
            credentials: credentials.redacted_summary(),
            status,
            credential_statuses,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialLifecycleCredentialStatus {
    pub kind: CredentialKind,
    pub present: bool,
    pub status: CredentialLifecycleStatus,
    pub source: Option<CredentialLifecycleSource>,
    pub access_key_provider: Option<AccessKeyProvider>,
    pub refresh_provider: Option<AccessKeyRefreshProvider>,
    pub refresh_keypair: Option<AccessKeyRefreshKeypair>,
    pub acquired_at_unix_millis: Option<u64>,
    pub checked_at_unix_millis: Option<u64>,
    pub expires_at_unix_millis: Option<u64>,
    pub refresh_token_present: Option<bool>,
    pub refresh_token_secret_present: Option<bool>,
}

impl CredentialLifecycleCredentialStatus {
    fn from_parts(
        kind: CredentialKind,
        credentials: &Credentials,
        metadata: Option<&CredentialLifecycleMetadata>,
        secrets: &CredentialProfileSecrets,
        policy: &CredentialLifecyclePolicy,
    ) -> Self {
        let present = kind.is_present_in(credentials);
        let metadata = metadata.cloned().unwrap_or_default();
        let refresh_secret = access_key_refresh_secret(kind, &metadata, secrets);
        let refresh_token_secret_present =
            access_key_refresh_secret_present(kind, &metadata, refresh_secret);
        let refresh_provider = refresh_secret.and_then(|secret| secret.refresh_provider);
        let refresh_keypair = refresh_secret.and_then(|secret| secret.refresh_keypair);
        let status = if present {
            CredentialLifecycleStatus::from_metadata(&metadata, policy)
        } else {
            CredentialLifecycleStatus::Missing
        };
        Self {
            kind,
            present,
            status,
            source: metadata.source,
            access_key_provider: metadata.access_key_provider,
            refresh_provider,
            refresh_keypair,
            acquired_at_unix_millis: metadata.acquired_at_unix_millis,
            checked_at_unix_millis: metadata.checked_at_unix_millis,
            expires_at_unix_millis: metadata.expires_at_unix_millis,
            refresh_token_present: metadata.refresh_token_present,
            refresh_token_secret_present,
        }
    }
}

fn access_key_refresh_secret<'a>(
    kind: CredentialKind,
    metadata: &CredentialLifecycleMetadata,
    secrets: &'a CredentialProfileSecrets,
) -> Option<&'a AccessKeyProviderSecret> {
    if kind != CredentialKind::AccessKey {
        return None;
    }
    let provider = metadata.access_key_provider?;
    secrets.access_key_provider(provider)
}

fn access_key_refresh_secret_present(
    kind: CredentialKind,
    metadata: &CredentialLifecycleMetadata,
    refresh_secret: Option<&AccessKeyProviderSecret>,
) -> Option<bool> {
    if kind != CredentialKind::AccessKey || metadata.access_key_provider.is_none() {
        return None;
    }
    Some(refresh_secret.is_some_and(AccessKeyProviderSecret::has_refresh_token))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialLifecycleStatus {
    Missing,
    Unknown,
    Fresh,
    Stale,
    Expiring,
    Expired,
}

impl CredentialLifecycleStatus {
    fn from_metadata(
        metadata: &CredentialLifecycleMetadata,
        policy: &CredentialLifecyclePolicy,
    ) -> Self {
        if let Some(expires_at) = metadata.expires_at_unix_millis {
            if expires_at <= policy.now_unix_millis {
                return Self::Expired;
            }
            if policy
                .expiring_within_millis
                .is_some_and(|window| expires_at <= policy.now_unix_millis.saturating_add(window))
            {
                return Self::Expiring;
            }
        }

        if let Some(last_seen) = metadata
            .checked_at_unix_millis
            .or(metadata.acquired_at_unix_millis)
        {
            if policy
                .stale_after_millis
                .is_some_and(|window| policy.now_unix_millis.saturating_sub(last_seen) > window)
            {
                return Self::Stale;
            }
            return Self::Fresh;
        }

        if metadata.expires_at_unix_millis.is_some() {
            Self::Fresh
        } else {
            Self::Unknown
        }
    }

    fn overall(statuses: impl IntoIterator<Item = Self>) -> Self {
        let mut saw_any = false;
        let mut result = Self::Fresh;
        for status in statuses {
            saw_any = true;
            if status.severity() > result.severity() {
                result = status;
            }
        }
        if saw_any { result } else { Self::Missing }
    }

    fn severity(self) -> u8 {
        match self {
            Self::Fresh => 0,
            Self::Unknown => 1,
            Self::Stale => 2,
            Self::Expiring => 3,
            Self::Expired => 4,
            Self::Missing => 5,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialLifecycleSource {
    ManualImport,
    WebQrLogin,
    TvQrLogin,
    AccessKeyLogin,
    #[serde(other)]
    Unknown,
}

fn credential_profiles_version() -> u32 {
    CREDENTIAL_PROFILES_VERSION
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

impl CredentialHealthReport {
    #[must_use]
    pub fn summary(&self) -> CredentialHealthSummary {
        CredentialHealthSummary::from_probes(&self.probes)
    }

    #[must_use]
    pub fn probe(
        &self,
        kind: CredentialKind,
        scope: CredentialHealthScope,
    ) -> Option<&CredentialHealthProbe> {
        self.probes
            .iter()
            .find(|probe| probe.kind == kind && probe.scope == scope)
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialHealthSummary {
    pub status: CredentialHealthSummaryStatus,
    pub valid_count: usize,
    pub missing_count: usize,
    pub rejected_count: usize,
    pub request_failed_count: usize,
}

impl CredentialHealthSummary {
    fn from_probes(probes: &[CredentialHealthProbe]) -> Self {
        let mut summary = Self {
            status: CredentialHealthSummaryStatus::Unknown,
            valid_count: 0,
            missing_count: 0,
            rejected_count: 0,
            request_failed_count: 0,
        };
        for probe in probes {
            match probe.status {
                CredentialHealthStatus::Missing => summary.missing_count += 1,
                CredentialHealthStatus::Valid => summary.valid_count += 1,
                CredentialHealthStatus::Rejected => summary.rejected_count += 1,
                CredentialHealthStatus::RequestFailed => summary.request_failed_count += 1,
            }
        }
        summary.status = if probes.is_empty() {
            CredentialHealthSummaryStatus::Unknown
        } else if summary.rejected_count > 0 {
            CredentialHealthSummaryStatus::Rejected
        } else if summary.request_failed_count > 0 {
            CredentialHealthSummaryStatus::RequestFailed
        } else if summary.valid_count == probes.len() {
            CredentialHealthSummaryStatus::Healthy
        } else if summary.valid_count == 0 && summary.missing_count == probes.len() {
            CredentialHealthSummaryStatus::Missing
        } else {
            CredentialHealthSummaryStatus::Degraded
        };
        summary
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialHealthSummaryStatus {
    Unknown,
    Healthy,
    Degraded,
    Missing,
    Rejected,
    RequestFailed,
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Cookie,
    AccessKey,
    TvAccessKey,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKeyProvider {
    BalhBiliplus,
    BilibiliMainOauth2,
    BiliIntlOauth2,
}

impl AccessKeyProvider {
    fn from_storage_key(key: &str) -> Option<Self> {
        match key {
            "balh_biliplus" => Some(Self::BalhBiliplus),
            "bilibili_main_oauth2" => Some(Self::BilibiliMainOauth2),
            "bili_intl_oauth2" => Some(Self::BiliIntlOauth2),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKeyRefreshProvider {
    BilibiliMainOauth2,
    BiliIntlOauth2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKeyRefreshKeypair {
    BiliTv,
    Android,
    AndroidB,
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
        let guard = self.acquire_update_lock()?;
        if self.path.exists() && self.file_uses_profiles()? {
            let mut profiles = self.load_profiles()?;
            let default_profile = profiles.default_profile.clone();
            profiles.set_profile(&default_profile, credentials.clone())?;
            self.save_profiles_locked(&profiles, &guard)
        } else {
            self.write_credentials_locked(credentials, &guard)
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
        let guard = self.acquire_update_lock()?;
        self.save_profiles_locked(profiles, &guard)
    }

    pub fn update_profiles<T>(
        &self,
        update: impl FnOnce(&mut CredentialProfiles) -> Result<T>,
    ) -> Result<T> {
        let guard = self.acquire_update_lock()?;
        let mut profiles = self.load_profiles()?;
        let before = profiles.clone();
        let output = update(&mut profiles)?;
        if profiles != before {
            self.save_profiles_locked(&profiles, &guard)?;
        }
        Ok(output)
    }

    pub fn update_profile(
        &self,
        profile: &str,
        update: impl FnOnce(Credentials) -> Result<Credentials>,
    ) -> Result<Credentials> {
        let profile = normalize_profile_name(profile)?;
        self.update_profiles(|profiles| {
            let credentials = profiles.profile(&profile)?;
            let updated = update(credentials)?;
            profiles.set_profile(&profile, updated.clone())?;
            Ok(updated)
        })
    }

    pub fn update_selected_profile(
        &self,
        selection: &CredentialProfileSelection,
        update: impl FnOnce(Credentials) -> Result<Credentials>,
    ) -> Result<Credentials> {
        match selection {
            CredentialProfileSelection::Default => self.update_default_profile(update),
            CredentialProfileSelection::Named(profile) => self.update_profile(profile, update),
        }
    }

    fn save_profiles_locked(
        &self,
        profiles: &CredentialProfiles,
        guard: &CredentialStoreUpdateLock,
    ) -> Result<()> {
        let profiles = profiles.clone().normalize()?;
        self.write_bytes_locked(&serde_json::to_vec_pretty(&profiles)?, guard)
    }

    fn update_default_profile(
        &self,
        update: impl FnOnce(Credentials) -> Result<Credentials>,
    ) -> Result<Credentials> {
        let guard = self.acquire_update_lock()?;
        if self.file_uses_profiles()? {
            let mut profiles = self.load_profiles()?;
            let profile = profiles.default_profile.clone();
            let credentials = profiles.profile(&profile)?;
            let updated = update(credentials)?;
            profiles.set_profile(&profile, updated.clone())?;
            self.save_profiles_locked(&profiles, &guard)?;
            Ok(updated)
        } else {
            let credentials = self.load_credentials_unlocked()?;
            let updated = update(credentials)?;
            self.write_credentials_locked(&updated, &guard)?;
            Ok(updated)
        }
    }

    pub fn load_profile(&self, profile: &str) -> Result<Credentials> {
        self.load_profiles()?.profile(profile)
    }

    pub fn load_selected_profile(
        &self,
        selection: &CredentialProfileSelection,
    ) -> Result<Credentials> {
        match selection {
            CredentialProfileSelection::Default => self.load(),
            CredentialProfileSelection::Named(profile) => self.load_profile(profile),
        }
    }

    pub fn save_profile(&self, profile: &str, credentials: &Credentials) -> Result<()> {
        self.update_profile(profile, |_| Ok(credentials.clone()))?;
        Ok(())
    }

    pub fn save_selected_profile(
        &self,
        selection: &CredentialProfileSelection,
        credentials: &Credentials,
    ) -> Result<()> {
        match selection {
            CredentialProfileSelection::Default => self.save(credentials),
            CredentialProfileSelection::Named(profile) => self.save_profile(profile, credentials),
        }
    }

    pub fn remove_profile(&self, profile: &str) -> Result<Option<Credentials>> {
        let guard = self.acquire_update_lock()?;
        let file_uses_profiles = self.file_uses_profiles()?;
        let mut profiles = self.load_profiles()?;
        let removed = profiles.remove_profile(profile)?;
        if removed.is_some() || file_uses_profiles {
            self.save_profiles_locked(&profiles, &guard)?;
        }
        Ok(removed)
    }

    fn write_credentials_locked(
        &self,
        credentials: &Credentials,
        guard: &CredentialStoreUpdateLock,
    ) -> Result<()> {
        self.write_bytes_locked(&serde_json::to_vec_pretty(credentials)?, guard)
    }

    fn load_credentials_unlocked(&self) -> Result<Credentials> {
        if !self.path.exists() {
            return Ok(Credentials::default());
        }
        let raw = fs::read_to_string(&self.path)?;
        if raw.trim().is_empty() {
            return Ok(Credentials::default());
        }
        Ok(parse_credential_profiles(&raw)?.default_credentials())
    }

    fn write_bytes_locked(&self, bytes: &[u8], guard: &CredentialStoreUpdateLock) -> Result<()> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        write_private_file(&self.path, bytes, || {
            acquire_replace_coordination_guard(guard)
        })
    }

    fn file_uses_profiles(&self) -> Result<bool> {
        if !self.path.exists() {
            return Ok(false);
        }
        let raw = fs::read_to_string(&self.path)?;
        if raw.trim().is_empty() {
            return Ok(false);
        }
        let value = serde_json::from_str(&raw)?;
        Ok(is_profile_document(&value))
    }

    fn acquire_update_lock(&self) -> Result<CredentialStoreUpdateLock> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let lock_path = private_lock_path(&self.path);
        loop {
            let Some(_coordination_guard) = acquire_lock_coordination_guard(&lock_path)? else {
                return Err(Error::InvalidInput(format!(
                    "credential store is locked by another update: {}",
                    lock_path.display()
                )));
            };
            if let Some(lock) = try_create_lock_file(&lock_path)? {
                return Ok(lock);
            }
            if remove_stale_lock_file(&lock_path)? {
                continue;
            }
            return Err(Error::InvalidInput(format!(
                "credential store is locked by another update: {}",
                lock_path.display()
            )));
        }
    }

    pub fn clear(&self) -> Result<()> {
        let guard = self.acquire_update_lock()?;
        let _replace_guard = acquire_replace_coordination_guard(&guard)?;
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

#[derive(Debug)]
struct CredentialStoreUpdateLock {
    path: PathBuf,
    token: String,
    coordinate_release: bool,
}

impl CredentialStoreUpdateLock {
    fn is_current(&self) -> Result<bool> {
        let raw = match fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(Error::Io(error)),
        };
        Ok(parse_lock_token(&raw) == Some(self.token.as_str()))
    }

    fn ensure_current(&self) -> Result<()> {
        if self.is_current()? {
            Ok(())
        } else {
            Err(Error::InvalidInput(format!(
                "credential store lock was reclaimed before write: {}",
                self.path.display()
            )))
        }
    }
}

impl Drop for CredentialStoreUpdateLock {
    fn drop(&mut self) {
        loop {
            let _coordination_guard = if self.coordinate_release {
                match acquire_lock_coordination_guard(&self.path) {
                    Ok(Some(guard)) => Some(guard),
                    Ok(None) => match self.is_current() {
                        Ok(true) => {
                            std::thread::sleep(std::time::Duration::from_millis(
                                CREDENTIAL_LOCK_RELEASE_RETRY_MILLIS,
                            ));
                            continue;
                        }
                        Ok(false) | Err(_) => return,
                    },
                    Err(_) => return,
                }
            } else {
                None
            };
            let Ok(raw) = fs::read_to_string(&self.path) else {
                return;
            };
            if parse_lock_token(&raw) == Some(self.token.as_str()) {
                let _ = fs::remove_file(&self.path);
            }
            return;
        }
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
    !has_flat_credential_fields(value)
        && (value.get("profiles").is_some() || value.get("default_profile").is_some())
}

fn has_flat_credential_fields(value: &serde_json::Value) -> bool {
    value.get("cookie").is_some()
        || value.get("access_key").is_some()
        || value.get("tv_access_key").is_some()
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
fn write_private_file(
    path: &Path,
    bytes: &[u8],
    acquire_replace_guard: impl FnOnce() -> Result<CredentialStoreUpdateLock>,
) -> Result<()> {
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
    let _replace_guard = match acquire_replace_guard() {
        Ok(guard) => guard,
        Err(error) => {
            let _ = fs::remove_file(&tmp_path);
            return Err(error);
        }
    };
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

fn private_lock_path(path: &Path) -> std::path::PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("credentials");
    path.with_file_name(format!(".{file_name}.lock"))
}

fn reclaim_lock_path(lock_path: &Path) -> std::path::PathBuf {
    let file_name = lock_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(".credentials.lock");
    lock_path.with_file_name(format!("{file_name}.reclaim"))
}

fn current_unix_millis() -> u64 {
    system_time_unix_millis(SystemTime::now())
}

fn system_time_unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |duration| {
        u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
    })
}

fn next_credential_lock_token(created_at_unix_millis: u64) -> String {
    let counter = CREDENTIAL_LOCK_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{created_at_unix_millis}-{counter}", std::process::id())
}

fn try_create_lock_file(lock_path: &Path) -> Result<Option<CredentialStoreUpdateLock>> {
    try_create_lock_file_with_release_mode(lock_path, true, |file, token, created_at| {
        writeln!(file, "token={token}")?;
        writeln!(file, "pid={}", std::process::id())?;
        writeln!(file, "created_at_unix_millis={created_at}")?;
        file.sync_all()?;
        Ok(())
    })
}

fn try_create_coordination_lock_file(
    lock_path: &Path,
) -> Result<Option<CredentialStoreUpdateLock>> {
    try_create_lock_file_with_release_mode(lock_path, false, |file, token, created_at| {
        writeln!(file, "token={token}")?;
        writeln!(file, "pid={}", std::process::id())?;
        writeln!(file, "created_at_unix_millis={created_at}")?;
        file.sync_all()?;
        Ok(())
    })
}

#[cfg(test)]
fn try_create_lock_file_with_metadata_writer(
    lock_path: &Path,
    write_metadata: impl FnOnce(&mut fs::File, &str, u64) -> Result<()>,
) -> Result<Option<CredentialStoreUpdateLock>> {
    try_create_lock_file_with_release_mode(lock_path, true, write_metadata)
}

fn try_create_lock_file_with_release_mode(
    lock_path: &Path,
    coordinate_release: bool,
    write_metadata: impl FnOnce(&mut fs::File, &str, u64) -> Result<()>,
) -> Result<Option<CredentialStoreUpdateLock>> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(lock_path) {
        Ok(mut file) => {
            let created_at = current_unix_millis();
            let token = next_credential_lock_token(created_at);
            let mut pending_lock = PendingCredentialStoreUpdateLock {
                path: lock_path.to_owned(),
                cleanup: true,
            };
            if let Err(error) = write_metadata(&mut file, &token, created_at) {
                drop(file);
                drop(pending_lock);
                return Err(error);
            }
            pending_lock.cleanup = false;
            Ok(Some(CredentialStoreUpdateLock {
                path: lock_path.to_owned(),
                token,
                coordinate_release,
            }))
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
        Err(error) => Err(Error::Io(error)),
    }
}

struct PendingCredentialStoreUpdateLock {
    path: PathBuf,
    cleanup: bool,
}

impl Drop for PendingCredentialStoreUpdateLock {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn acquire_lock_coordination_guard(lock_path: &Path) -> Result<Option<CredentialStoreUpdateLock>> {
    let reclaim_lock_path = reclaim_lock_path(lock_path);
    loop {
        if let Some(reclaim_guard) = try_create_coordination_lock_file(&reclaim_lock_path)? {
            return Ok(Some(reclaim_guard));
        }
        if !remove_stale_lock_file(&reclaim_lock_path)? {
            return Ok(None);
        }
    }
}

fn acquire_replace_coordination_guard(
    guard: &CredentialStoreUpdateLock,
) -> Result<CredentialStoreUpdateLock> {
    let Some(coordination_guard) = acquire_lock_coordination_guard(&guard.path)? else {
        return Err(Error::InvalidInput(format!(
            "credential store lock was reclaimed before write: {}",
            guard.path.display()
        )));
    };
    guard.ensure_current()?;
    Ok(coordination_guard)
}

fn remove_stale_lock_file(lock_path: &Path) -> Result<bool> {
    let raw = match fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(Error::Io(error)),
    };
    let Some(created_at) = lock_created_at_unix_millis(lock_path, &raw) else {
        return Ok(false);
    };
    if current_unix_millis().saturating_sub(created_at) < CREDENTIAL_LOCK_STALE_AFTER_MILLIS {
        return Ok(false);
    }
    if lock_owner_may_still_be_running(&raw) {
        return Ok(false);
    }
    let expected = LockFileIdentity::from_raw(&raw);
    remove_lock_file_if_matches(lock_path, &expected)
}

enum LockFileIdentity<'a> {
    Token(&'a str),
    Raw(&'a str),
}

impl<'a> LockFileIdentity<'a> {
    fn from_raw(raw: &'a str) -> Self {
        parse_lock_token(raw).map_or(Self::Raw(raw), Self::Token)
    }
}

fn remove_lock_file_if_matches(lock_path: &Path, expected: &LockFileIdentity<'_>) -> Result<bool> {
    let current = match fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(Error::Io(error)),
    };
    let matches = match expected {
        LockFileIdentity::Token(token) => parse_lock_token(&current) == Some(*token),
        LockFileIdentity::Raw(raw) => current == *raw,
    };
    if !matches {
        return Ok(false);
    }
    match fs::remove_file(lock_path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(Error::Io(error)),
    }
}

fn lock_created_at_unix_millis(lock_path: &Path, raw: &str) -> Option<u64> {
    if let Some(created_at) = parse_lock_created_at_unix_millis(raw) {
        return Some(created_at);
    }
    match fs::metadata(lock_path).and_then(|metadata| metadata.modified()) {
        Ok(modified) => Some(system_time_unix_millis(modified)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(0),
        Err(_) => None,
    }
}

fn parse_lock_token(raw: &str) -> Option<&str> {
    raw.lines()
        .find_map(|line| line.strip_prefix("token=").map(str::trim))
        .filter(|token| !token.is_empty())
}

fn lock_owner_may_still_be_running(raw: &str) -> bool {
    parse_lock_owner_pid(raw).is_some_and(process_may_still_be_running)
}

fn parse_lock_owner_pid(raw: &str) -> Option<u32> {
    raw.lines().find_map(|line| {
        line.strip_prefix("pid=")?
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|pid| *pid > 0)
    })
}

#[cfg(unix)]
fn process_may_still_be_running(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    if Path::new("/proc").exists() {
        return fs::metadata(format!("/proc/{pid}")).is_ok();
    }
    process_exists_by_kill_probe(pid).unwrap_or(true)
}

#[cfg(unix)]
fn process_exists_by_kill_probe(pid: u32) -> Option<bool> {
    let output = std::process::Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.to_string())
        .env("LC_ALL", "C")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .or_else(|_| {
            std::process::Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .env("LC_ALL", "C")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .output()
        })
        .ok()?;
    if output.status.success() {
        return Some(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if stderr.contains("no such process")
        || stderr.contains("invalid process")
        || stderr.contains("illegal process")
    {
        Some(false)
    } else {
        Some(true)
    }
}

#[cfg(windows)]
fn process_may_still_be_running(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    process_exists_by_tasklist(pid).unwrap_or(true)
}

#[cfg(windows)]
fn process_exists_by_tasklist(pid: u32) -> Option<bool> {
    let filter = format!("PID eq {pid}");
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let needle = format!("\",\"{pid}\",");
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(stdout.lines().any(|line| line.contains(&needle)))
}

#[cfg(not(any(unix, windows)))]
fn process_may_still_be_running(_pid: u32) -> bool {
    true
}

fn parse_lock_created_at_unix_millis(raw: &str) -> Option<u64> {
    raw.lines().find_map(|line| {
        line.strip_prefix("created_at_unix_millis=")?
            .trim()
            .parse::<u64>()
            .ok()
    })
}

#[cfg(not(unix))]
fn write_private_file(
    path: &Path,
    bytes: &[u8],
    acquire_replace_guard: impl FnOnce() -> Result<CredentialStoreUpdateLock>,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let mut file = match parent {
        Some(parent) => tempfile::NamedTempFile::new_in(parent)?,
        None => tempfile::NamedTempFile::new_in(".")?,
    };
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    let _replace_guard = acquire_replace_guard()?;
    file.persist(path).map_err(std::io::Error::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::private_temp_path;
    use super::{
        AccessKeyProvider, AccessKeyProviderSecret, AccessKeyRefreshKeypair,
        AccessKeyRefreshProvider, CREDENTIAL_LOCK_STALE_AFTER_MILLIS, CredentialHealthProbe,
        CredentialHealthReport, CredentialHealthScope, CredentialHealthStatus,
        CredentialHealthSummaryStatus, CredentialKind, CredentialLifecycleMetadata,
        CredentialLifecyclePolicy, CredentialLifecycleSource, CredentialLifecycleStatus,
        CredentialProfileMetadata, CredentialProfileSecrets, CredentialProfileSelection,
        CredentialProfiles, CredentialStore, CredentialStoreUpdateLock, Credentials,
        DEFAULT_CREDENTIAL_PROFILE, current_unix_millis, parse_lock_token, private_lock_path,
        reclaim_lock_path, try_create_coordination_lock_file, try_create_lock_file,
        try_create_lock_file_with_metadata_writer,
    };
    use std::io::Write;

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
    fn whitespace_only_credentials_are_treated_as_missing() -> anyhow::Result<()> {
        let credentials = Credentials {
            cookie: Some("   ".to_owned()),
            access_key: Some("\t".to_owned()),
            tv_access_key: Some("\n".to_owned()),
        };

        assert!(credentials.is_empty());
        assert_eq!(
            credentials.redacted_summary(),
            super::CredentialSource {
                has_cookie: false,
                has_access_key: false,
                has_tv_access_key: false,
            }
        );

        let mut profiles = CredentialProfiles::default();
        profiles.set_profile("default", credentials)?;
        let status = profiles.profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;

        assert!(
            status
                .credential_statuses
                .iter()
                .all(|credential| !credential.present
                    && credential.status == CredentialLifecycleStatus::Missing)
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
        assert!(
            profiles
                .profile_metadata(DEFAULT_CREDENTIAL_PROFILE)?
                .is_empty()
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
    fn legacy_flat_credentials_with_profile_like_unknown_fields_are_not_profiles()
    -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        std::fs::write(
            &path,
            r#"{
  "default_profile": "intl",
  "profiles": {
    "intl": {
      "access_key": "ignored-profile-token"
    }
  },
  "cookie": "SESSDATA=secret",
  "access_key": "access-token",
  "tv_access_key": "tv-access-token"
}"#,
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
    fn update_profile_preserves_other_profiles_and_secret_metadata() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            DEFAULT_CREDENTIAL_PROFILE,
            Credentials::default().with_cookie("SESSDATA=default"),
        )?;
        let mut default_metadata = CredentialProfileMetadata::default();
        default_metadata.set_credential(
            CredentialKind::Cookie,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::ManualImport)
                .with_checked_at_unix_millis(1_700_000_000_000),
        );
        profiles.set_profile_metadata(DEFAULT_CREDENTIAL_PROFILE, default_metadata)?;
        profiles.set_profile("intl", Credentials::default().with_access_key("ACCESS"))?;
        let mut intl_metadata = CredentialProfileMetadata::default();
        intl_metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_access_key_provider(AccessKeyProvider::BalhBiliplus)
                .with_acquired_at_unix_millis(1_700_000_000_000)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("intl", intl_metadata)?;
        let mut intl_secrets = CredentialProfileSecrets::default();
        intl_secrets.set_access_key_provider(
            AccessKeyProvider::BalhBiliplus,
            AccessKeyProviderSecret::default()
                .with_refresh_token("REFRESH")
                .with_refresh_provider(AccessKeyRefreshProvider::BilibiliMainOauth2)
                .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv),
        );
        profiles.set_profile_secrets("intl", intl_secrets)?;
        store.save_profiles(&profiles)?;

        let updated = store.update_profile("intl", |mut credentials| {
            credentials.tv_access_key = Some("TV".to_owned());
            Ok(credentials)
        })?;

        assert_eq!(updated.access_key.as_deref(), Some("ACCESS"));
        assert_eq!(updated.tv_access_key.as_deref(), Some("TV"));
        let profiles = store.load_profiles()?;
        assert_eq!(
            profiles
                .profile(DEFAULT_CREDENTIAL_PROFILE)?
                .cookie
                .as_deref(),
            Some("SESSDATA=default")
        );
        assert!(
            profiles
                .profile_metadata(DEFAULT_CREDENTIAL_PROFILE)?
                .credential(CredentialKind::Cookie)
                .is_some()
        );
        assert!(
            profiles
                .profile_metadata("intl")?
                .credential(CredentialKind::AccessKey)
                .is_some()
        );
        assert_eq!(
            profiles
                .profile_secrets("intl")?
                .access_key_provider(AccessKeyProvider::BalhBiliplus)
                .and_then(|secret| secret.refresh_token.as_deref()),
            Some("REFRESH")
        );
        Ok(())
    }

    #[test]
    fn update_selected_profile_uses_current_default_profile() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            DEFAULT_CREDENTIAL_PROFILE,
            Credentials::default().with_cookie("SESSDATA=default"),
        )?;
        profiles.set_profile("intl", Credentials::default().with_access_key("ACCESS"))?;
        profiles.set_default_profile("intl")?;
        store.save_profiles(&profiles)?;

        store.update_selected_profile(
            &CredentialProfileSelection::default_profile(),
            |mut stored| {
                stored.tv_access_key = Some("TV".to_owned());
                Ok(stored)
            },
        )?;

        assert_eq!(
            store
                .load_profile(DEFAULT_CREDENTIAL_PROFILE)?
                .cookie
                .as_deref(),
            Some("SESSDATA=default")
        );
        assert_eq!(
            store.load_profile("intl")?.access_key.as_deref(),
            Some("ACCESS")
        );
        assert_eq!(
            store.load_profile("intl")?.tv_access_key.as_deref(),
            Some("TV")
        );
        Ok(())
    }

    #[test]
    fn update_selected_profile_preserves_flat_default_store_format() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        store.save(&Credentials {
            cookie: Some("SESSDATA=default".to_owned()),
            access_key: Some("ACCESS".to_owned()),
            tv_access_key: None,
        })?;

        store.update_selected_profile(
            &CredentialProfileSelection::default_profile(),
            |mut stored| {
                stored.tv_access_key = Some("TV".to_owned());
                Ok(stored)
            },
        )?;

        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(raw.get("profiles").is_none());
        assert!(raw.get("default_profile").is_none());
        assert_eq!(
            raw.get("cookie").and_then(serde_json::Value::as_str),
            Some("SESSDATA=default")
        );
        assert_eq!(
            raw.get("access_key").and_then(serde_json::Value::as_str),
            Some("ACCESS")
        );
        assert_eq!(
            raw.get("tv_access_key").and_then(serde_json::Value::as_str),
            Some("TV")
        );
        Ok(())
    }

    #[test]
    fn update_profiles_noop_does_not_create_missing_store() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());

        store.update_profiles(|profiles| {
            assert_eq!(profiles.default_credentials(), Credentials::default());
            Ok(())
        })?;

        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn update_profiles_noop_preserves_flat_store_format() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        store.save(&Credentials {
            cookie: Some("SESSDATA=default".to_owned()),
            access_key: None,
            tv_access_key: None,
        })?;
        let before = std::fs::read_to_string(&path)?;

        store.update_profiles(|profiles| {
            assert_eq!(
                profiles.default_credentials().cookie.as_deref(),
                Some("SESSDATA=default")
            );
            Ok(())
        })?;

        assert_eq!(std::fs::read_to_string(path)?, before);
        Ok(())
    }

    #[test]
    fn update_profiles_reports_lock_contention() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let lock_path = private_lock_path(&path);
        std::fs::write(&lock_path, "other process")?;

        let Err(error) = store.update_profile("intl", Ok) else {
            anyhow::bail!("lock contention must fail");
        };

        assert!(error.to_string().contains("credential store is locked"));
        assert_eq!(std::fs::read_to_string(lock_path)?, "other process");
        Ok(())
    }

    #[test]
    fn update_profiles_reclaims_stale_lock() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let lock_path = private_lock_path(&path);
        let stale_created_at = current_unix_millis()
            .saturating_sub(CREDENTIAL_LOCK_STALE_AFTER_MILLIS)
            .saturating_sub(1);
        std::fs::write(
            &lock_path,
            format!("created_at_unix_millis={stale_created_at}\n"),
        )?;

        store.update_profile("intl", |mut credentials| {
            credentials.access_key = Some("ACCESS".to_owned());
            Ok(credentials)
        })?;

        assert_eq!(
            store.load_profile("intl")?.access_key.as_deref(),
            Some("ACCESS")
        );
        assert!(!lock_path.exists());
        Ok(())
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn process_liveness_reports_current_process() {
        assert!(super::process_may_still_be_running(std::process::id()));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn update_profiles_reclaims_stale_lock_with_dead_owner() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let lock_path = private_lock_path(&path);
        let stale_created_at = current_unix_millis()
            .saturating_sub(CREDENTIAL_LOCK_STALE_AFTER_MILLIS)
            .saturating_sub(1);
        std::fs::write(
            &lock_path,
            format!(
                "token=dead-owner\npid={}\ncreated_at_unix_millis={stale_created_at}\n",
                u32::MAX
            ),
        )?;

        store.update_profile("intl", |mut credentials| {
            credentials.access_key = Some("ACCESS".to_owned());
            Ok(credentials)
        })?;

        assert_eq!(
            store.load_profile("intl")?.access_key.as_deref(),
            Some("ACCESS")
        );
        assert!(!lock_path.exists());
        Ok(())
    }

    #[test]
    fn update_profiles_keeps_stale_lock_with_live_owner() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let lock_path = private_lock_path(&path);
        let stale_created_at = current_unix_millis()
            .saturating_sub(CREDENTIAL_LOCK_STALE_AFTER_MILLIS)
            .saturating_sub(1);
        let live_lock = format!(
            "token=live-owner\npid={}\ncreated_at_unix_millis={stale_created_at}\n",
            std::process::id()
        );
        std::fs::write(&lock_path, &live_lock)?;

        let Err(error) = store.update_profile("intl", |mut credentials| {
            credentials.access_key = Some("ACCESS".to_owned());
            Ok(credentials)
        }) else {
            anyhow::bail!("live stale lock owner must not be reclaimed");
        };

        assert!(error.to_string().contains("credential store is locked"));
        assert_eq!(std::fs::read_to_string(lock_path)?, live_lock);
        Ok(())
    }

    #[test]
    fn stale_lock_reclaim_contention_preserves_existing_lock() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let lock_path = private_lock_path(&path);
        let stale_created_at = current_unix_millis()
            .saturating_sub(CREDENTIAL_LOCK_STALE_AFTER_MILLIS)
            .saturating_sub(1);
        let stale_lock = format!("token=stale-owner\ncreated_at_unix_millis={stale_created_at}\n");
        std::fs::write(&lock_path, &stale_lock)?;
        std::fs::write(reclaim_lock_path(&lock_path), "another reclaim")?;

        let Err(error) = store.update_profile("intl", Ok) else {
            anyhow::bail!("reclaim contention must fail");
        };

        assert!(error.to_string().contains("credential store is locked"));
        assert_eq!(std::fs::read_to_string(lock_path)?, stale_lock);
        Ok(())
    }

    #[test]
    fn update_profiles_waits_for_lock_coordination_guard() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let lock_path = private_lock_path(&path);
        let Some(_coordination_guard) =
            try_create_coordination_lock_file(&reclaim_lock_path(&lock_path))?
        else {
            anyhow::bail!("coordination guard should be acquired");
        };

        let Err(error) = store.update_profile("intl", |mut credentials| {
            credentials.access_key = Some("ACCESS".to_owned());
            Ok(credentials)
        }) else {
            anyhow::bail!("coordination contention must fail");
        };

        assert!(error.to_string().contains("credential store is locked"));
        assert!(!lock_path.exists());
        Ok(())
    }

    #[test]
    fn lock_release_waits_for_lock_coordination_guard() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let lock_path = private_lock_path(&path);
        let guard = store.acquire_update_lock()?;
        let Some(coordination_guard) =
            try_create_coordination_lock_file(&reclaim_lock_path(&lock_path))?
        else {
            anyhow::bail!("coordination guard should be acquired");
        };

        let (release_started_tx, release_started_rx) = std::sync::mpsc::channel();
        let (release_done_tx, release_done_rx) = std::sync::mpsc::channel();
        let lock_path_for_thread = lock_path.clone();
        let release = std::thread::spawn(move || {
            let _ = release_started_tx.send(());
            drop(guard);
            let _ = release_done_tx.send(lock_path_for_thread.exists());
        });

        release_started_rx.recv_timeout(std::time::Duration::from_secs(1))?;
        assert!(matches!(
            release_done_rx.recv_timeout(std::time::Duration::from_millis(
                super::CREDENTIAL_LOCK_RELEASE_RETRY_MILLIS.saturating_mul(5),
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(lock_path.exists());

        drop(coordination_guard);
        let lock_exists_after_release =
            release_done_rx.recv_timeout(std::time::Duration::from_secs(1))?;
        let Ok(()) = release.join() else {
            anyhow::bail!("release thread should not panic");
        };
        assert!(!lock_exists_after_release);
        assert!(!lock_path.exists());
        Ok(())
    }

    #[test]
    fn update_profiles_reclaims_stale_reclaim_lock() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let lock_path = private_lock_path(&path);
        let stale_created_at = current_unix_millis()
            .saturating_sub(CREDENTIAL_LOCK_STALE_AFTER_MILLIS)
            .saturating_sub(1);
        std::fs::write(
            &lock_path,
            format!("token=stale-owner\ncreated_at_unix_millis={stale_created_at}\n"),
        )?;
        std::fs::write(
            reclaim_lock_path(&lock_path),
            format!("token=stale-reclaim\ncreated_at_unix_millis={stale_created_at}\n"),
        )?;

        store.update_profile("intl", |mut credentials| {
            credentials.access_key = Some("ACCESS".to_owned());
            Ok(credentials)
        })?;

        assert_eq!(
            store.load_profile("intl")?.access_key.as_deref(),
            Some("ACCESS")
        );
        assert!(!lock_path.exists());
        assert!(!reclaim_lock_path(&lock_path).exists());
        Ok(())
    }

    #[test]
    fn try_create_lock_file_cleans_up_after_metadata_write_error() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let lock_path = private_lock_path(&temp.path().join("credentials.json"));

        let Err(error) =
            try_create_lock_file_with_metadata_writer(&lock_path, |file, token, _created_at| {
                writeln!(file, "token={token}")?;
                Err(crate::Error::InvalidInput(
                    "metadata write failed".to_owned(),
                ))
            })
        else {
            anyhow::bail!("metadata write failure must fail lock creation");
        };

        assert!(error.to_string().contains("metadata write failed"));
        assert!(!lock_path.exists());
        assert!(try_create_lock_file(&lock_path)?.is_some());
        Ok(())
    }

    #[test]
    fn stale_lock_owner_drop_does_not_remove_reclaimed_lock() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let lock_path = private_lock_path(&path);
        let stale_created_at = current_unix_millis()
            .saturating_sub(CREDENTIAL_LOCK_STALE_AFTER_MILLIS)
            .saturating_sub(1);
        let stale_token = "stale-owner";
        std::fs::write(
            &lock_path,
            format!("token={stale_token}\ncreated_at_unix_millis={stale_created_at}\n"),
        )?;
        let stale_guard = CredentialStoreUpdateLock {
            path: lock_path.clone(),
            token: stale_token.to_owned(),
            coordinate_release: true,
        };

        let active_guard = store.acquire_update_lock()?;
        let active_lock = std::fs::read_to_string(&lock_path)?;
        assert_ne!(parse_lock_token(&active_lock), Some(stale_token));

        drop(stale_guard);

        assert_eq!(std::fs::read_to_string(&lock_path)?, active_lock);
        drop(active_guard);
        assert!(!lock_path.exists());
        Ok(())
    }

    #[test]
    fn stale_lock_owner_resume_after_reclaim_does_not_write_stale_snapshot() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let competing_store = CredentialStore::new(path.clone());
        store.save_profile(
            "intl",
            &Credentials {
                cookie: None,
                access_key: Some("INITIAL".to_owned()),
                tv_access_key: None,
            },
        )?;

        let lock_path = private_lock_path(&path);
        let Err(error) = store.update_profiles::<()>(|profiles| {
            let mut stale_credentials = profiles.profile("intl")?;
            stale_credentials.access_key = Some("STALE".to_owned());
            profiles.set_profile("intl", stale_credentials)?;

            let raw_lock = std::fs::read_to_string(&lock_path)?;
            let token = parse_lock_token(&raw_lock)
                .ok_or_else(|| crate::Error::InvalidInput("lock token should exist".to_owned()))?
                .to_owned();
            let stale_created_at = current_unix_millis()
                .saturating_sub(CREDENTIAL_LOCK_STALE_AFTER_MILLIS)
                .saturating_sub(1);
            std::fs::write(
                &lock_path,
                format!("token={token}\ncreated_at_unix_millis={stale_created_at}\n"),
            )?;

            competing_store.update_profile("intl", |mut credentials| {
                credentials.access_key = Some("NEWER".to_owned());
                Ok(credentials)
            })?;
            Ok(())
        }) else {
            anyhow::bail!("resumed stale lock owner must fail before writing");
        };

        assert!(
            error
                .to_string()
                .contains("credential store lock was reclaimed before write")
        );
        assert_eq!(
            store.load_profile("intl")?.access_key.as_deref(),
            Some("NEWER")
        );
        assert!(!lock_path.exists());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn write_credentials_fences_after_temp_file_write_before_replace() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        store.save(&Credentials {
            cookie: None,
            access_key: Some("INITIAL".to_owned()),
            tv_access_key: None,
        })?;

        let lock_path = private_lock_path(&path);
        let guard = store.acquire_update_lock()?;
        std::fs::write(
            &lock_path,
            "token=newer-writer\npid=1\ncreated_at_unix_millis=1\n",
        )?;

        let Err(error) = store.write_credentials_locked(
            &Credentials {
                cookie: None,
                access_key: Some("STALE".to_owned()),
                tv_access_key: None,
            },
            &guard,
        ) else {
            anyhow::bail!("reclaimed writer must fail before replacing the credential file");
        };

        assert!(
            error
                .to_string()
                .contains("credential store lock was reclaimed before write")
        );
        assert_eq!(store.load()?.access_key.as_deref(), Some("INITIAL"));
        assert!(!private_temp_path(&path).exists());
        Ok(())
    }

    #[test]
    fn update_profiles_releases_lock_after_update_error() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());

        let Err(error) =
            store.update_profiles::<()>(|_| Err(crate::Error::InvalidInput("boom".to_owned())))
        else {
            anyhow::bail!("update error must fail");
        };

        assert!(error.to_string().contains("boom"));
        assert!(!private_lock_path(&path).exists());
        Ok(())
    }

    #[test]
    fn profile_metadata_round_trips_lifecycle_fields() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_acquired_at_unix_millis(1_710_000_000_000)
                .with_checked_at_unix_millis(1_710_000_010_000)
                .with_expires_at_unix_millis(1_710_007_200_000)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("intl", metadata)?;

        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert_eq!(
            value["profile_metadata"]["intl"]["credentials"]["access_key"]["source"].as_str(),
            Some("access_key_login")
        );
        assert_eq!(
            value["profile_metadata"]["intl"]["credentials"]["access_key"]["refresh_token_present"]
                .as_bool(),
            Some(true)
        );
        assert!(
            !value["profile_metadata"]
                .to_string()
                .contains("access-token"),
            "metadata must not duplicate raw credential values"
        );
        let loaded = store.load_profiles()?;
        let metadata = loaded.profile_metadata("intl")?;
        assert_eq!(
            metadata
                .credential(CredentialKind::AccessKey)
                .map(|lifecycle| lifecycle.source),
            Some(Some(CredentialLifecycleSource::AccessKeyLogin))
        );
        assert_eq!(
            metadata
                .credential(CredentialKind::AccessKey)
                .and_then(|lifecycle| lifecycle.expires_at_unix_millis),
            Some(1_710_007_200_000)
        );
        assert_eq!(
            metadata
                .credential(CredentialKind::AccessKey)
                .and_then(|lifecycle| lifecycle.refresh_token_present),
            Some(true)
        );
        Ok(())
    }

    #[test]
    fn profile_secrets_round_trip_without_debug_leakage() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_access_key_provider(AccessKeyProvider::BalhBiliplus)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("intl", metadata)?;
        let mut secrets = CredentialProfileSecrets::default();
        secrets.set_access_key_provider(
            AccessKeyProvider::BalhBiliplus,
            AccessKeyProviderSecret::default()
                .with_refresh_token("refresh-secret")
                .with_refresh_provider(AccessKeyRefreshProvider::BilibiliMainOauth2)
                .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv),
        );
        profiles.set_profile_secrets("intl", secrets)?;

        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        assert_eq!(
            value["profile_secrets"]["intl"]["access_key"]["balh_biliplus"]["refresh_token"]
                .as_str(),
            Some("refresh-secret")
        );
        assert_eq!(
            value["profile_secrets"]["intl"]["access_key"]["balh_biliplus"]["refresh_provider"]
                .as_str(),
            Some("bilibili_main_oauth2")
        );
        assert_eq!(
            value["profile_secrets"]["intl"]["access_key"]["balh_biliplus"]["refresh_keypair"]
                .as_str(),
            Some("bili_tv")
        );
        let debug = format!("{:?}", store.load_profiles()?);
        assert!(debug.contains("has_refresh_token: true"));
        assert!(!debug.contains("refresh-secret"));

        let policy = CredentialLifecyclePolicy::at_unix_millis(1_000);
        let status = store
            .load_profiles()?
            .profile_lifecycle_status("intl", &policy)?;
        let access_key_status = status
            .credential_statuses
            .iter()
            .find(|status| status.kind == CredentialKind::AccessKey)
            .ok_or_else(|| anyhow::anyhow!("access-key status should exist"))?;
        assert_eq!(
            access_key_status.access_key_provider,
            Some(AccessKeyProvider::BalhBiliplus)
        );
        assert_eq!(access_key_status.refresh_token_secret_present, Some(true));
        Ok(())
    }

    #[test]
    fn whitespace_refresh_token_secret_is_not_lifecycle_ready() -> anyhow::Result<()> {
        assert!(AccessKeyProviderSecret::default().is_empty());
        assert!(
            AccessKeyProviderSecret::default()
                .with_refresh_token("   ")
                .is_empty()
        );
        assert!(
            !AccessKeyProviderSecret::default()
                .with_refresh_token("   ")
                .has_refresh_token()
        );

        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("ACCESS_SECRET".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_access_key_provider(AccessKeyProvider::BalhBiliplus)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("intl", metadata)?;
        let mut secrets = CredentialProfileSecrets::default();
        secrets.set_access_key_provider(
            AccessKeyProvider::BalhBiliplus,
            AccessKeyProviderSecret::default()
                .with_refresh_token("   ")
                .with_refresh_provider(AccessKeyRefreshProvider::BilibiliMainOauth2)
                .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv),
        );
        profiles.set_profile_secrets("intl", secrets)?;

        let status = profiles.profile_lifecycle_status(
            "intl",
            &CredentialLifecyclePolicy::at_unix_millis(1_700_000_000_000),
        )?;
        let access_key_status = status
            .credential_statuses
            .iter()
            .find(|status| status.kind == CredentialKind::AccessKey)
            .ok_or_else(|| anyhow::anyhow!("access-key status should exist"))?;
        assert_eq!(access_key_status.refresh_token_secret_present, Some(false));
        Ok(())
    }

    #[test]
    fn missing_refresh_secret_reports_false_when_provider_metadata_exists() -> anyhow::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("ACCESS_SECRET".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_access_key_provider(AccessKeyProvider::BalhBiliplus)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("intl", metadata)?;

        let status = profiles.profile_lifecycle_status(
            "intl",
            &CredentialLifecyclePolicy::at_unix_millis(1_700_000_000_000),
        )?;
        let access_key_status = status
            .credential_statuses
            .iter()
            .find(|status| status.kind == CredentialKind::AccessKey)
            .ok_or_else(|| anyhow::anyhow!("access-key status should exist"))?;
        assert_eq!(access_key_status.refresh_token_secret_present, Some(false));
        Ok(())
    }

    #[test]
    fn save_profile_drops_provider_secrets_for_changed_access_key() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("old-access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut secrets = CredentialProfileSecrets::default();
        secrets.set_access_key_provider(
            AccessKeyProvider::BalhBiliplus,
            AccessKeyProviderSecret::default()
                .with_refresh_token("refresh-secret")
                .with_refresh_provider(AccessKeyRefreshProvider::BilibiliMainOauth2),
        );
        profiles.set_profile_secrets("intl", secrets)?;
        store.save_profiles(&profiles)?;

        store.save_profile(
            "intl",
            &Credentials {
                cookie: None,
                access_key: Some("new-access-token".to_owned()),
                tv_access_key: None,
            },
        )?;

        let loaded = store.load_profiles()?;
        assert_eq!(
            loaded.profile("intl")?.access_key.as_deref(),
            Some("new-access-token")
        );
        assert!(loaded.profile_secrets("intl")?.is_empty());
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_secrets").is_none());
        Ok(())
    }

    #[test]
    fn save_profile_preserves_profile_metadata() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path);
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("old-access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("intl", metadata)?;
        store.save_profiles(&profiles)?;

        store.save_profile(
            "intl",
            &Credentials {
                cookie: None,
                access_key: Some("old-access-token".to_owned()),
                tv_access_key: None,
            },
        )?;

        let loaded = store.load_profiles()?;
        assert_eq!(
            loaded.profile("intl")?.access_key.as_deref(),
            Some("old-access-token")
        );
        assert_eq!(
            loaded
                .profile_metadata("intl")?
                .credential(CredentialKind::AccessKey)
                .and_then(|lifecycle| lifecycle.refresh_token_present),
            Some(true)
        );
        Ok(())
    }

    #[test]
    fn save_profile_drops_metadata_for_changed_credential_value() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("old-access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_expires_at_unix_millis(1_710_007_200_000),
        );
        profiles.set_profile_metadata("intl", metadata)?;
        store.save_profiles(&profiles)?;

        store.save_profile(
            "intl",
            &Credentials {
                cookie: None,
                access_key: Some("new-access-token".to_owned()),
                tv_access_key: None,
            },
        )?;

        let loaded = store.load_profiles()?;
        assert_eq!(
            loaded.profile("intl")?.access_key.as_deref(),
            Some("new-access-token")
        );
        assert!(loaded.profile_metadata("intl")?.is_empty());
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn save_profile_prunes_metadata_for_removed_credential_kind() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: Some("SESSDATA=old".to_owned()),
                access_key: Some("access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("intl", metadata)?;
        store.save_profiles(&profiles)?;

        store.save_profile(
            "intl",
            &Credentials {
                cookie: Some("SESSDATA=new".to_owned()),
                access_key: None,
                tv_access_key: None,
            },
        )?;

        let loaded = store.load_profiles()?;
        assert_eq!(
            loaded.profile("intl")?.cookie.as_deref(),
            Some("SESSDATA=new")
        );
        assert!(loaded.profile_metadata("intl")?.is_empty());
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn set_profile_prunes_in_memory_metadata_for_removed_credential_kind() -> anyhow::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: Some("SESSDATA=old".to_owned()),
                access_key: Some("access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("intl", metadata)?;

        profiles.set_profile(
            "intl",
            Credentials {
                cookie: Some("SESSDATA=new".to_owned()),
                access_key: None,
                tv_access_key: None,
            },
        )?;

        assert!(profiles.profile_metadata("intl")?.is_empty());
        Ok(())
    }

    #[test]
    fn set_profile_drops_in_memory_metadata_for_changed_credential_value() -> anyhow::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("old-access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_expires_at_unix_millis(1_710_007_200_000),
        );
        profiles.set_profile_metadata("intl", metadata)?;

        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("new-access-token".to_owned()),
                tv_access_key: None,
            },
        )?;

        assert!(profiles.profile_metadata("intl")?.is_empty());
        Ok(())
    }

    #[test]
    fn set_profile_metadata_filters_missing_credential_kinds_in_memory() -> anyhow::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: Some("SESSDATA=secret".to_owned()),
                access_key: None,
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::Cookie,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::ManualImport),
        );
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin),
        );

        profiles.set_profile_metadata("intl", metadata)?;

        let metadata = profiles.profile_metadata("intl")?;
        assert!(metadata.credential(CredentialKind::AccessKey).is_none());
        assert_eq!(
            metadata
                .credential(CredentialKind::Cookie)
                .and_then(|lifecycle| lifecycle.source),
            Some(CredentialLifecycleSource::ManualImport)
        );
        Ok(())
    }

    #[test]
    fn set_profile_metadata_drops_metadata_without_profile() -> anyhow::Result<()> {
        let mut profiles = CredentialProfiles::default();
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin),
        );

        profiles.set_profile_metadata("missing", metadata)?;

        assert!(profiles.profile_metadata("missing")?.is_empty());
        Ok(())
    }

    #[test]
    fn unknown_lifecycle_source_deserializes_as_unknown() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path);
        std::fs::write(
            store.path(),
            r#"{
  "version": 1,
  "default_profile": "intl",
  "profiles": {
    "intl": {
      "access_key": "access-token"
    }
  },
  "profile_metadata": {
    "intl": {
      "credentials": {
        "access_key": {
          "source": "future_login"
        }
      }
    }
  }
}"#,
        )?;

        let profiles = store.load_profiles()?;
        assert_eq!(
            profiles
                .profile_metadata("intl")?
                .credential(CredentialKind::AccessKey)
                .and_then(|lifecycle| lifecycle.source),
            Some(CredentialLifecycleSource::Unknown)
        );
        Ok(())
    }

    #[test]
    fn unknown_metadata_credential_kind_is_ignored() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "default_profile": "intl",
  "profiles": {
    "intl": {
      "access_key": "access-token"
    }
  },
  "profile_metadata": {
    "intl": {
      "credentials": {
        "access_key": {
          "source": "access_key_login"
        },
        "future_key": {
          "source": "future_login"
        }
      }
    }
  }
}"#,
        )?;

        let profiles = store.load_profiles()?;
        assert_eq!(
            profiles
                .profile_metadata("intl")?
                .credential(CredentialKind::AccessKey)
                .and_then(|lifecycle| lifecycle.source),
            Some(CredentialLifecycleSource::AccessKeyLogin)
        );
        store.save_profiles(&profiles)?;

        let value = std::fs::read_to_string(path)?;
        assert!(!value.contains("future_key"));
        Ok(())
    }

    #[test]
    fn profile_lifecycle_status_evaluates_policy_without_secrets() -> anyhow::Result<()> {
        let now = 1_700_000_000_000;
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: Some("SESSDATA=COOKIE_SECRET".to_owned()),
                access_key: Some("ACCESS_SECRET".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::Cookie,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::ManualImport)
                .with_checked_at_unix_millis(now - 2_000),
        );
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_expires_at_unix_millis(now + 200)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("intl", metadata)?;
        let policy = CredentialLifecyclePolicy::at_unix_millis(now)
            .with_stale_after_millis(Some(1_000))
            .with_expiring_within_millis(Some(500));

        let status = profiles.profile_lifecycle_status("intl", &policy)?;

        assert_eq!(status.profile, "intl");
        assert_eq!(status.status, CredentialLifecycleStatus::Expiring);
        assert_eq!(
            status
                .credential_statuses
                .iter()
                .map(|status| (status.kind, status.present, status.status))
                .collect::<Vec<_>>(),
            vec![
                (
                    CredentialKind::Cookie,
                    true,
                    CredentialLifecycleStatus::Stale
                ),
                (
                    CredentialKind::AccessKey,
                    true,
                    CredentialLifecycleStatus::Expiring,
                ),
                (
                    CredentialKind::TvAccessKey,
                    false,
                    CredentialLifecycleStatus::Missing,
                ),
            ]
        );
        let serialized = serde_json::to_string(&status)?;
        assert!(serialized.contains("\"status\":\"expiring\""));
        assert!(!serialized.contains("COOKIE_SECRET"));
        assert!(!serialized.contains("ACCESS_SECRET"));
        Ok(())
    }

    #[test]
    fn lifecycle_statuses_include_default_and_named_profiles() -> anyhow::Result<()> {
        let now = 1_700_000_000_000;
        let mut profiles = CredentialProfiles::default();
        profiles.set_default_profile("intl")?;
        profiles.set_profile(
            "web",
            Credentials {
                cookie: Some("SESSDATA=COOKIE_SECRET".to_owned()),
                access_key: None,
                tv_access_key: None,
            },
        )?;

        let statuses =
            profiles.lifecycle_statuses(&CredentialLifecyclePolicy::at_unix_millis(now))?;

        assert_eq!(
            statuses
                .iter()
                .map(|status| (
                    status.profile.as_str(),
                    status.is_default_profile,
                    status.status
                ))
                .collect::<Vec<_>>(),
            vec![
                ("intl", true, CredentialLifecycleStatus::Missing),
                ("web", false, CredentialLifecycleStatus::Unknown),
            ]
        );
        Ok(())
    }

    #[test]
    fn lifecycle_status_treats_far_future_expiry_as_stale_when_last_seen_is_old() {
        let now = 1_700_000_000_000;
        let metadata = CredentialLifecycleMetadata::default()
            .with_checked_at_unix_millis(now - 5_000)
            .with_expires_at_unix_millis(now + 60_000);
        let policy = CredentialLifecyclePolicy::at_unix_millis(now)
            .with_stale_after_millis(Some(1_000))
            .with_expiring_within_millis(Some(500));

        let status = CredentialLifecycleStatus::from_metadata(&metadata, &policy);

        assert_eq!(status, CredentialLifecycleStatus::Stale);
    }

    #[test]
    fn credential_health_report_summary_counts_probe_statuses() -> anyhow::Result<()> {
        let report = CredentialHealthReport {
            credentials: super::CredentialSource {
                has_cookie: true,
                has_access_key: false,
                has_tv_access_key: false,
            },
            probes: vec![
                CredentialHealthProbe::valid(
                    CredentialKind::Cookie,
                    CredentialHealthScope::WebCookie,
                    "web_nav",
                ),
                CredentialHealthProbe::missing(
                    CredentialKind::AccessKey,
                    CredentialHealthScope::IntlBstar,
                ),
                CredentialHealthProbe::request_failed(
                    CredentialKind::TvAccessKey,
                    CredentialHealthScope::Tv,
                    "oauth2_info",
                    "network unavailable",
                ),
            ],
        };

        let summary = report.summary();

        assert_eq!(summary.status, CredentialHealthSummaryStatus::RequestFailed);
        assert_eq!(summary.valid_count, 1);
        assert_eq!(summary.missing_count, 1);
        assert_eq!(summary.request_failed_count, 1);
        assert_eq!(
            serde_json::to_value(summary)?.get("status"),
            Some(&serde_json::json!("request_failed"))
        );
        assert_eq!(
            report
                .probe(CredentialKind::Cookie, CredentialHealthScope::WebCookie)
                .map(|probe| probe.status),
            Some(CredentialHealthStatus::Valid)
        );
        assert!(
            report
                .probe(CredentialKind::AccessKey, CredentialHealthScope::Tv)
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn unknown_metadata_credential_kind_with_invalid_payload_is_ignored() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "default_profile": "intl",
  "profiles": {
    "intl": {
      "access_key": "access-token"
    }
  },
  "profile_metadata": {
    "intl": {
      "credentials": {
        "access_key": {
          "source": "access_key_login"
        },
        "future_key": [
          "future",
          {
            "payload": true
          }
        ]
      }
    }
  }
}"#,
        )?;

        let profiles = store.load_profiles()?;
        assert_eq!(
            profiles
                .profile_metadata("intl")?
                .credential(CredentialKind::AccessKey)
                .and_then(|lifecycle| lifecycle.source),
            Some(CredentialLifecycleSource::AccessKeyLogin)
        );
        store.save_profiles(&profiles)?;

        let value = std::fs::read_to_string(path)?;
        assert!(!value.contains("future_key"));
        Ok(())
    }

    #[test]
    fn malformed_profile_metadata_payload_is_ignored() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "default_profile": "default",
  "profiles": {
    "default": {
      "cookie": "SESSDATA=secret"
    }
  },
  "profile_metadata": {
    "default": [
      "future",
      {
        "payload": true
      }
    ]
  }
}"#,
        )?;

        let profiles = store.load_profiles()?;
        assert!(
            profiles
                .profile_metadata(DEFAULT_CREDENTIAL_PROFILE)?
                .is_empty()
        );
        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn malformed_profile_metadata_document_is_ignored() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "default_profile": "default",
  "profiles": {
    "default": {
      "cookie": "SESSDATA=secret"
    }
  },
  "profile_metadata": [
    "future",
    {
      "payload": true
    }
  ]
}"#,
        )?;

        let profiles = store.load_profiles()?;
        assert!(
            profiles
                .profile_metadata(DEFAULT_CREDENTIAL_PROFILE)?
                .is_empty()
        );
        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn malformed_known_lifecycle_metadata_payload_is_ignored() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "default_profile": "intl",
  "profiles": {
    "intl": {
      "access_key": "access-token"
    }
  },
  "profile_metadata": {
    "intl": {
      "credentials": {
        "access_key": [
          "future",
          {
            "payload": true
          }
        ]
      }
    }
  }
}"#,
        )?;

        let profiles = store.load_profiles()?;
        assert!(profiles.profile_metadata("intl")?.is_empty());
        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn malformed_lifecycle_source_is_ignored() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "default_profile": "intl",
  "profiles": {
    "intl": {
      "access_key": "access-token"
    }
  },
  "profile_metadata": {
    "intl": {
      "credentials": {
        "access_key": {
          "source": {
            "future": true
          }
        }
      }
    }
  }
}"#,
        )?;

        let profiles = store.load_profiles()?;
        assert!(profiles.profile_metadata("intl")?.is_empty());
        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn empty_lifecycle_entries_are_not_serialized() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "default_profile": "intl",
  "profiles": {
    "intl": {
      "access_key": "access-token"
    }
  },
  "profile_metadata": {
    "intl": {
      "credentials": {
        "access_key": {}
      }
    }
  }
}"#,
        )?;

        let profiles = store.load_profiles()?;
        assert!(profiles.profile_metadata("intl")?.is_empty());
        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn empty_profile_metadata_is_not_serialized() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            DEFAULT_CREDENTIAL_PROFILE,
            Credentials {
                cookie: Some("SESSDATA=default".to_owned()),
                access_key: None,
                tv_access_key: None,
            },
        )?;
        profiles.set_profile_metadata(
            DEFAULT_CREDENTIAL_PROFILE,
            CredentialProfileMetadata::default(),
        )?;

        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn removing_profile_removes_profile_metadata() -> anyhow::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "intl",
            Credentials {
                cookie: None,
                access_key: Some("access-token".to_owned()),
                tv_access_key: None,
            },
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin),
        );
        profiles.set_profile_metadata("intl", metadata)?;

        assert!(profiles.remove_profile("intl")?.is_some());

        assert!(profiles.profile_metadata("intl")?.is_empty());
        Ok(())
    }

    #[test]
    fn orphan_profile_metadata_is_dropped_on_save() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "default_profile": "default",
  "profiles": {},
  "profile_metadata": {
    "orphan": {
      "credentials": {
        "access_key": {
          "source": "access_key_login"
        }
      }
    }
  }
}"#,
        )?;

        let profiles = store.load_profiles()?;
        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn blank_profile_metadata_key_is_dropped_on_load() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{
  "version": 1,
  "default_profile": "default",
  "profiles": {
    "default": {
      "cookie": "SESSDATA=secret"
    }
  },
  "profile_metadata": {
    " ": {
      "credentials": {
        "cookie": {
          "source": "manual_import"
        }
      }
    }
  }
}"#,
        )?;

        let profiles = store.load_profiles()?;
        assert!(
            profiles
                .profile_metadata(DEFAULT_CREDENTIAL_PROFILE)?
                .is_empty()
        );
        store.save_profiles(&profiles)?;

        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        assert!(value.get("profile_metadata").is_none());
        Ok(())
    }

    #[test]
    fn selected_profile_helpers_preserve_default_and_named_profiles() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        let default_selection = CredentialProfileSelection::default_profile();
        let intl_selection = CredentialProfileSelection::named("intl")?;

        store.save_selected_profile(
            &default_selection,
            &Credentials {
                cookie: Some("SESSDATA=default".to_owned()),
                access_key: None,
                tv_access_key: None,
            },
        )?;
        store.save_selected_profile(
            &intl_selection,
            &Credentials {
                cookie: None,
                access_key: Some("intl-access".to_owned()),
                tv_access_key: None,
            },
        )?;

        let default = store.load_selected_profile(&default_selection)?;
        let intl = store.load_selected_profile(&intl_selection)?;
        assert_eq!(default.cookie.as_deref(), Some("SESSDATA=default"));
        assert_eq!(default.access_key, None);
        assert_eq!(intl.cookie, None);
        assert_eq!(intl.access_key.as_deref(), Some("intl-access"));
        assert!(default_selection.is_default());
        assert_eq!(intl_selection.profile_name(), Some("intl"));
        Ok(())
    }

    #[test]
    fn selected_profile_rejects_blank_named_profile() {
        assert!(CredentialProfileSelection::named(" ").is_err());
    }

    #[test]
    fn unsupported_profile_document_version_is_rejected() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let raw = r#"{
  "version": 2,
  "default_profile": "default",
  "profiles": {
    "default": {
      "cookie": "SESSDATA=secret"
    }
  }
}"#;
        std::fs::write(&path, raw)?;
        let store = CredentialStore::new(path.clone());

        let Err(load_error) = store.load_profiles() else {
            anyhow::bail!("unsupported profile version was accepted");
        };
        assert!(
            load_error
                .to_string()
                .contains("unsupported credential profile document version 2")
        );

        let Err(save_error) = store.save(&Credentials {
            cookie: Some("SESSDATA=updated".to_owned()),
            access_key: None,
            tv_access_key: None,
        }) else {
            anyhow::bail!("unsupported profile version was overwritten");
        };
        assert!(
            save_error
                .to_string()
                .contains("unsupported credential profile document version 2")
        );
        assert_eq!(std::fs::read_to_string(path)?, raw);
        Ok(())
    }

    #[test]
    fn malformed_profile_document_does_not_fall_back_to_flat_credentials() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let raw = r#"{
  "version": 2,
  "profiles": []
}"#;
        std::fs::write(&path, raw)?;
        let store = CredentialStore::new(path.clone());

        let Err(load_error) = store.load_profiles() else {
            anyhow::bail!("malformed profile document was accepted as flat credentials");
        };
        assert!(load_error.to_string().contains("invalid type"));

        let Err(save_error) = store.save(&Credentials {
            cookie: Some("SESSDATA=updated".to_owned()),
            access_key: None,
            tv_access_key: None,
        }) else {
            anyhow::bail!("malformed profile document was overwritten as flat credentials");
        };
        assert!(save_error.to_string().contains("invalid type"));
        assert_eq!(std::fs::read_to_string(path)?, raw);
        Ok(())
    }

    #[test]
    fn malformed_existing_store_is_not_overwritten_by_save() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let raw = r#"{
  "version": 1,
  "profiles": {
    "default": {
      "cookie": "SESSDATA=secret"
    }
  }
"#;
        std::fs::write(&path, raw)?;
        let store = CredentialStore::new(path.clone());

        let Err(save_error) = store.save(&Credentials {
            cookie: Some("SESSDATA=updated".to_owned()),
            access_key: None,
            tv_access_key: None,
        }) else {
            anyhow::bail!("syntax-invalid credential store was overwritten");
        };
        assert!(save_error.to_string().contains("EOF"));
        assert_eq!(std::fs::read_to_string(path)?, raw);
        Ok(())
    }

    #[test]
    fn invalid_profile_document_default_profile_is_rejected() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let raw = r#"{
  "version": 1,
  "default_profile": " ",
  "profiles": {
    "intl": {
      "access_key": "access-token"
    }
  }
}"#;
        std::fs::write(&path, raw)?;
        let store = CredentialStore::new(path.clone());

        let Err(load_error) = store.load() else {
            anyhow::bail!("invalid default profile was accepted");
        };
        assert!(
            load_error
                .to_string()
                .contains("credential profile name must not be empty")
        );

        let Err(save_error) = store.save(&Credentials {
            cookie: Some("SESSDATA=updated".to_owned()),
            access_key: None,
            tv_access_key: None,
        }) else {
            anyhow::bail!("invalid default profile was overwritten");
        };
        assert!(
            save_error
                .to_string()
                .contains("credential profile name must not be empty")
        );
        assert_eq!(std::fs::read_to_string(path)?, raw);
        Ok(())
    }

    #[test]
    fn remove_missing_profile_does_not_create_store() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());

        let removed = store.remove_profile("intl")?;

        assert_eq!(removed, None);
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn remove_missing_profile_keeps_legacy_flat_store() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("credentials.json");
        let store = CredentialStore::new(path.clone());
        store.save(&Credentials {
            cookie: Some("SESSDATA=secret".to_owned()),
            access_key: Some("access-token".to_owned()),
            tv_access_key: None,
        })?;
        let before = std::fs::read_to_string(&path)?;

        let removed = store.remove_profile("intl")?;

        assert_eq!(removed, None);
        assert_eq!(std::fs::read_to_string(path)?, before);
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

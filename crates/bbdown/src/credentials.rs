use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_CREDENTIAL_PROFILE: &str = "default";
const CREDENTIAL_PROFILES_VERSION: u32 = 1;

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
                .filter(|value| !value.is_empty()),
            Self::AccessKey => credentials
                .access_key
                .as_deref()
                .filter(|value| !value.is_empty()),
            Self::TvAccessKey => credentials
                .tv_access_key
                .as_deref()
                .filter(|value| !value.is_empty()),
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
        let mut profiles = self.load_profiles()?;
        profiles.set_profile(profile, credentials.clone())?;
        self.save_profiles(&profiles)
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
        let file_uses_profiles = self.file_uses_profiles()?;
        let mut profiles = self.load_profiles()?;
        let removed = profiles.remove_profile(profile)?;
        if removed.is_some() || file_uses_profiles {
            self.save_profiles(&profiles)?;
        }
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
        if raw.trim().is_empty() {
            return Ok(false);
        }
        let value = serde_json::from_str(&raw)?;
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
    use super::{
        AccessKeyProvider, AccessKeyProviderSecret, AccessKeyRefreshKeypair,
        AccessKeyRefreshProvider, CredentialHealthProbe, CredentialHealthReport,
        CredentialHealthScope, CredentialHealthStatus, CredentialHealthSummaryStatus,
        CredentialKind, CredentialLifecycleMetadata, CredentialLifecyclePolicy,
        CredentialLifecycleSource, CredentialLifecycleStatus, CredentialProfileMetadata,
        CredentialProfileSecrets, CredentialProfileSelection, CredentialProfiles, CredentialStore,
        Credentials, DEFAULT_CREDENTIAL_PROFILE,
    };

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

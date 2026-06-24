use crate::{
    AccessKeyProvider, AccessKeyRefreshKeypair, AccessKeyRefreshProvider, BiliClient,
    CredentialKind, CredentialLifecycleCredentialStatus, CredentialLifecycleSource,
    CredentialLifecycleStatus, CredentialProfileLifecycleStatus, Credentials, Error, Result,
};
use md5::Digest;
use rand::rngs::OsRng;
use reqwest::header::{ACCEPT, COOKIE, HeaderMap, HeaderValue, ORIGIN, REFERER, SET_COOKIE};
use rsa::{Oaep, RsaPublicKey, pkcs8::DecodePublicKey};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

const WEB_QR_WAITING_SCAN: i64 = 86_101;
const WEB_QR_WAITING_CONFIRM: i64 = 86_090;
const WEB_QR_EXPIRED: i64 = 86_038;
const TV_QR_WAITING_SCAN: i64 = 86_039;
const TV_QR_WAITING_CONFIRM: i64 = 86_090;
const TV_QR_EXPIRED: i64 = 86_038;
const BILIPLUS_ACCESS_KEY_LOGIN_BASE: &str = "https://www.biliplus.com";
const BALH_LOGIN_CREDENTIALS_PREFIX: &str = "balh-login-credentials:";
const WEB_COOKIE_REFRESH_PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\n\
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDLgd2OAkcGVtoE3ThUREbio0Eg\n\
Uc/prcajMKXvkCKFCWhJYJcLkcM2DKKcSeFpD/j6Boy538YXnR6VhcuUJOhH2x71\n\
nzPjfdTcqMz7djHum0qSZA0AyCBDABUqCrfNgCiJ00Ra7GmRj+YCK1NJEuewlb40\n\
JNrRuoEUXpabUzGB8QIDAQAB\n\
-----END PUBLIC KEY-----";

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccessKeyLoginConfig {
    pub auth_base: String,
    pub callback_origin: String,
}

impl AccessKeyLoginConfig {
    pub fn new(auth_base: impl AsRef<str>, callback_origin: impl AsRef<str>) -> Result<Self> {
        Ok(Self {
            auth_base: normalize_http_url(auth_base.as_ref(), "access-key auth base")?,
            callback_origin: http_origin(callback_origin.as_ref(), "access-key callback origin")?,
        })
    }

    pub fn biliplus(callback_origin: impl AsRef<str>) -> Result<Self> {
        Self::new(BILIPLUS_ACCESS_KEY_LOGIN_BASE, callback_origin)
    }

    pub fn ticket(&self) -> Result<AccessKeyLoginTicket> {
        let auth_base = normalize_http_url(&self.auth_base, "access-key auth base")?;
        let callback_origin = http_origin(&self.callback_origin, "access-key callback origin")?;
        let message_origin = http_origin(&auth_base, "access-key auth base")?;
        let mut url = BiliClient::endpoint_url(&auth_base, "/login")?;
        url.query_pairs_mut()
            .append_pair("balh_auth", "1")
            .append_pair("balh_auth_origin", &callback_origin);
        let url = url.to_string();
        Ok(AccessKeyLoginTicket {
            url: url.clone(),
            qr_payload: url,
            message_origin,
            callback_origin,
        })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AccessKeyLoginTicket {
    pub url: String,
    pub qr_payload: String,
    pub message_origin: String,
    pub callback_origin: String,
}

impl AccessKeyLoginTicket {
    #[must_use]
    pub fn output(&self) -> AccessKeyLoginTicketOutput {
        AccessKeyLoginTicketOutput {
            url: self.url.clone(),
            qr_payload: self.qr_payload.clone(),
            message_origin: self.message_origin.clone(),
            callback_origin: self.callback_origin.clone(),
        }
    }

    pub fn credentials_from_message(
        &self,
        message_origin: impl AsRef<str>,
        message: impl AsRef<str>,
    ) -> Result<AccessKeyLoginCredentials> {
        credentials_from_trusted_message(
            &[self.message_origin.as_str(), self.callback_origin.as_str()],
            message_origin.as_ref(),
            message.as_ref(),
        )
    }
}

impl fmt::Debug for AccessKeyLoginTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccessKeyLoginTicket")
            .field("has_url", &!self.url.is_empty())
            .field("has_qr_payload", &!self.qr_payload.is_empty())
            .field("has_message_origin", &!self.message_origin.is_empty())
            .field("has_callback_origin", &!self.callback_origin.is_empty())
            .finish()
    }
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccessKeyLoginTicketOutput {
    pub url: String,
    pub qr_payload: String,
    pub message_origin: String,
    pub callback_origin: String,
}

impl fmt::Debug for AccessKeyLoginTicketOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccessKeyLoginTicketOutput")
            .field("has_url", &!self.url.is_empty())
            .field("has_qr_payload", &!self.qr_payload.is_empty())
            .field("has_message_origin", &!self.message_origin.is_empty())
            .field("has_callback_origin", &!self.callback_origin.is_empty())
            .finish()
    }
}

impl AccessKeyLoginTicketOutput {
    pub fn credentials_from_message(
        &self,
        message_origin: impl AsRef<str>,
        message: impl AsRef<str>,
    ) -> Result<AccessKeyLoginCredentials> {
        credentials_from_trusted_message(
            &[self.message_origin.as_str(), self.callback_origin.as_str()],
            message_origin.as_ref(),
            message.as_ref(),
        )
    }
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccessKeyLoginCredentials {
    pub access_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_expires_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
}

impl AccessKeyLoginCredentials {
    pub fn from_balh_message(message: &str) -> Result<Self> {
        let payload = message
            .trim()
            .strip_prefix(BALH_LOGIN_CREDENTIALS_PREFIX)
            .ok_or_else(|| {
                Error::InvalidInput(
                    "access-key login message must start with balh-login-credentials:".to_owned(),
                )
            })?
            .trim();
        Self::from_balh_payload(payload)
    }

    pub fn from_balh_payload(payload: &str) -> Result<Self> {
        let payload = payload.trim();
        if payload.starts_with('{') {
            credentials_from_json_payload(payload)
        } else {
            credentials_from_query_payload(payload)
        }
    }

    #[must_use]
    pub fn credentials(&self) -> Credentials {
        Credentials {
            cookie: None,
            access_key: Some(self.access_key.clone()),
            tv_access_key: None,
        }
    }
}

impl fmt::Debug for AccessKeyLoginCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccessKeyLoginCredentials")
            .field("has_access_key", &!self.access_key.is_empty())
            .field(
                "has_refresh_token",
                &self
                    .refresh_token
                    .as_deref()
                    .is_some_and(|value| !value.is_empty()),
            )
            .field("oauth_expires_at", &self.oauth_expires_at)
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq)]
pub struct AccessKeyRefreshRequest {
    pub access_key: String,
    pub refresh_token: String,
    pub refresh_provider: AccessKeyRefreshProvider,
    pub refresh_keypair: Option<AccessKeyRefreshKeypair>,
}

impl AccessKeyRefreshRequest {
    pub fn new(
        access_key: impl Into<String>,
        refresh_token: impl Into<String>,
        refresh_provider: AccessKeyRefreshProvider,
    ) -> Result<Self> {
        let access_key = access_key.into();
        let refresh_token = refresh_token.into();
        if access_key.trim().is_empty() {
            return Err(Error::MissingField("access_key"));
        }
        if refresh_token.trim().is_empty() {
            return Err(Error::MissingField("refresh_token"));
        }
        Ok(Self {
            access_key,
            refresh_token,
            refresh_provider,
            refresh_keypair: None,
        })
    }

    #[must_use]
    pub fn with_refresh_keypair(mut self, refresh_keypair: AccessKeyRefreshKeypair) -> Self {
        self.refresh_keypair = Some(refresh_keypair);
        self
    }
}

impl fmt::Debug for AccessKeyRefreshRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccessKeyRefreshRequest")
            .field("has_access_key", &!self.access_key.is_empty())
            .field("has_refresh_token", &!self.refresh_token.is_empty())
            .field("refresh_provider", &self.refresh_provider)
            .field("refresh_keypair", &self.refresh_keypair)
            .finish()
    }
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq)]
pub struct WebCookieRefreshRequest {
    pub cookie: String,
    pub refresh_token: String,
}

impl WebCookieRefreshRequest {
    pub fn new(cookie: impl Into<String>, refresh_token: impl Into<String>) -> Result<Self> {
        let cookie = cookie.into();
        let refresh_token = refresh_token.into();
        if cookie.trim().is_empty() {
            return Err(Error::MissingField("cookie"));
        }
        if refresh_token.trim().is_empty() {
            return Err(Error::MissingField("refresh_token"));
        }
        Ok(Self {
            cookie,
            refresh_token,
        })
    }
}

impl fmt::Debug for WebCookieRefreshRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebCookieRefreshRequest")
            .field("has_cookie", &!self.cookie.is_empty())
            .field("has_refresh_token", &!self.refresh_token.is_empty())
            .finish()
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebCookieRefreshInfo {
    pub refresh: bool,
    pub timestamp_unix_millis: u64,
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq)]
pub struct WebCookieRefreshCredentials {
    pub cookie: String,
    pub refresh_token: String,
    pub refreshed: bool,
}

impl WebCookieRefreshCredentials {
    #[must_use]
    pub fn credentials(&self) -> Credentials {
        Credentials {
            cookie: Some(self.cookie.clone()),
            access_key: None,
            tv_access_key: None,
        }
    }
}

impl fmt::Debug for WebCookieRefreshCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebCookieRefreshCredentials")
            .field("has_cookie", &!self.cookie.is_empty())
            .field("has_refresh_token", &!self.refresh_token.is_empty())
            .field("refreshed", &self.refreshed)
            .finish()
    }
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq)]
pub struct TvAccessKeyRefreshRequest {
    pub tv_access_key: String,
    pub refresh_token: String,
}

impl TvAccessKeyRefreshRequest {
    pub fn new(tv_access_key: impl Into<String>, refresh_token: impl Into<String>) -> Result<Self> {
        let tv_access_key = tv_access_key.into();
        let refresh_token = refresh_token.into();
        if tv_access_key.trim().is_empty() {
            return Err(Error::MissingField("tv_access_key"));
        }
        if refresh_token.trim().is_empty() {
            return Err(Error::MissingField("refresh_token"));
        }
        Ok(Self {
            tv_access_key,
            refresh_token,
        })
    }
}

impl fmt::Debug for TvAccessKeyRefreshRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TvAccessKeyRefreshRequest")
            .field("has_tv_access_key", &!self.tv_access_key.is_empty())
            .field("has_refresh_token", &!self.refresh_token.is_empty())
            .finish()
    }
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq)]
pub struct TvAccessKeyLoginCredentials {
    pub tv_access_key: String,
    pub refresh_token: Option<String>,
    pub oauth_expires_at: Option<u64>,
    pub expires_in: Option<u64>,
}

impl TvAccessKeyLoginCredentials {
    #[must_use]
    pub fn credentials(&self) -> Credentials {
        Credentials {
            cookie: None,
            access_key: None,
            tv_access_key: Some(self.tv_access_key.clone()),
        }
    }
}

impl fmt::Debug for TvAccessKeyLoginCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TvAccessKeyLoginCredentials")
            .field("has_tv_access_key", &!self.tv_access_key.is_empty())
            .field(
                "has_refresh_token",
                &self
                    .refresh_token
                    .as_deref()
                    .is_some_and(|value| !value.is_empty()),
            )
            .field("oauth_expires_at", &self.oauth_expires_at)
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKeyRenewalAction {
    NoAction,
    Reauthorize,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKeyRenewalReason {
    CredentialMissing,
    LifecycleFresh,
    LifecycleUnknown,
    LifecycleStale,
    LifecycleExpiring,
    LifecycleExpired,
    Forced,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKeyAutomaticRefreshReadiness {
    Ready,
    CredentialMissing,
    UnsupportedSource,
    MissingRefreshToken,
    MetadataOnlyRefreshToken,
    MissingRefreshProvider,
    MissingRefreshKeypair,
    UnsupportedRefreshProvider,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccessKeyRenewalDecision {
    pub profile: String,
    pub present: bool,
    pub lifecycle_status: CredentialLifecycleStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<CredentialLifecycleSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key_provider: Option<AccessKeyProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_provider: Option<AccessKeyRefreshProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_keypair: Option<AccessKeyRefreshKeypair>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acquired_at_unix_millis: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at_unix_millis: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_unix_millis: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token_secret_present: Option<bool>,
    pub automatic_refresh_readiness: AccessKeyAutomaticRefreshReadiness,
    pub action: AccessKeyRenewalAction,
    pub reason: AccessKeyRenewalReason,
}

impl AccessKeyRenewalDecision {
    #[must_use]
    pub fn from_profile_status(
        status: &CredentialProfileLifecycleStatus,
        force_reauthorization: bool,
    ) -> Self {
        let credential = status
            .credential_statuses
            .iter()
            .find(|credential| credential.kind == CredentialKind::AccessKey);
        let present = credential.is_some_and(|credential| credential.present);
        let lifecycle_status = credential
            .map_or(CredentialLifecycleStatus::Missing, |credential| {
                credential.status
            });
        let reason = access_key_renewal_reason(present, lifecycle_status, force_reauthorization);
        let action = match reason {
            AccessKeyRenewalReason::LifecycleFresh => AccessKeyRenewalAction::NoAction,
            AccessKeyRenewalReason::CredentialMissing
            | AccessKeyRenewalReason::LifecycleUnknown
            | AccessKeyRenewalReason::LifecycleStale
            | AccessKeyRenewalReason::LifecycleExpiring
            | AccessKeyRenewalReason::LifecycleExpired
            | AccessKeyRenewalReason::Forced => AccessKeyRenewalAction::Reauthorize,
        };
        let automatic_refresh_readiness = credential.map_or(
            AccessKeyAutomaticRefreshReadiness::CredentialMissing,
            access_key_automatic_refresh_readiness,
        );

        Self {
            profile: status.profile.clone(),
            present,
            lifecycle_status,
            source: credential.and_then(|credential| credential.source),
            access_key_provider: credential.and_then(|credential| credential.access_key_provider),
            refresh_provider: credential.and_then(|credential| credential.refresh_provider),
            refresh_keypair: credential.and_then(|credential| credential.refresh_keypair),
            acquired_at_unix_millis: credential
                .and_then(|credential| credential.acquired_at_unix_millis),
            checked_at_unix_millis: credential
                .and_then(|credential| credential.checked_at_unix_millis),
            expires_at_unix_millis: credential
                .and_then(|credential| credential.expires_at_unix_millis),
            refresh_token_present: credential
                .and_then(|credential| credential.refresh_token_present),
            refresh_token_secret_present: credential
                .and_then(|credential| credential.refresh_token_secret_present),
            automatic_refresh_readiness,
            action,
            reason,
        }
    }

    #[must_use]
    pub fn requires_reauthorization(&self) -> bool {
        self.action == AccessKeyRenewalAction::Reauthorize
    }
}

fn access_key_renewal_reason(
    present: bool,
    lifecycle_status: CredentialLifecycleStatus,
    force_reauthorization: bool,
) -> AccessKeyRenewalReason {
    if force_reauthorization && present {
        return AccessKeyRenewalReason::Forced;
    }
    if !present {
        return AccessKeyRenewalReason::CredentialMissing;
    }
    match lifecycle_status {
        CredentialLifecycleStatus::Missing => AccessKeyRenewalReason::CredentialMissing,
        CredentialLifecycleStatus::Unknown => AccessKeyRenewalReason::LifecycleUnknown,
        CredentialLifecycleStatus::Fresh => AccessKeyRenewalReason::LifecycleFresh,
        CredentialLifecycleStatus::Stale => AccessKeyRenewalReason::LifecycleStale,
        CredentialLifecycleStatus::Expiring => AccessKeyRenewalReason::LifecycleExpiring,
        CredentialLifecycleStatus::Expired => AccessKeyRenewalReason::LifecycleExpired,
    }
}

fn access_key_automatic_refresh_readiness(
    credential: &CredentialLifecycleCredentialStatus,
) -> AccessKeyAutomaticRefreshReadiness {
    if !credential.present {
        return AccessKeyAutomaticRefreshReadiness::CredentialMissing;
    }
    if credential.source != Some(CredentialLifecycleSource::AccessKeyLogin) {
        return AccessKeyAutomaticRefreshReadiness::UnsupportedSource;
    }
    if credential.refresh_token_secret_present == Some(true) {
        return match credential.refresh_provider {
            Some(AccessKeyRefreshProvider::BilibiliMainOauth2) => {
                if credential.refresh_keypair.is_some() {
                    AccessKeyAutomaticRefreshReadiness::Ready
                } else {
                    AccessKeyAutomaticRefreshReadiness::MissingRefreshKeypair
                }
            }
            Some(AccessKeyRefreshProvider::BiliIntlOauth2) => {
                AccessKeyAutomaticRefreshReadiness::Ready
            }
            None => AccessKeyAutomaticRefreshReadiness::MissingRefreshProvider,
        };
    }
    if credential.refresh_token_present == Some(true) {
        AccessKeyAutomaticRefreshReadiness::MetadataOnlyRefreshToken
    } else {
        AccessKeyAutomaticRefreshReadiness::MissingRefreshToken
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrLoginKind {
    Web,
    Tv,
}

#[derive(Clone, Eq, PartialEq)]
pub struct QrLoginTicket {
    pub kind: QrLoginKind,
    pub url: String,
    pub key: String,
    tv_context: Option<TvLoginContext>,
}

impl QrLoginTicket {
    #[must_use]
    pub fn output(&self) -> QrLoginTicketOutput {
        QrLoginTicketOutput {
            kind: self.kind,
            url: self.url.clone(),
            qr_payload: self.url.clone(),
        }
    }
}

impl fmt::Debug for QrLoginTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QrLoginTicket")
            .field("kind", &self.kind)
            .field("has_url", &!self.url.is_empty())
            .field("has_key", &!self.key.is_empty())
            .field("has_tv_context", &self.tv_context.is_some())
            .finish()
    }
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct QrLoginTicketOutput {
    pub kind: QrLoginKind,
    pub url: String,
    pub qr_payload: String,
}

impl fmt::Debug for QrLoginTicketOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QrLoginTicketOutput")
            .field("kind", &self.kind)
            .field("has_url", &!self.url.is_empty())
            .field("has_qr_payload", &!self.qr_payload.is_empty())
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QrLoginState {
    WaitingForScan,
    WaitingForConfirm,
    Expired,
    Succeeded { credentials: Credentials },
}

impl QrLoginState {
    #[must_use]
    pub fn from_credentials_state(state: QrLoginCredentialsState) -> Self {
        match state {
            QrLoginCredentialsState::WaitingForScan => Self::WaitingForScan,
            QrLoginCredentialsState::WaitingForConfirm => Self::WaitingForConfirm,
            QrLoginCredentialsState::Expired => Self::Expired,
            QrLoginCredentialsState::Succeeded { credentials } => Self::Succeeded {
                credentials: credentials.credentials,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QrLoginCredentialsState {
    WaitingForScan,
    WaitingForConfirm,
    Expired,
    Succeeded { credentials: QrLoginCredentials },
}

#[non_exhaustive]
#[derive(Clone, Eq, PartialEq)]
pub struct QrLoginCredentials {
    pub credentials: Credentials,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

impl QrLoginCredentials {
    #[must_use]
    pub fn new(credentials: Credentials) -> Self {
        Self {
            credentials,
            refresh_token: None,
            expires_in: None,
        }
    }

    #[must_use]
    pub fn with_refresh_token(mut self, refresh_token: impl Into<String>) -> Self {
        let refresh_token = refresh_token.into();
        if !refresh_token.trim().is_empty() {
            self.refresh_token = Some(refresh_token);
        }
        self
    }

    #[must_use]
    pub fn with_expires_in(mut self, expires_in: Option<u64>) -> Self {
        self.expires_in = expires_in.filter(|value| *value > 0);
        self
    }
}

impl fmt::Debug for QrLoginCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QrLoginCredentials")
            .field("credentials", &self.credentials.redacted_summary())
            .field(
                "has_refresh_token",
                &self
                    .refresh_token
                    .as_deref()
                    .is_some_and(|value| !value.is_empty()),
            )
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

impl BiliClient {
    pub async fn refresh_access_key(
        &self,
        request: &AccessKeyRefreshRequest,
    ) -> Result<AccessKeyLoginCredentials> {
        let endpoint = access_key_refresh_endpoint_and_params(
            request,
            current_timestamp_seconds(),
            &self.config.endpoints.passport_base,
            &self.config.endpoints.intl_passport_base,
        )?;
        let url = Self::endpoint_url(endpoint.base, endpoint.path)?;
        let response = self
            .http
            .post(url)
            .headers(self.anonymous_headers()?)
            .timeout(self.config.request_timeout)
            .form(&endpoint.params)
            .send()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?
            .json::<ApiData<AccessKeyRefreshData>>()
            .await
            .map_err(BiliClient::http_error_without_url)?;
        access_key_credentials_from_refresh_data(response.into_data()?)
    }

    pub async fn web_cookie_refresh_info(&self, cookie: &str) -> Result<WebCookieRefreshInfo> {
        let csrf = csrf_from_cookie(cookie).ok_or(Error::MissingField("bili_jct"))?;
        let mut url = Self::endpoint_url(
            &self.config.endpoints.passport_base,
            "/x/passport-login/web/cookie/info",
        )?;
        url.query_pairs_mut().append_pair("csrf", &csrf);
        let response = self
            .http
            .get(url)
            .headers(self.web_cookie_refresh_headers(cookie)?)
            .timeout(self.config.request_timeout)
            .send()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?
            .json::<ApiData<WebCookieRefreshInfoData>>()
            .await
            .map_err(BiliClient::http_error_without_url)?;
        let data = response.into_data()?;
        Ok(WebCookieRefreshInfo {
            refresh: data.refresh,
            timestamp_unix_millis: data.timestamp,
        })
    }

    pub async fn refresh_web_cookie(
        &self,
        request: &WebCookieRefreshRequest,
    ) -> Result<WebCookieRefreshCredentials> {
        let info = self.web_cookie_refresh_info(&request.cookie).await?;
        if !info.refresh {
            return Ok(WebCookieRefreshCredentials {
                cookie: request.cookie.clone(),
                refresh_token: request.refresh_token.clone(),
                refreshed: false,
            });
        }
        let csrf = csrf_from_cookie(&request.cookie).ok_or(Error::MissingField("bili_jct"))?;
        let correspond_path = web_cookie_refresh_correspond_path(info.timestamp_unix_millis)?;
        let refresh_csrf = self
            .fetch_web_cookie_refresh_csrf(&request.cookie, &correspond_path)
            .await?;

        let refresh_url = Self::endpoint_url(
            &self.config.endpoints.passport_base,
            "/x/passport-login/web/cookie/refresh",
        )?;
        let refresh_form = [
            ("csrf", csrf.as_str()),
            ("refresh_csrf", refresh_csrf.as_str()),
            ("refresh_token", request.refresh_token.as_str()),
            ("source", "main_web"),
        ];
        let response = self
            .http
            .post(refresh_url)
            .headers(self.web_cookie_refresh_headers(&request.cookie)?)
            .timeout(self.config.request_timeout)
            .form(&refresh_form)
            .send()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?;
        let refreshed_cookie =
            merge_cookie_with_set_cookie_headers(&request.cookie, response.headers());
        let has_refreshed_auth_cookie =
            set_cookie_headers_contain_non_empty_cookie(response.headers(), "SESSDATA")
                && cookie_header_contains_non_empty_cookie(&refreshed_cookie, "SESSDATA");
        let response = response
            .json::<ApiData<WebCookieRefreshData>>()
            .await
            .map_err(BiliClient::http_error_without_url)?;
        let data = response.into_data()?;
        let refresh_token = non_empty_refresh_string(data.refresh_token)
            .ok_or(Error::MissingField("refresh_token"))?;
        if !has_refreshed_auth_cookie {
            return Err(Error::MissingField("SESSDATA Set-Cookie"));
        }

        let confirm_csrf = csrf_from_cookie(&refreshed_cookie).unwrap_or(csrf);
        let confirm_url = Self::endpoint_url(
            &self.config.endpoints.passport_base,
            "/x/passport-login/web/confirm/refresh",
        )?;
        let confirm_form = [
            ("csrf", confirm_csrf.as_str()),
            ("refresh_token", request.refresh_token.as_str()),
        ];
        self.http
            .post(confirm_url)
            .headers(self.web_cookie_refresh_headers(&refreshed_cookie)?)
            .timeout(self.config.request_timeout)
            .form(&confirm_form)
            .send()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?
            .json::<ApiData<serde_json::Value>>()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .ensure_success()?;

        Ok(WebCookieRefreshCredentials {
            cookie: refreshed_cookie,
            refresh_token,
            refreshed: true,
        })
    }

    pub async fn refresh_tv_access_key(
        &self,
        request: &TvAccessKeyRefreshRequest,
    ) -> Result<TvAccessKeyLoginCredentials> {
        let access_key_request = AccessKeyRefreshRequest::new(
            request.tv_access_key.clone(),
            request.refresh_token.clone(),
            AccessKeyRefreshProvider::BilibiliMainOauth2,
        )?
        .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv);
        let credentials = self.refresh_access_key(&access_key_request).await?;
        Ok(TvAccessKeyLoginCredentials {
            tv_access_key: credentials.access_key,
            refresh_token: credentials.refresh_token,
            oauth_expires_at: credentials.oauth_expires_at,
            expires_in: credentials.expires_in,
        })
    }

    async fn fetch_web_cookie_refresh_csrf(
        &self,
        cookie: &str,
        correspond_path: &str,
    ) -> Result<String> {
        let url = Self::endpoint_url(
            &self.config.endpoints.web_base,
            &format!("/correspond/1/{correspond_path}"),
        )?;
        let body = self
            .http
            .get(url)
            .headers(self.web_cookie_refresh_headers(cookie)?)
            .timeout(self.config.request_timeout)
            .send()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?
            .text()
            .await
            .map_err(BiliClient::http_error_without_url)?;
        refresh_csrf_from_correspond_body(&body).ok_or(Error::MissingField("refresh_csrf"))
    }

    fn web_cookie_refresh_headers(&self, cookie: &str) -> Result<HeaderMap> {
        let mut headers = self.anonymous_headers()?;
        headers.insert(
            COOKIE,
            HeaderValue::from_str(cookie)
                .map_err(|_| Error::InvalidInput("invalid cookie header".to_owned()))?,
        );
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        headers.insert(
            REFERER,
            HeaderValue::from_str(&self.config.endpoints.web_base)
                .unwrap_or_else(|_| HeaderValue::from_static("https://www.bilibili.com/")),
        );
        headers.insert(
            ORIGIN,
            HeaderValue::from_str(&http_origin(&self.config.endpoints.web_base, "web base")?)
                .unwrap_or_else(|_| HeaderValue::from_static("https://www.bilibili.com")),
        );
        Ok(headers)
    }

    pub async fn create_web_qr_login(&self) -> Result<QrLoginTicket> {
        let mut url = Self::endpoint_url(
            &self.config.endpoints.passport_base,
            "/x/passport-login/web/qrcode/generate",
        )?;
        url.query_pairs_mut()
            .append_pair("source", "main-fe-header");
        let response = self
            .http
            .get(url)
            .headers(self.anonymous_headers()?)
            .timeout(self.config.request_timeout)
            .send()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?
            .json::<ApiData<WebQrGenerateData>>()
            .await
            .map_err(BiliClient::http_error_without_url)?;
        let data = response.into_data()?;
        let key = data
            .qrcode_key
            .or_else(|| qrcode_key_from_url(&data.url))
            .ok_or(Error::MissingField("qrcode_key"))?;
        Ok(QrLoginTicket {
            kind: QrLoginKind::Web,
            url: data.url,
            key,
            tv_context: None,
        })
    }

    pub async fn poll_web_qr_login(&self, qrcode_key: &str) -> Result<QrLoginState> {
        self.poll_web_qr_login_credentials(qrcode_key)
            .await
            .map(QrLoginState::from_credentials_state)
    }

    pub async fn poll_web_qr_login_credentials(
        &self,
        qrcode_key: &str,
    ) -> Result<QrLoginCredentialsState> {
        let mut url = Self::endpoint_url(
            &self.config.endpoints.passport_base,
            "/x/passport-login/web/qrcode/poll",
        )?;
        url.query_pairs_mut()
            .append_pair("qrcode_key", qrcode_key)
            .append_pair("source", "main-fe-header");
        let response = self
            .http
            .get(url)
            .headers(self.anonymous_headers()?)
            .timeout(self.config.request_timeout)
            .send()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?;
        let header_cookie = cookie_from_set_cookie_headers(response.headers());
        let response = response
            .json::<ApiData<WebQrPollData>>()
            .await
            .map_err(BiliClient::http_error_without_url)?;
        let data = response.into_data()?;
        match data.code {
            WEB_QR_WAITING_SCAN => Ok(QrLoginCredentialsState::WaitingForScan),
            WEB_QR_WAITING_CONFIRM => Ok(QrLoginCredentialsState::WaitingForConfirm),
            WEB_QR_EXPIRED => Ok(QrLoginCredentialsState::Expired),
            0 => {
                let cookie = if let Some(cookie) = header_cookie {
                    cookie
                } else {
                    let url = data.url.ok_or(Error::MissingField("url"))?;
                    cookie_from_success_url(&url)?
                };
                Ok(QrLoginCredentialsState::Succeeded {
                    credentials: QrLoginCredentials::new(Credentials {
                        cookie: Some(cookie),
                        access_key: None,
                        tv_access_key: None,
                    })
                    .with_refresh_token(data.refresh_token.unwrap_or_default()),
                })
            }
            code => Err(Error::Api {
                code,
                message: data.message.unwrap_or_default(),
            }),
        }
    }

    pub async fn create_tv_qr_login(&self) -> Result<QrLoginTicket> {
        let url = Self::endpoint_url(
            &self.config.endpoints.tv_passport_base,
            "/x/passport-tv-login/qrcode/auth_code",
        )?;
        let timestamp = current_timestamp_seconds();
        let context = TvLoginContext::new(timestamp);
        let params = context.params("", timestamp);
        let response = self
            .http
            .post(url)
            .headers(self.anonymous_headers()?)
            .timeout(self.config.request_timeout)
            .form(&params)
            .send()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?
            .json::<ApiData<TvQrGenerateData>>()
            .await
            .map_err(BiliClient::http_error_without_url)?;
        let data = response.into_data()?;
        Ok(QrLoginTicket {
            kind: QrLoginKind::Tv,
            url: data.url,
            key: data.auth_code,
            tv_context: Some(context),
        })
    }

    pub async fn poll_tv_qr_login(&self, ticket: &QrLoginTicket) -> Result<QrLoginState> {
        self.poll_tv_qr_login_credentials(ticket)
            .await
            .map(QrLoginState::from_credentials_state)
    }

    pub async fn poll_tv_qr_login_credentials(
        &self,
        ticket: &QrLoginTicket,
    ) -> Result<QrLoginCredentialsState> {
        if ticket.kind != QrLoginKind::Tv {
            return Err(Error::InvalidInput(
                "poll_tv_qr_login requires a TV QR login ticket".to_owned(),
            ));
        }
        let context = ticket
            .tv_context
            .as_ref()
            .ok_or(Error::MissingField("tv login context"))?;
        let url = Self::endpoint_url(
            &self.config.endpoints.tv_passport_poll_base,
            "/x/passport-tv-login/qrcode/poll",
        )?;
        let params = context.params(&ticket.key, current_timestamp_seconds());
        let response = self
            .http
            .post(url)
            .headers(self.anonymous_headers()?)
            .timeout(self.config.request_timeout)
            .form(&params)
            .send()
            .await
            .map_err(BiliClient::http_error_without_url)?
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?
            .json::<ApiData<TvQrPollData>>()
            .await
            .map_err(BiliClient::http_error_without_url)?;
        match response.code {
            TV_QR_WAITING_SCAN => Ok(QrLoginCredentialsState::WaitingForScan),
            TV_QR_WAITING_CONFIRM => Ok(QrLoginCredentialsState::WaitingForConfirm),
            TV_QR_EXPIRED => Ok(QrLoginCredentialsState::Expired),
            0 => {
                let data = response.data.ok_or(Error::MissingField("data"))?;
                Ok(QrLoginCredentialsState::Succeeded {
                    credentials: QrLoginCredentials::new(Credentials {
                        cookie: None,
                        access_key: None,
                        tv_access_key: Some(data.access_token),
                    })
                    .with_refresh_token(data.refresh_token.unwrap_or_default())
                    .with_expires_in(data.expires_in),
                })
            }
            code => Err(Error::Api {
                code,
                message: response.message,
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiData<T> {
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<T>,
}

impl<T> ApiData<T> {
    fn into_data(self) -> Result<T> {
        if self.code != 0 {
            return Err(Error::Api {
                code: self.code,
                message: self.message,
            });
        }
        self.data.ok_or(Error::MissingField("data"))
    }

    fn ensure_success(self) -> Result<Option<T>> {
        if self.code != 0 {
            return Err(Error::Api {
                code: self.code,
                message: self.message,
            });
        }
        Ok(self.data)
    }
}

#[derive(Deserialize)]
struct WebQrGenerateData {
    url: String,
    qrcode_key: Option<String>,
}

#[derive(Deserialize)]
struct WebQrPollData {
    code: i64,
    message: Option<String>,
    url: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct TvQrGenerateData {
    url: String,
    auth_code: String,
}

#[derive(Deserialize)]
struct TvQrPollData {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct WebCookieRefreshInfoData {
    refresh: bool,
    timestamp: u64,
}

#[derive(Deserialize)]
struct WebCookieRefreshData {
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct AccessKeyRefreshData {
    token_info: Option<AccessKeyRefreshTokenInfo>,
    access_key: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    oauth_expires_at: Option<u64>,
    expires_at: Option<u64>,
    expires_in: Option<u64>,
}

#[derive(Default, Deserialize)]
struct AccessKeyRefreshTokenInfo {
    access_key: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    expires_in: Option<u64>,
}

#[derive(Clone, Eq, PartialEq)]
struct TvLoginContext {
    device_id: String,
    buvid: String,
    fingerprint: String,
}

impl TvLoginContext {
    fn new(timestamp: u64) -> Self {
        let device_id = device_token("device", timestamp, 20);
        let buvid = device_token("buvid", timestamp, 37);
        let fingerprint = format!(
            "{}{}",
            timestamp,
            device_token("fingerprint", timestamp, 45)
        );
        Self {
            device_id,
            buvid,
            fingerprint,
        }
    }

    fn params(&self, auth_code: &str, timestamp: u64) -> Vec<(&'static str, String)> {
        let mut params = vec![
            ("appkey", "4409e2ce8ffd12b8".to_owned()),
            ("auth_code", auth_code.to_owned()),
            ("bili_local_id", self.device_id.clone()),
            ("build", "102801".to_owned()),
            ("buvid", self.buvid.clone()),
            ("channel", "master".to_owned()),
            ("device", "OnePlus".to_owned()),
            ("device_id", self.device_id.clone()),
            ("device_name", "OnePlus7TPro".to_owned()),
            ("device_platform", "Android10OnePlusHD1910".to_owned()),
            ("fingerprint", self.fingerprint.clone()),
            ("guid", self.buvid.clone()),
            ("local_fingerprint", self.fingerprint.clone()),
            ("local_id", self.buvid.clone()),
            ("mobi_app", "android_tv_yst".to_owned()),
            ("networkstate", "wifi".to_owned()),
            ("platform", "android".to_owned()),
            ("sys_ver", "29".to_owned()),
            ("ts", timestamp.to_string()),
        ];
        let sign = crate::client::sign_ordered_params(&params, "59b43e04ad6965f34319062b478f83dd");
        params.push(("sign", sign));
        params
    }
}

struct AccessKeyRefreshEndpoint<'a> {
    base: &'a str,
    path: &'static str,
    params: Vec<(&'static str, String)>,
}

fn access_key_refresh_endpoint_and_params<'a>(
    request: &AccessKeyRefreshRequest,
    timestamp: u64,
    passport_base: &'a str,
    intl_passport_base: &'a str,
) -> Result<AccessKeyRefreshEndpoint<'a>> {
    match request.refresh_provider {
        AccessKeyRefreshProvider::BilibiliMainOauth2 => Ok(AccessKeyRefreshEndpoint {
            base: passport_base,
            path: main_access_key_refresh_path(request)?,
            params: main_access_key_refresh_params(request, timestamp)?,
        }),
        AccessKeyRefreshProvider::BiliIntlOauth2 => Ok(AccessKeyRefreshEndpoint {
            base: intl_passport_base,
            path: "/x/intl/passport-login/oauth2/refresh_token",
            params: intl_access_key_refresh_params(request),
        }),
    }
}

fn main_access_key_refresh_path(request: &AccessKeyRefreshRequest) -> Result<&'static str> {
    let keypair = request
        .refresh_keypair
        .ok_or(Error::MissingField("refresh_keypair"))?;
    Ok(AccessKeyRefreshKeypairSpec::new(keypair).path)
}

fn main_access_key_refresh_params(
    request: &AccessKeyRefreshRequest,
    timestamp: u64,
) -> Result<Vec<(&'static str, String)>> {
    let keypair = request
        .refresh_keypair
        .ok_or(Error::MissingField("refresh_keypair"))?;
    let spec = AccessKeyRefreshKeypairSpec::new(keypair);
    let mut params = vec![
        ("access_key", request.access_key.clone()),
        ("access_token", request.access_key.clone()),
    ];
    if spec.include_action_key {
        params.push(("actionKey", "appkey".to_owned()));
    }
    params.extend([
        ("appkey", spec.appkey.to_owned()),
        ("refresh_token", request.refresh_token.clone()),
        ("ts", timestamp.to_string()),
    ]);
    params.sort_by(|left, right| left.0.cmp(right.0));
    let sign = crate::client::sign_ordered_params(&params, spec.secret);
    params.push(("sign", sign));
    Ok(params)
}

fn intl_access_key_refresh_params(
    request: &AccessKeyRefreshRequest,
) -> Vec<(&'static str, String)> {
    vec![
        ("access_token", request.access_key.clone()),
        ("refresh_token", request.refresh_token.clone()),
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AccessKeyRefreshKeypairSpec {
    path: &'static str,
    appkey: &'static str,
    secret: &'static str,
    include_action_key: bool,
}

impl AccessKeyRefreshKeypairSpec {
    fn new(keypair: AccessKeyRefreshKeypair) -> Self {
        match keypair {
            AccessKeyRefreshKeypair::BiliTv => Self {
                path: "/x/passport-tv-login/oauth2/refresh_token",
                appkey: crate::client::TV_PLAYURL_APPKEY,
                secret: crate::client::TV_PLAYURL_APP_SECRET,
                include_action_key: true,
            },
            AccessKeyRefreshKeypair::Android => Self {
                path: "/x/passport-login/oauth2/refresh_token",
                appkey: crate::client::BILIBILI_ANDROID_APPKEY,
                secret: crate::client::BILIBILI_ANDROID_APP_SECRET,
                include_action_key: true,
            },
            AccessKeyRefreshKeypair::AndroidB => Self {
                path: "/x/passport-login/oauth2/refresh_token",
                appkey: crate::client::BILIBILI_ANDROID_B_APPKEY,
                secret: crate::client::BILIBILI_ANDROID_B_APP_SECRET,
                include_action_key: false,
            },
        }
    }
}

fn qrcode_key_from_url(raw: &str) -> Option<String> {
    url::Url::parse(raw)
        .ok()?
        .query_pairs()
        .find_map(|(key, value)| {
            (key == "qrcode_key" && !value.is_empty()).then(|| value.into_owned())
        })
}

fn normalize_http_url(raw: &str, label: &'static str) -> Result<String> {
    let mut url = parse_http_url(raw, label)?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn http_origin(raw: &str, label: &'static str) -> Result<String> {
    Ok(parse_http_url(raw, label)?.origin().ascii_serialization())
}

fn parse_http_url(raw: &str, label: &'static str) -> Result<url::Url> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput(format!("{label} must not be empty")));
    }
    let url = url::Url::parse(trimmed)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::InvalidInput(format!(
            "{label} must use http or https"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InvalidInput(format!(
            "{label} must not include userinfo"
        )));
    }
    Ok(url)
}

fn credentials_from_json_payload(payload: &str) -> Result<AccessKeyLoginCredentials> {
    let value: serde_json::Value = serde_json::from_str(payload)?;
    let object = value.as_object().ok_or_else(|| {
        Error::InvalidInput("access-key login JSON payload must be an object".to_owned())
    })?;
    build_access_key_login_credentials(
        json_string_field(object, "access_key")
            .or_else(|| json_string_field(object, "access_token")),
        json_string_field(object, "refresh_token"),
        json_u64_field_or(object, "oauth_expires_at", "expires_at")?,
        json_u64_field(object, "expires_in")?,
    )
}

fn credentials_from_query_payload(payload: &str) -> Result<AccessKeyLoginCredentials> {
    let query = access_key_callback_query(payload)?;
    let params = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
    build_access_key_login_credentials(
        query_string_field(&params, "access_key")
            .or_else(|| query_string_field(&params, "access_token")),
        query_string_field(&params, "refresh_token"),
        query_u64_field_or(&params, "oauth_expires_at", "expires_at")?,
        query_u64_field(&params, "expires_in")?,
    )
}

fn access_key_credentials_from_refresh_data(
    data: AccessKeyRefreshData,
) -> Result<AccessKeyLoginCredentials> {
    let AccessKeyRefreshData {
        token_info,
        access_key,
        access_token,
        refresh_token,
        oauth_expires_at,
        expires_at,
        expires_in,
    } = data;
    let token_info = token_info.unwrap_or_default();
    build_access_key_login_credentials(
        non_empty_refresh_string(token_info.access_key)
            .or_else(|| non_empty_refresh_string(token_info.access_token))
            .or_else(|| non_empty_refresh_string(access_key))
            .or_else(|| non_empty_refresh_string(access_token)),
        non_empty_refresh_string(token_info.refresh_token)
            .or_else(|| non_empty_refresh_string(refresh_token)),
        non_zero_refresh_u64(oauth_expires_at)
            .or_else(|| non_zero_refresh_u64(expires_at))
            .or_else(|| non_zero_refresh_u64(token_info.expires_at)),
        non_zero_refresh_u64(token_info.expires_in).or_else(|| non_zero_refresh_u64(expires_in)),
    )
}

fn non_empty_refresh_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn non_zero_refresh_u64(value: Option<u64>) -> Option<u64> {
    value.filter(|value| *value > 0)
}

fn access_key_callback_query(payload: &str) -> Result<String> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return Err(Error::MissingField("access-key callback query"));
    }
    if let Ok(url) = url::Url::parse(trimmed) {
        let query = url.query().filter(|query| !query.is_empty());
        let fragment = url
            .fragment()
            .map(str::trim)
            .filter(|fragment| !fragment.is_empty());
        return match (query, fragment) {
            (Some(query), Some(fragment)) => Ok(format!("{query}&{fragment}")),
            (Some(query), None) => Ok(query.to_owned()),
            (None, Some(fragment)) => Ok(fragment.to_owned()),
            (None, None) => Err(Error::MissingField("access-key callback query")),
        };
    }
    let (query, fragment) = match (trimmed.split_once('?'), trimmed.split_once('#')) {
        (Some((_, query)), Some((_, fragment))) => (Some(query), Some(fragment)),
        (Some((_, query)), None) => (Some(query), None),
        (None, Some((before_fragment, fragment))) if before_fragment.trim().is_empty() => {
            (None, Some(fragment))
        }
        (None, Some((before_fragment, fragment))) => (Some(before_fragment), Some(fragment)),
        (None, None) => (Some(trimmed), None),
    };
    let query = match (query, fragment) {
        (Some(query), Some(fragment)) => {
            let query = query.split('#').next().unwrap_or_default().trim();
            let fragment = fragment.trim();
            match (query.is_empty(), fragment.is_empty()) {
                (false, false) => format!("{query}&{fragment}"),
                (false, true) => query.to_owned(),
                (true, false) => fragment.to_owned(),
                (true, true) => String::new(),
            }
        }
        (Some(query), None) => query.trim().to_owned(),
        (None, Some(fragment)) => fragment.trim().to_owned(),
        (None, None) => String::new(),
    };
    let query = query.trim_start_matches('?').trim_start_matches('#').trim();
    if query.is_empty() {
        return Err(Error::MissingField("access-key callback query"));
    }
    Ok(query.to_owned())
}

fn credentials_from_trusted_message(
    expected_origins: &[&str],
    message_origin: &str,
    message: &str,
) -> Result<AccessKeyLoginCredentials> {
    let expected_origins = expected_origins
        .iter()
        .map(|origin| http_origin(origin, "access-key expected message origin"))
        .collect::<Result<Vec<_>>>()?;
    let message_origin = http_origin(message_origin, "access-key message origin")?;
    if !expected_origins.contains(&message_origin) {
        return Err(Error::InvalidInput(
            "access-key login message origin does not match ticket".to_owned(),
        ));
    }
    AccessKeyLoginCredentials::from_balh_message(message)
}

fn build_access_key_login_credentials(
    access_key: Option<String>,
    refresh_token: Option<String>,
    oauth_expires_at: Option<u64>,
    expires_in: Option<u64>,
) -> Result<AccessKeyLoginCredentials> {
    let access_key = access_key.ok_or(Error::MissingField("access_key"))?;
    Ok(AccessKeyLoginCredentials {
        access_key,
        refresh_token,
        oauth_expires_at: oauth_expires_at.and_then(normalize_expires_at),
        expires_in: expires_in.filter(|value| *value > 0),
    })
}

fn json_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn json_u64_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &'static str,
) -> Result<Option<u64>> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(number) => match number.as_u64() {
            Some(0) => Ok(None),
            Some(value) => Ok(Some(value)),
            None => Err(Error::InvalidInput(format!(
                "access-key login field {key} must be a positive integer"
            ))),
        },
        serde_json::Value::String(raw) => parse_optional_u64(raw, key),
        _ => Err(Error::InvalidInput(format!(
            "access-key login field {key} must be a positive integer"
        ))),
    }
}

fn json_u64_field_or(
    object: &serde_json::Map<String, serde_json::Value>,
    preferred_key: &'static str,
    fallback_key: &'static str,
) -> Result<Option<u64>> {
    match json_u64_field(object, preferred_key)? {
        Some(value) => Ok(Some(value)),
        None => json_u64_field(object, fallback_key),
    }
}

fn query_string_field(
    params: &[(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)],
    key: &str,
) -> Option<String> {
    params.iter().find_map(|(name, value)| {
        (name == key)
            .then(|| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn query_u64_field(
    params: &[(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)],
    key: &'static str,
) -> Result<Option<u64>> {
    let Some(value) = params
        .iter()
        .find_map(|(name, value)| (name == key).then(|| value.as_ref()))
    else {
        return Ok(None);
    };
    parse_optional_u64(value, key)
}

fn query_u64_field_or(
    params: &[(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)],
    preferred_key: &'static str,
    fallback_key: &'static str,
) -> Result<Option<u64>> {
    match query_u64_field(params, preferred_key)? {
        Some(value) => Ok(Some(value)),
        None => query_u64_field(params, fallback_key),
    }
}

fn parse_optional_u64(raw: &str, key: &'static str) -> Result<Option<u64>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let value = raw.parse::<u64>().map_err(|_| {
        Error::InvalidInput(format!(
            "access-key login field {key} must be a positive integer"
        ))
    })?;
    Ok((value > 0).then_some(value))
}

fn normalize_expires_at(value: u64) -> Option<u64> {
    (value > 0).then_some(if value < 1_000_000_000_000 {
        value * 1_000
    } else {
        value
    })
}

fn cookie_from_success_url(raw: &str) -> Result<String> {
    let url = url::Url::parse(raw)?;
    let query = url.query().ok_or(Error::MissingField("url query"))?;
    if query.is_empty() {
        return Err(Error::MissingField("url query"));
    }
    Ok(query.replace('&', ";").replace(',', "%2C"))
}

fn cookie_from_set_cookie_headers(headers: &HeaderMap) -> Option<String> {
    let pairs = headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(cookie_pair_from_set_cookie)
        .collect::<Vec<_>>();
    (!pairs.is_empty()).then(|| pairs.join(";"))
}

fn set_cookie_headers_contain_non_empty_cookie(headers: &HeaderMap, expected_name: &str) -> bool {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(cookie_pair_from_set_cookie)
        .any(|pair| {
            pair.split_once('=').is_some_and(|(name, value)| {
                name.trim() == expected_name && !value.trim().is_empty()
            })
        })
}

fn cookie_header_contains_non_empty_cookie(cookie: &str, expected_name: &str) -> bool {
    cookie_header_pairs(cookie).into_iter().any(|pair| {
        pair.split_once('=')
            .is_some_and(|(name, value)| name.trim() == expected_name && !value.trim().is_empty())
    })
}

fn merge_cookie_with_set_cookie_headers(cookie: &str, headers: &HeaderMap) -> String {
    let mut pairs = cookie_header_pairs(cookie);
    for pair in headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(cookie_pair_from_set_cookie)
    {
        let Some(name) = cookie_pair_name(&pair) else {
            continue;
        };
        if let Some(index) = pairs
            .iter()
            .position(|existing| cookie_pair_name(existing).as_deref() == Some(name.as_str()))
        {
            pairs[index] = pair;
            let mut kept_replacement = false;
            pairs.retain(|existing| {
                if cookie_pair_name(existing).as_deref() != Some(name.as_str()) {
                    return true;
                }
                if kept_replacement {
                    return false;
                }
                kept_replacement = true;
                true
            });
        } else {
            pairs.push(pair);
        }
    }
    pairs.join(";")
}

fn cookie_pair_from_set_cookie(raw: &str) -> Option<String> {
    let pair = raw.split(';').next()?.trim();
    (!pair.is_empty()).then(|| pair.replace(',', "%2C"))
}

fn cookie_header_pairs(cookie: &str) -> Vec<String> {
    cookie
        .split(';')
        .map(str::trim)
        .filter(|pair| !pair.is_empty() && pair.contains('='))
        .map(str::to_owned)
        .collect()
}

fn cookie_pair_name(pair: &str) -> Option<String> {
    pair.split_once('=')
        .map(|(name, _)| name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn csrf_from_cookie(cookie: &str) -> Option<String> {
    cookie_header_pairs(cookie).into_iter().find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        let name = name.trim();
        let value = value.trim();
        matches!(name, "bili_jct" | "csrf")
            .then(|| value)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn web_cookie_refresh_correspond_path(timestamp_unix_millis: u64) -> Result<String> {
    let public_key =
        RsaPublicKey::from_public_key_pem(WEB_COOKIE_REFRESH_PUBLIC_KEY_PEM).map_err(|error| {
            Error::InvalidInput(format!("invalid web cookie refresh public key: {error}"))
        })?;
    let encrypted = public_key
        .encrypt(
            &mut OsRng,
            Oaep::new::<Sha256>(),
            format!("refresh_{timestamp_unix_millis}").as_bytes(),
        )
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to build web cookie refresh challenge: {error}"
            ))
        })?;
    Ok(lower_hex(&encrypted))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn refresh_csrf_from_correspond_body(body: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body)
        && let Some(refresh_csrf) = json_value_string_field(&value, "refresh_csrf")
    {
        return Some(refresh_csrf);
    }
    html_text_after_marker(body, "id=\"1-name\">")
        .or_else(|| html_text_after_marker(body, "id='1-name'>"))
        .or_else(|| json_like_string_after_marker(body, "\"refresh_csrf\":\""))
        .or_else(|| json_like_string_after_marker(body, "'refresh_csrf':'"))
}

fn json_value_string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    match value {
        serde_json::Value::Object(object) => object
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                object
                    .values()
                    .find_map(|value| json_value_string_field(value, key))
            }),
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|value| json_value_string_field(value, key)),
        _ => None,
    }
}

fn html_text_after_marker(body: &str, marker: &str) -> Option<String> {
    let start = body.find(marker)? + marker.len();
    let end = body[start..].find('<')?;
    let value = body[start..start + end].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn json_like_string_after_marker(body: &str, marker: &str) -> Option<String> {
    let start = body.find(marker)? + marker.len();
    let quote = marker.chars().last()?;
    let end = body[start..].find(quote)?;
    let value = body[start..start + end].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
fn tv_login_params(auth_code: &str, timestamp: u64) -> Vec<(&'static str, String)> {
    TvLoginContext::new(timestamp).params(auth_code, timestamp)
}

fn device_token(label: &str, timestamp: u64, len: usize) -> String {
    let digest = md5::Md5::digest(format!("{label}:{timestamp}:bbdown-rust").as_bytes());
    let mut token = format!("{label}{digest:x}");
    token.retain(|character| character.is_ascii_alphanumeric());
    token.truncate(len);
    while token.len() < len {
        token.push('0');
    }
    token
}

fn current_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::{
        AccessKeyAutomaticRefreshReadiness, AccessKeyLoginConfig, AccessKeyLoginCredentials,
        AccessKeyRefreshData, AccessKeyRefreshRequest, AccessKeyRefreshTokenInfo,
        AccessKeyRenewalAction, AccessKeyRenewalDecision, AccessKeyRenewalReason,
        QrLoginCredentials, QrLoginCredentialsState, QrLoginState, QrLoginTicket,
        TvAccessKeyRefreshRequest, TvLoginContext, WebCookieRefreshRequest,
        access_key_credentials_from_refresh_data, cookie_from_set_cookie_headers,
        cookie_from_success_url, cookie_header_contains_non_empty_cookie, csrf_from_cookie,
        intl_access_key_refresh_params, main_access_key_refresh_params,
        main_access_key_refresh_path, merge_cookie_with_set_cookie_headers, qrcode_key_from_url,
        refresh_csrf_from_correspond_body, set_cookie_headers_contain_non_empty_cookie,
        tv_login_params, web_cookie_refresh_correspond_path,
    };
    use crate::{
        AccessKeyProvider, AccessKeyProviderSecret, AccessKeyRefreshKeypair,
        AccessKeyRefreshProvider, BiliClient, ClientConfig, CredentialKind,
        CredentialLifecycleMetadata, CredentialLifecyclePolicy, CredentialLifecycleSource,
        CredentialLifecycleStatus, CredentialProfileMetadata, CredentialProfileSecrets,
        CredentialProfiles, Credentials, EndpointConfig, Error, PlayurlMode, RestrictedAreaConfig,
    };
    use httpmock::MockServer;
    use httpmock::prelude::*;
    use reqwest::header::{HeaderMap, HeaderValue, SET_COOKIE};

    #[test]
    fn extracts_qrcode_key_from_login_url() {
        assert_eq!(
            qrcode_key_from_url("https://passport.example/scan?qrcode_key=abc123").as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn converts_web_success_url_query_to_cookie() -> anyhow::Result<()> {
        assert_eq!(
            cookie_from_success_url(
                "https://www.bilibili.com/?SESSDATA=abc%2Cdef&bili_jct=csrf&DedeUserID=1",
            )?,
            "SESSDATA=abc%2Cdef;bili_jct=csrf;DedeUserID=1"
        );
        Ok(())
    }

    #[test]
    fn converts_set_cookie_headers_to_cookie() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("SESSDATA=abc,def; Path=/; Domain=.bilibili.com"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("bili_jct=csrf; Path=/; Domain=.bilibili.com"),
        );

        assert_eq!(
            cookie_from_set_cookie_headers(&headers).as_deref(),
            Some("SESSDATA=abc%2Cdef;bili_jct=csrf")
        );
    }

    #[test]
    fn extracts_csrf_and_merges_refreshed_cookie_pairs() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("SESSDATA=new; Path=/; Domain=.bilibili.com"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("DedeUserID=1; Path=/; Domain=.bilibili.com"),
        );

        let merged = merge_cookie_with_set_cookie_headers("SESSDATA=old; bili_jct=csrf", &headers);

        assert_eq!(csrf_from_cookie(&merged).as_deref(), Some("csrf"));
        assert_eq!(merged, "SESSDATA=new;bili_jct=csrf;DedeUserID=1");
        assert!(cookie_header_contains_non_empty_cookie(&merged, "SESSDATA"));
    }

    #[test]
    fn refreshed_cookie_merge_deduplicates_replaced_cookie_pairs() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("SESSDATA=new; Path=/; Domain=.bilibili.com"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("SESSDATA=; Path=/; Domain=.bilibili.com"),
        );

        let merged = merge_cookie_with_set_cookie_headers(
            "SESSDATA=old;bili_jct=csrf;SESSDATA=older",
            &headers,
        );

        assert_eq!(merged, "SESSDATA=;bili_jct=csrf");
        assert!(!cookie_header_contains_non_empty_cookie(
            &merged, "SESSDATA"
        ));
    }

    #[test]
    fn detects_auth_cookie_set_cookie_headers() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("bili_jct=csrf; Path=/; Domain=.bilibili.com"),
        );
        assert!(!set_cookie_headers_contain_non_empty_cookie(
            &headers, "SESSDATA"
        ));

        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("SESSDATA=; Path=/; Domain=.bilibili.com"),
        );
        assert!(!set_cookie_headers_contain_non_empty_cookie(
            &headers, "SESSDATA"
        ));

        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("SESSDATA=new; Path=/; Domain=.bilibili.com"),
        );
        assert!(set_cookie_headers_contain_non_empty_cookie(
            &headers, "SESSDATA"
        ));
    }

    #[test]
    fn detects_non_empty_cookie_header_values() {
        assert!(cookie_header_contains_non_empty_cookie(
            "SESSDATA=new;bili_jct=csrf",
            "SESSDATA"
        ));
        assert!(!cookie_header_contains_non_empty_cookie(
            "SESSDATA=;bili_jct=csrf",
            "SESSDATA"
        ));
    }

    #[test]
    fn extracts_refresh_csrf_from_correspond_body() {
        assert_eq!(
            refresh_csrf_from_correspond_body(r#"{"data":{"refresh_csrf":"JSON_CSRF"}}"#)
                .as_deref(),
            Some("JSON_CSRF")
        );
        assert_eq!(
            refresh_csrf_from_correspond_body(r#"<html><div id="1-name">HTML_CSRF</div></html>"#)
                .as_deref(),
            Some("HTML_CSRF")
        );
    }

    #[test]
    fn builds_web_cookie_refresh_correspond_path() -> anyhow::Result<()> {
        let path = web_cookie_refresh_correspond_path(1_710_000_000_000)?;

        assert_eq!(path.len(), 256);
        assert!(path.chars().all(|character| character.is_ascii_hexdigit()));
        assert!(
            path.chars()
                .all(|character| !character.is_ascii_uppercase())
        );
        Ok(())
    }

    #[test]
    fn qr_login_ticket_debug_is_redacted() {
        let ticket = QrLoginTicket {
            kind: super::QrLoginKind::Web,
            url: "https://passport.example/scan?qrcode_key=SECRET".to_owned(),
            key: "SECRET".to_owned(),
            tv_context: None,
        };
        let debug = format!("{ticket:?}");

        assert!(debug.contains("kind: Web"));
        assert!(debug.contains("has_url: true"));
        assert!(debug.contains("has_key: true"));
        assert!(!debug.contains("SECRET"));
        assert!(!debug.contains("qrcode_key"));
    }

    #[test]
    fn qr_login_ticket_output_exposes_scan_payload_but_redacts_debug() {
        let ticket = QrLoginTicket {
            kind: super::QrLoginKind::Web,
            url: "https://passport.example/scan?qrcode_key=SECRET".to_owned(),
            key: "SECRET".to_owned(),
            tv_context: None,
        };
        let output = ticket.output();

        assert_eq!(output.kind, super::QrLoginKind::Web);
        assert_eq!(
            output.url,
            "https://passport.example/scan?qrcode_key=SECRET"
        );
        assert_eq!(output.qr_payload, output.url);
        let debug = format!("{output:?}");
        assert!(debug.contains("kind: Web"));
        assert!(debug.contains("has_url: true"));
        assert!(debug.contains("has_qr_payload: true"));
        assert!(!debug.contains("SECRET"));
        assert!(!debug.contains("qrcode_key"));
    }

    #[test]
    fn access_key_login_config_builds_biliplus_ticket() -> anyhow::Result<()> {
        let config = AccessKeyLoginConfig::biliplus("https://www.bilibili.com/video/BV1")?;
        let ticket = config.ticket()?;
        let url = url::Url::parse(&ticket.url)?;
        let query = url.query_pairs().collect::<Vec<_>>();

        assert_eq!(url.as_str(), ticket.qr_payload);
        assert_eq!(ticket.message_origin, "https://www.biliplus.com");
        assert_eq!(ticket.callback_origin, "https://www.bilibili.com");
        assert!(
            query
                .iter()
                .any(|(key, value)| key == "balh_auth" && value == "1")
        );
        assert!(
            query.iter().any(
                |(key, value)| key == "balh_auth_origin" && value == "https://www.bilibili.com"
            )
        );

        let output = ticket.output();
        assert_eq!(output.url, ticket.url);
        assert_eq!(output.qr_payload, ticket.url);
        assert_eq!(output.message_origin, ticket.message_origin);
        assert_eq!(output.callback_origin, ticket.callback_origin);
        Ok(())
    }

    #[test]
    fn access_key_login_ticket_revalidates_public_config_fields() -> anyhow::Result<()> {
        let mut config = AccessKeyLoginConfig::biliplus("https://www.bilibili.com")?;
        config.callback_origin = "https://m.bilibili.com/bangumi/play/ep1?from=unsafe".to_owned();
        let ticket = config.ticket()?;

        assert_eq!(ticket.callback_origin, "https://m.bilibili.com");
        assert!(
            ticket
                .url
                .contains("balh_auth_origin=https%3A%2F%2Fm.bilibili.com")
        );

        config.callback_origin = "javascript:alert(1)".to_owned();
        let error = config.ticket().err();
        assert!(
            matches!(error, Some(Error::InvalidInput(message)) if message.contains("http or https"))
        );
        Ok(())
    }

    #[test]
    fn access_key_login_ticket_debug_redacts_auth_url() -> anyhow::Result<()> {
        let ticket = AccessKeyLoginConfig::biliplus("https://www.bilibili.com")?.ticket()?;
        let debug = format!("{ticket:?}");
        let output_debug = format!("{:?}", ticket.output());

        assert!(debug.contains("has_url: true"));
        assert!(debug.contains("has_qr_payload: true"));
        assert!(!debug.contains("biliplus.com"));
        assert!(!debug.contains("balh_auth"));
        assert!(output_debug.contains("has_url: true"));
        assert!(!output_debug.contains("biliplus.com"));
        assert!(!output_debug.contains("balh_auth"));
        Ok(())
    }

    #[test]
    fn access_key_login_ticket_validates_message_origin() -> anyhow::Result<()> {
        let ticket = AccessKeyLoginConfig::biliplus("https://www.bilibili.com")?.ticket()?;
        let message = r#"balh-login-credentials: {"access_key":"AK"}"#;

        assert_eq!(
            ticket
                .credentials_from_message("https://www.biliplus.com/login?ignored=1", message)?
                .access_key,
            "AK"
        );
        assert_eq!(
            ticket
                .credentials_from_message("https://www.bilibili.com/video/BV1", message)?
                .access_key,
            "AK"
        );
        assert_eq!(
            ticket
                .output()
                .credentials_from_message("https://www.bilibili.com", message)?
                .access_key,
            "AK"
        );
        let error = ticket
            .credentials_from_message("https://evil.example", message)
            .err();
        assert!(
            matches!(error, Some(Error::InvalidInput(message)) if message.contains("does not match ticket"))
        );
        let output_error = ticket
            .output()
            .credentials_from_message("https://evil.example", message)
            .err();
        assert!(
            matches!(output_error, Some(Error::InvalidInput(message)) if message.contains("does not match ticket"))
        );
        Ok(())
    }

    #[test]
    fn parses_balh_json_access_key_credentials() -> anyhow::Result<()> {
        let credentials = AccessKeyLoginCredentials::from_balh_message(
            r#"balh-login-credentials: {"access_key":"AK","refresh_token":"RT","oauth_expires_at":"1710000000","expires_in":"7200"}"#,
        )?;

        assert_eq!(credentials.access_key, "AK");
        assert_eq!(credentials.refresh_token.as_deref(), Some("RT"));
        assert_eq!(credentials.oauth_expires_at, Some(1_710_000_000_000));
        assert_eq!(credentials.expires_in, Some(7_200));
        assert_eq!(
            credentials.credentials(),
            Credentials {
                cookie: None,
                access_key: Some("AK".to_owned()),
                tv_access_key: None,
            }
        );
        let debug = format!("{credentials:?}");
        assert!(debug.contains("has_access_key: true"));
        assert!(debug.contains("has_refresh_token: true"));
        assert!(!debug.contains("AK"));
        assert!(!debug.contains("RT"));
        Ok(())
    }

    #[test]
    fn access_key_renewal_decision_keeps_fresh_credentials() -> anyhow::Result<()> {
        let status = access_key_profile_status(
            Credentials::default().with_access_key("AK"),
            Some(
                CredentialLifecycleMetadata::default()
                    .with_source(CredentialLifecycleSource::AccessKeyLogin)
                    .with_acquired_at_unix_millis(1_000)
                    .with_expires_at_unix_millis(100_000_000_000)
                    .with_refresh_token_present(true),
            ),
            2_000,
        )?;
        let decision = AccessKeyRenewalDecision::from_profile_status(&status, false);

        assert!(decision.present);
        assert_eq!(decision.lifecycle_status, CredentialLifecycleStatus::Fresh);
        assert_eq!(
            decision.automatic_refresh_readiness,
            AccessKeyAutomaticRefreshReadiness::MetadataOnlyRefreshToken
        );
        assert_eq!(decision.action, AccessKeyRenewalAction::NoAction);
        assert_eq!(decision.reason, AccessKeyRenewalReason::LifecycleFresh);
        assert!(!decision.requires_reauthorization());
        Ok(())
    }

    #[test]
    fn access_key_renewal_decision_reports_ready_refresh_secret() -> anyhow::Result<()> {
        let status = access_key_profile_status_with_secrets(
            Credentials::default().with_access_key("AK"),
            Some(
                CredentialLifecycleMetadata::default()
                    .with_source(CredentialLifecycleSource::AccessKeyLogin)
                    .with_access_key_provider(AccessKeyProvider::BalhBiliplus)
                    .with_acquired_at_unix_millis(1_000)
                    .with_expires_at_unix_millis(100_000_000_000)
                    .with_refresh_token_present(true),
            ),
            Some((
                AccessKeyProvider::BalhBiliplus,
                AccessKeyProviderSecret::default()
                    .with_refresh_token("RT")
                    .with_refresh_provider(AccessKeyRefreshProvider::BilibiliMainOauth2)
                    .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv),
            )),
            1_500,
        )?;
        let decision = AccessKeyRenewalDecision::from_profile_status(&status, false);

        assert_eq!(
            decision.access_key_provider,
            Some(AccessKeyProvider::BalhBiliplus)
        );
        assert_eq!(decision.refresh_token_secret_present, Some(true));
        assert_eq!(
            decision.automatic_refresh_readiness,
            AccessKeyAutomaticRefreshReadiness::Ready
        );
        assert_eq!(decision.action, AccessKeyRenewalAction::NoAction);
        Ok(())
    }

    #[test]
    fn access_key_renewal_decision_reauthorizes_expired_metadata_only_token() -> anyhow::Result<()>
    {
        let status = access_key_profile_status(
            Credentials::default().with_access_key("AK"),
            Some(
                CredentialLifecycleMetadata::default()
                    .with_source(CredentialLifecycleSource::AccessKeyLogin)
                    .with_acquired_at_unix_millis(1_000)
                    .with_expires_at_unix_millis(2_000)
                    .with_refresh_token_present(true),
            ),
            3_000,
        )?;
        let decision = AccessKeyRenewalDecision::from_profile_status(&status, false);

        assert_eq!(
            decision.lifecycle_status,
            CredentialLifecycleStatus::Expired
        );
        assert_eq!(
            decision.automatic_refresh_readiness,
            AccessKeyAutomaticRefreshReadiness::MetadataOnlyRefreshToken
        );
        assert_eq!(decision.action, AccessKeyRenewalAction::Reauthorize);
        assert_eq!(decision.reason, AccessKeyRenewalReason::LifecycleExpired);
        assert!(decision.requires_reauthorization());
        Ok(())
    }

    #[test]
    fn access_key_renewal_decision_reauthorizes_missing_access_key() -> anyhow::Result<()> {
        let status = access_key_profile_status(Credentials::default(), None, 3_000)?;
        let decision = AccessKeyRenewalDecision::from_profile_status(&status, false);

        assert!(!decision.present);
        assert_eq!(
            decision.lifecycle_status,
            CredentialLifecycleStatus::Missing
        );
        assert_eq!(
            decision.automatic_refresh_readiness,
            AccessKeyAutomaticRefreshReadiness::CredentialMissing
        );
        assert_eq!(decision.action, AccessKeyRenewalAction::Reauthorize);
        assert_eq!(decision.reason, AccessKeyRenewalReason::CredentialMissing);
        Ok(())
    }

    fn access_key_profile_status(
        credentials: Credentials,
        access_key_metadata: Option<CredentialLifecycleMetadata>,
        now_unix_millis: u64,
    ) -> anyhow::Result<crate::CredentialProfileLifecycleStatus> {
        access_key_profile_status_with_secrets(
            credentials,
            access_key_metadata,
            None,
            now_unix_millis,
        )
    }

    fn access_key_profile_status_with_secrets(
        credentials: Credentials,
        access_key_metadata: Option<CredentialLifecycleMetadata>,
        access_key_secret: Option<(AccessKeyProvider, AccessKeyProviderSecret)>,
        now_unix_millis: u64,
    ) -> anyhow::Result<crate::CredentialProfileLifecycleStatus> {
        let mut profiles = CredentialProfiles::from_credentials(credentials);
        if let Some(access_key_metadata) = access_key_metadata {
            let mut metadata = CredentialProfileMetadata::default();
            metadata.set_credential(CredentialKind::AccessKey, access_key_metadata);
            profiles.set_profile_metadata("default", metadata)?;
        }
        if let Some((provider, secret)) = access_key_secret {
            let mut secrets = CredentialProfileSecrets::default();
            secrets.set_access_key_provider(provider, secret);
            profiles.set_profile_secrets("default", secrets)?;
        }
        Ok(profiles.profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(now_unix_millis),
        )?)
    }

    #[test]
    fn preferred_balh_expiry_fields_skip_invalid_fallbacks() -> anyhow::Result<()> {
        let json_credentials = AccessKeyLoginCredentials::from_balh_message(
            r#"balh-login-credentials: {"access_key":"AK","oauth_expires_at":"1710000000","expires_at":"not-used"}"#,
        )?;
        assert_eq!(json_credentials.oauth_expires_at, Some(1_710_000_000_000));

        let json_zero_credentials = AccessKeyLoginCredentials::from_balh_message(
            r#"balh-login-credentials: {"access_key":"AK","oauth_expires_at":0,"expires_at":1710000000}"#,
        )?;
        assert_eq!(
            json_zero_credentials.oauth_expires_at,
            Some(1_710_000_000_000)
        );

        let query_credentials = AccessKeyLoginCredentials::from_balh_payload(
            "access_key=AK&oauth_expires_at=1710000000&expires_at=not-used",
        )?;
        assert_eq!(query_credentials.oauth_expires_at, Some(1_710_000_000_000));

        let error =
            AccessKeyLoginCredentials::from_balh_payload("access_key=AK&expires_at=not-a-number")
                .err();
        assert!(
            matches!(error, Some(Error::InvalidInput(message)) if message.contains("expires_at"))
        );
        Ok(())
    }

    #[test]
    fn parses_balh_url_callback_with_access_token_alias() -> anyhow::Result<()> {
        let credentials = AccessKeyLoginCredentials::from_balh_payload(
            "https://legacy.example/callback?access_token=ALT&refresh_token=RT&expires_at=1710000000000",
        )?;

        assert_eq!(credentials.access_key, "ALT");
        assert_eq!(credentials.refresh_token.as_deref(), Some("RT"));
        assert_eq!(credentials.oauth_expires_at, Some(1_710_000_000_000));
        assert_eq!(credentials.expires_in, None);
        Ok(())
    }

    #[test]
    fn parses_balh_fragment_callback_with_access_token_alias() -> anyhow::Result<()> {
        let credentials = AccessKeyLoginCredentials::from_balh_payload(
            "https://legacy.example/callback#access_token=ALT&refresh_token=RT&expires_in=7200",
        )?;

        assert_eq!(credentials.access_key, "ALT");
        assert_eq!(credentials.refresh_token.as_deref(), Some("RT"));
        assert_eq!(credentials.oauth_expires_at, None);
        assert_eq!(credentials.expires_in, Some(7_200));

        let raw = AccessKeyLoginCredentials::from_balh_payload("#access_key=AK&expires_in=60")?;
        assert_eq!(raw.access_key, "AK");
        assert_eq!(raw.expires_in, Some(60));

        let mixed = AccessKeyLoginCredentials::from_balh_payload(
            "https://legacy.example/callback?state=csrf#access_token=MIXED&refresh_token=RT",
        )?;
        assert_eq!(mixed.access_key, "MIXED");
        assert_eq!(mixed.refresh_token.as_deref(), Some("RT"));
        Ok(())
    }

    #[test]
    fn parses_balh_raw_query_callback() -> anyhow::Result<()> {
        let credentials = AccessKeyLoginCredentials::from_balh_payload(
            "?access_key=AK&refresh_token=&expires_in=0#ignored",
        )?;

        assert_eq!(credentials.access_key, "AK");
        assert_eq!(credentials.refresh_token, None);
        assert_eq!(credentials.oauth_expires_at, None);
        assert_eq!(credentials.expires_in, None);

        let raw_without_marker = AccessKeyLoginCredentials::from_balh_payload(
            "access_key=RAW&refresh_token=RT#ignored",
        )?;
        assert_eq!(raw_without_marker.access_key, "RAW");
        assert_eq!(raw_without_marker.refresh_token.as_deref(), Some("RT"));
        Ok(())
    }

    #[test]
    fn rejects_balh_payload_without_access_key() {
        let error = AccessKeyLoginCredentials::from_balh_payload(
            "https://legacy.example/callback?refresh_token=RT",
        )
        .err();

        assert!(matches!(error, Some(Error::MissingField("access_key"))));
    }

    #[test]
    fn signs_stable_tv_login_params_after_auth_code() {
        let params = tv_login_params("AUTH", 1_700_000_000);
        assert_eq!(
            params,
            vec![
                ("appkey", "4409e2ce8ffd12b8".to_owned()),
                ("auth_code", "AUTH".to_owned()),
                ("bili_local_id", "device068a1f84f3b481".to_owned()),
                ("build", "102801".to_owned()),
                ("buvid", "buvid9bb49b85083b8fa445ee2eb127052e63".to_owned()),
                ("channel", "master".to_owned()),
                ("device", "OnePlus".to_owned()),
                ("device_id", "device068a1f84f3b481".to_owned()),
                ("device_name", "OnePlus7TPro".to_owned()),
                ("device_platform", "Android10OnePlusHD1910".to_owned()),
                (
                    "fingerprint",
                    "1700000000fingerprint2fee77e506dae703f7a1197bd676400600".to_owned()
                ),
                ("guid", "buvid9bb49b85083b8fa445ee2eb127052e63".to_owned()),
                (
                    "local_fingerprint",
                    "1700000000fingerprint2fee77e506dae703f7a1197bd676400600".to_owned()
                ),
                (
                    "local_id",
                    "buvid9bb49b85083b8fa445ee2eb127052e63".to_owned()
                ),
                ("mobi_app", "android_tv_yst".to_owned()),
                ("networkstate", "wifi".to_owned()),
                ("platform", "android".to_owned()),
                ("sys_ver", "29".to_owned()),
                ("ts", "1700000000".to_owned()),
                ("sign", "fcaa54c903154ca39a4e046b73469f74".to_owned()),
            ]
        );
    }

    #[test]
    fn builds_main_access_key_refresh_params_for_keypairs() -> anyhow::Result<()> {
        let tv_request = AccessKeyRefreshRequest::new(
            "OLD_ACCESS",
            "OLD_REFRESH",
            AccessKeyRefreshProvider::BilibiliMainOauth2,
        )?
        .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv);
        let tv_params = main_access_key_refresh_params(&tv_request, 1_700_000_000)?;

        assert_eq!(
            main_access_key_refresh_path(&tv_request)?,
            "/x/passport-tv-login/oauth2/refresh_token"
        );
        assert_eq!(param_value(&tv_params, "access_key"), Some("OLD_ACCESS"));
        assert_eq!(param_value(&tv_params, "access_token"), Some("OLD_ACCESS"));
        assert_eq!(param_value(&tv_params, "actionKey"), Some("appkey"));
        assert_eq!(param_value(&tv_params, "appkey"), Some("4409e2ce8ffd12b8"));
        assert_eq!(
            param_value(&tv_params, "refresh_token"),
            Some("OLD_REFRESH")
        );
        assert_eq!(param_value(&tv_params, "ts"), Some("1700000000"));
        assert_eq!(param_value(&tv_params, "sign").map(str::len), Some(32));

        let android_b_request = AccessKeyRefreshRequest::new(
            "OLD_ACCESS",
            "OLD_REFRESH",
            AccessKeyRefreshProvider::BilibiliMainOauth2,
        )?
        .with_refresh_keypair(AccessKeyRefreshKeypair::AndroidB);
        let android_b_params = main_access_key_refresh_params(&android_b_request, 1_700_000_000)?;

        assert_eq!(
            main_access_key_refresh_path(&android_b_request)?,
            "/x/passport-login/oauth2/refresh_token"
        );
        assert_eq!(
            param_value(&android_b_params, "access_token"),
            Some("OLD_ACCESS")
        );
        assert_eq!(
            param_value(&android_b_params, "access_key"),
            Some("OLD_ACCESS")
        );
        assert_eq!(param_value(&android_b_params, "actionKey"), None);
        assert_eq!(
            param_value(&android_b_params, "appkey"),
            Some("1d8b6e7d45233436")
        );
        assert_eq!(
            param_value(&android_b_params, "sign").map(str::len),
            Some(32)
        );
        assert_ne!(
            param_value(&tv_params, "sign"),
            param_value(&android_b_params, "sign")
        );
        Ok(())
    }

    #[test]
    fn builds_intl_access_key_refresh_params_without_app_signature() -> anyhow::Result<()> {
        let request = AccessKeyRefreshRequest::new(
            "OLD_ACCESS",
            "OLD_REFRESH",
            AccessKeyRefreshProvider::BiliIntlOauth2,
        )?;
        let params = intl_access_key_refresh_params(&request);

        assert_eq!(
            params,
            vec![
                ("access_token", "OLD_ACCESS".to_owned()),
                ("refresh_token", "OLD_REFRESH".to_owned()),
            ]
        );
        Ok(())
    }

    #[test]
    fn access_key_refresh_request_debug_redacts_tokens() -> anyhow::Result<()> {
        let request = AccessKeyRefreshRequest::new(
            "OLD_ACCESS_SECRET",
            "OLD_REFRESH_SECRET",
            AccessKeyRefreshProvider::BilibiliMainOauth2,
        )?
        .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv);
        let debug = format!("{request:?}");

        assert!(debug.contains("has_access_key: true"));
        assert!(debug.contains("has_refresh_token: true"));
        assert!(debug.contains("BiliTv"));
        assert!(!debug.contains("OLD_ACCESS_SECRET"));
        assert!(!debug.contains("OLD_REFRESH_SECRET"));
        Ok(())
    }

    #[test]
    fn refresh_credentials_ignore_zero_expiry_aliases_when_falling_back() -> anyhow::Result<()> {
        let credentials = access_key_credentials_from_refresh_data(AccessKeyRefreshData {
            token_info: Some(AccessKeyRefreshTokenInfo {
                access_key: Some("NEW_ACCESS".to_owned()),
                access_token: None,
                refresh_token: Some("NEW_REFRESH".to_owned()),
                expires_at: Some(1_710_000_000),
                expires_in: Some(0),
            }),
            access_key: None,
            access_token: None,
            refresh_token: None,
            oauth_expires_at: Some(0),
            expires_at: None,
            expires_in: Some(3_600),
        })?;

        assert_eq!(credentials.access_key, "NEW_ACCESS");
        assert_eq!(credentials.refresh_token.as_deref(), Some("NEW_REFRESH"));
        assert_eq!(credentials.oauth_expires_at, Some(1_710_000_000_000));
        assert_eq!(credentials.expires_in, Some(3_600));
        Ok(())
    }

    #[test]
    fn tv_login_context_reuses_device_identity_for_poll() {
        let context = TvLoginContext::new(1_700_000_000);
        let create_params = context.params("", 1_700_000_000);
        let poll_params = context.params("AUTH", 1_700_000_050);

        for key in [
            "bili_local_id",
            "buvid",
            "device_id",
            "fingerprint",
            "guid",
            "local_fingerprint",
            "local_id",
        ] {
            assert_eq!(
                param_value(&create_params, key),
                param_value(&poll_params, key)
            );
        }
        assert_eq!(param_value(&poll_params, "auth_code"), Some("AUTH"));
        assert_eq!(param_value(&create_params, "ts"), Some("1700000000"));
        assert_eq!(param_value(&poll_params, "ts"), Some("1700000050"));
        assert_ne!(
            param_value(&create_params, "sign"),
            param_value(&poll_params, "sign")
        );
    }

    #[tokio::test]
    async fn refreshes_access_key_with_tv_keypair_oauth_endpoint() -> anyhow::Result<()> {
        let server = MockServer::start();
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-tv-login/oauth2/refresh_token")
                .header_missing("cookie")
                .form_urlencoded_tuple("access_key", "OLD_ACCESS")
                .form_urlencoded_tuple("access_token", "OLD_ACCESS")
                .form_urlencoded_tuple("actionKey", "appkey")
                .form_urlencoded_tuple("appkey", "4409e2ce8ffd12b8")
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH")
                .form_urlencoded_tuple_exists("sign");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "token_info": {
                        "access_key": "",
                        "access_token": "NEW_ACCESS",
                        "refresh_token": "NEW_REFRESH",
                        "expires_in": 2_592_000
                    }
                }
            }));
        });
        let client = test_client(&server);
        let request = AccessKeyRefreshRequest::new(
            "OLD_ACCESS",
            "OLD_REFRESH",
            AccessKeyRefreshProvider::BilibiliMainOauth2,
        )?
        .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv);

        let credentials = client.refresh_access_key(&request).await?;

        assert_eq!(credentials.access_key, "NEW_ACCESS");
        assert_eq!(credentials.refresh_token.as_deref(), Some("NEW_REFRESH"));
        assert_eq!(credentials.expires_in, Some(2_592_000));
        refresh_mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn refreshes_android_access_key_with_main_oauth_endpoint() -> anyhow::Result<()> {
        let server = MockServer::start();
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/oauth2/refresh_token")
                .header_missing("cookie")
                .form_urlencoded_tuple("access_key", "OLD_ACCESS")
                .form_urlencoded_tuple("access_token", "OLD_ACCESS")
                .form_urlencoded_tuple("actionKey", "appkey")
                .form_urlencoded_tuple("appkey", crate::client::BILIBILI_ANDROID_APPKEY)
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH")
                .form_urlencoded_tuple_exists("sign");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "token_info": {
                        "access_key": "NEW_ANDROID_ACCESS",
                        "refresh_token": "NEW_ANDROID_REFRESH",
                        "expires_in": 3600
                    }
                }
            }));
        });
        let client = test_client(&server);
        let request = AccessKeyRefreshRequest::new(
            "OLD_ACCESS",
            "OLD_REFRESH",
            AccessKeyRefreshProvider::BilibiliMainOauth2,
        )?
        .with_refresh_keypair(AccessKeyRefreshKeypair::Android);

        let credentials = client.refresh_access_key(&request).await?;

        assert_eq!(credentials.access_key, "NEW_ANDROID_ACCESS");
        assert_eq!(
            credentials.refresh_token.as_deref(),
            Some("NEW_ANDROID_REFRESH")
        );
        assert_eq!(credentials.expires_in, Some(3_600));
        refresh_mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn refreshes_intl_access_key_with_intl_passport_endpoint() -> anyhow::Result<()> {
        let server = MockServer::start();
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/intl/passport-login/oauth2/refresh_token")
                .header_missing("cookie")
                .form_urlencoded_tuple("access_token", "OLD_INTL_ACCESS")
                .form_urlencoded_tuple("refresh_token", "OLD_INTL_REFRESH");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "token_info": {
                        "access_token": "NEW_INTL_ACCESS",
                        "refresh_token": "NEW_INTL_REFRESH",
                        "expires_in": 3600
                    }
                }
            }));
        });
        let client = test_client(&server);
        let request = AccessKeyRefreshRequest::new(
            "OLD_INTL_ACCESS",
            "OLD_INTL_REFRESH",
            AccessKeyRefreshProvider::BiliIntlOauth2,
        )?;

        let credentials = client.refresh_access_key(&request).await?;

        assert_eq!(credentials.access_key, "NEW_INTL_ACCESS");
        assert_eq!(
            credentials.refresh_token.as_deref(),
            Some("NEW_INTL_REFRESH")
        );
        assert_eq!(credentials.expires_in, Some(3_600));
        refresh_mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn refreshes_web_cookie_with_correspond_challenge() -> anyhow::Result<()> {
        let server = MockServer::start();
        let info_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/passport-login/web/cookie/info")
                .query_param("csrf", "OLD_CSRF")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"refresh": true, "timestamp": 1_710_000_000_000_u64}
            }));
        });
        let correspond_mock = server.mock(|when, then| {
            when.method(GET)
                .path_matches(r"^/correspond/1/[0-9a-f]{256}$")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200)
                .body(r#"<html><div id="1-name">REFRESH_CSRF</div></html>"#);
        });
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/web/cookie/refresh")
                .form_urlencoded_tuple("csrf", "OLD_CSRF")
                .form_urlencoded_tuple("refresh_csrf", "REFRESH_CSRF")
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH")
                .form_urlencoded_tuple("source", "main_web")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200)
                .header("Set-Cookie", "SESSDATA=new; Path=/; Domain=.bilibili.com")
                .header(
                    "Set-Cookie",
                    "bili_jct=NEW_CSRF; Path=/; Domain=.bilibili.com",
                )
                .json_body_obj(&serde_json::json!({
                    "code": 0,
                    "data": {"refresh_token": "NEW_REFRESH"}
                }));
        });
        let confirm_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/web/confirm/refresh")
                .form_urlencoded_tuple("csrf", "NEW_CSRF")
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH")
                .header("cookie", "SESSDATA=new;bili_jct=NEW_CSRF");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0
            }));
        });
        let client = test_client(&server);
        let request =
            WebCookieRefreshRequest::new("SESSDATA=old;bili_jct=OLD_CSRF", "OLD_REFRESH")?;

        let credentials = client.refresh_web_cookie(&request).await?;

        assert_eq!(credentials.cookie, "SESSDATA=new;bili_jct=NEW_CSRF");
        assert_eq!(credentials.refresh_token, "NEW_REFRESH");
        assert!(credentials.refreshed);
        info_mock.assert_calls(1);
        correspond_mock.assert_calls(1);
        refresh_mock.assert_calls(1);
        confirm_mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn web_cookie_refresh_requires_replacement_refresh_token() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/passport-login/web/cookie/info")
                .query_param("csrf", "OLD_CSRF")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"refresh": true, "timestamp": 1_710_000_000_000_u64}
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path_matches(r"^/correspond/1/[0-9a-f]{256}$")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200)
                .body(r#"<html><div id="1-name">REFRESH_CSRF</div></html>"#);
        });
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/web/cookie/refresh")
                .form_urlencoded_tuple("csrf", "OLD_CSRF")
                .form_urlencoded_tuple("refresh_csrf", "REFRESH_CSRF")
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH")
                .form_urlencoded_tuple("source", "main_web")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {}
            }));
        });
        let confirm_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/web/confirm/refresh")
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0
            }));
        });
        let client = test_client(&server);
        let request =
            WebCookieRefreshRequest::new("SESSDATA=old;bili_jct=OLD_CSRF", "OLD_REFRESH")?;

        let error = client.refresh_web_cookie(&request).await.err();

        assert!(matches!(error, Some(Error::MissingField("refresh_token"))));
        refresh_mock.assert_calls(1);
        confirm_mock.assert_calls(0);
        Ok(())
    }

    #[tokio::test]
    async fn web_cookie_refresh_requires_refreshed_auth_cookie_set_cookie() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/passport-login/web/cookie/info")
                .query_param("csrf", "OLD_CSRF")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"refresh": true, "timestamp": 1_710_000_000_000_u64}
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path_matches(r"^/correspond/1/[0-9a-f]{256}$")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200)
                .body(r#"<html><div id="1-name">REFRESH_CSRF</div></html>"#);
        });
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/web/cookie/refresh")
                .form_urlencoded_tuple("csrf", "OLD_CSRF")
                .form_urlencoded_tuple("refresh_csrf", "REFRESH_CSRF")
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH")
                .form_urlencoded_tuple("source", "main_web")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200)
                .header("Set-Cookie", "SESSDATA=; Path=/; Domain=.bilibili.com")
                .json_body_obj(&serde_json::json!({
                    "code": 0,
                    "data": {"refresh_token": "NEW_REFRESH"}
                }));
        });
        let confirm_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/web/confirm/refresh")
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0
            }));
        });
        let client = test_client(&server);
        let request =
            WebCookieRefreshRequest::new("SESSDATA=old;bili_jct=OLD_CSRF", "OLD_REFRESH")?;

        let error = client.refresh_web_cookie(&request).await.err();

        assert!(matches!(
            error,
            Some(Error::MissingField("SESSDATA Set-Cookie"))
        ));
        refresh_mock.assert_calls(1);
        confirm_mock.assert_calls(0);
        Ok(())
    }

    #[tokio::test]
    async fn web_cookie_refresh_rejects_merged_empty_auth_cookie() -> anyhow::Result<()> {
        let original_cookie = "SESSDATA=old;bili_jct=OLD_CSRF;SESSDATA=older";
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/passport-login/web/cookie/info")
                .query_param("csrf", "OLD_CSRF")
                .header("cookie", original_cookie);
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"refresh": true, "timestamp": 1_710_000_000_000_u64}
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path_matches(r"^/correspond/1/[0-9a-f]{256}$")
                .header("cookie", original_cookie);
            then.status(200)
                .body(r#"<html><div id="1-name">REFRESH_CSRF</div></html>"#);
        });
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/web/cookie/refresh")
                .form_urlencoded_tuple("csrf", "OLD_CSRF")
                .form_urlencoded_tuple("refresh_csrf", "REFRESH_CSRF")
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH")
                .form_urlencoded_tuple("source", "main_web")
                .header("cookie", original_cookie);
            then.status(200)
                .header("Set-Cookie", "SESSDATA=new; Path=/; Domain=.bilibili.com")
                .header("Set-Cookie", "SESSDATA=; Path=/; Domain=.bilibili.com")
                .json_body_obj(&serde_json::json!({
                    "code": 0,
                    "data": {"refresh_token": "NEW_REFRESH"}
                }));
        });
        let confirm_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/web/confirm/refresh")
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0
            }));
        });
        let client = test_client(&server);
        let request = WebCookieRefreshRequest::new(original_cookie, "OLD_REFRESH")?;

        let error = client.refresh_web_cookie(&request).await.err();

        assert!(matches!(
            error,
            Some(Error::MissingField("SESSDATA Set-Cookie"))
        ));
        refresh_mock.assert_calls(1);
        confirm_mock.assert_calls(0);
        Ok(())
    }

    #[tokio::test]
    async fn web_cookie_refresh_noops_when_cookie_info_is_fresh() -> anyhow::Result<()> {
        let server = MockServer::start();
        let info_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/passport-login/web/cookie/info")
                .query_param("csrf", "OLD_CSRF")
                .header("cookie", "SESSDATA=old;bili_jct=OLD_CSRF");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"refresh": false, "timestamp": 1_710_000_000_000_u64}
            }));
        });
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-login/web/cookie/refresh");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"refresh_token": "UNUSED"}
            }));
        });
        let client = test_client(&server);
        let request =
            WebCookieRefreshRequest::new("SESSDATA=old;bili_jct=OLD_CSRF", "OLD_REFRESH")?;

        let credentials = client.refresh_web_cookie(&request).await?;

        assert_eq!(credentials.cookie, "SESSDATA=old;bili_jct=OLD_CSRF");
        assert_eq!(credentials.refresh_token, "OLD_REFRESH");
        assert!(!credentials.refreshed);
        info_mock.assert_calls(1);
        refresh_mock.assert_calls(0);
        Ok(())
    }

    #[tokio::test]
    async fn refreshes_tv_access_key_with_tv_oauth_endpoint() -> anyhow::Result<()> {
        let server = MockServer::start();
        let refresh_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-tv-login/oauth2/refresh_token")
                .header_missing("cookie")
                .form_urlencoded_tuple("access_key", "OLD_TV")
                .form_urlencoded_tuple("access_token", "OLD_TV")
                .form_urlencoded_tuple("actionKey", "appkey")
                .form_urlencoded_tuple("appkey", crate::client::TV_PLAYURL_APPKEY)
                .form_urlencoded_tuple("refresh_token", "OLD_REFRESH")
                .form_urlencoded_tuple_exists("sign");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "token_info": {
                        "access_token": "NEW_TV",
                        "refresh_token": "NEW_REFRESH",
                        "expires_in": 7200
                    }
                }
            }));
        });
        let client = test_client(&server);
        let request = TvAccessKeyRefreshRequest::new("OLD_TV", "OLD_REFRESH")?;

        let credentials = client.refresh_tv_access_key(&request).await?;

        assert_eq!(credentials.tv_access_key, "NEW_TV");
        assert_eq!(credentials.refresh_token.as_deref(), Some("NEW_REFRESH"));
        assert_eq!(credentials.expires_in, Some(7_200));
        refresh_mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn polls_web_qr_login_states() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/passport-login/web/qrcode/poll")
                .query_param("qrcode_key", "WAIT")
                .header_missing("cookie");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"code": 86101}
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/passport-login/web/qrcode/poll")
                .query_param("qrcode_key", "CONFIRM")
                .header_missing("cookie");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"code": 86090}
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/passport-login/web/qrcode/poll")
                .query_param("qrcode_key", "DONE")
                .header_missing("cookie");
            then.status(200)
                .header("Set-Cookie", "SESSDATA=sess; Path=/; Domain=.bilibili.com")
                .json_body_obj(&serde_json::json!({
                    "code": 0,
                    "data": {
                        "code": 0,
                        "refresh_token": "WEB_RT",
                        "url": "https://passport.biligame.com/crossDomain?source=main_web&go_url=https%3A%2F%2Fpassport.bilibili.com"
                    }
                }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/passport-login/web/qrcode/poll")
                .query_param("qrcode_key", "EXPIRED")
                .header_missing("cookie");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "code": 86038,
                    "message": "expired"
                }
            }));
        });
        let client = test_client(&server);

        assert_eq!(
            client.poll_web_qr_login("WAIT").await?,
            QrLoginState::WaitingForScan
        );
        assert_eq!(
            client.poll_web_qr_login("CONFIRM").await?,
            QrLoginState::WaitingForConfirm
        );
        assert_eq!(
            client.poll_web_qr_login("DONE").await?,
            QrLoginState::Succeeded {
                credentials: Credentials {
                    cookie: Some("SESSDATA=sess".to_owned()),
                    access_key: None,
                    tv_access_key: None,
                }
            }
        );
        assert_eq!(
            client.poll_web_qr_login_credentials("DONE").await?,
            QrLoginCredentialsState::Succeeded {
                credentials: QrLoginCredentials::new(Credentials {
                    cookie: Some("SESSDATA=sess".to_owned()),
                    access_key: None,
                    tv_access_key: None,
                })
                .with_refresh_token("WEB_RT")
            }
        );
        assert_eq!(
            client.poll_web_qr_login("EXPIRED").await?,
            QrLoginState::Expired
        );
        Ok(())
    }

    #[tokio::test]
    async fn creates_and_polls_tv_qr_login() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-tv-login/qrcode/auth_code")
                .header_missing("cookie")
                .form_urlencoded_tuple("appkey", "4409e2ce8ffd12b8")
                .form_urlencoded_tuple("auth_code", "")
                .form_urlencoded_tuple("mobi_app", "android_tv_yst")
                .form_urlencoded_tuple_exists("sign");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"url": "https://tv.example/scan", "auth_code": "AUTH"}
            }));
        });
        mock_tv_qr_poll(
            &server,
            "WAIT",
            &serde_json::json!({"code": 86039, "message": "waiting scan"}),
        );
        mock_tv_qr_poll(
            &server,
            "CONFIRM",
            &serde_json::json!({"code": 86090, "message": "waiting confirm"}),
        );
        mock_tv_qr_poll(
            &server,
            "EXPIRED",
            &serde_json::json!({"code": 86038, "message": "expired"}),
        );
        mock_tv_qr_poll(
            &server,
            "AUTH",
            &serde_json::json!({
                "code": 0,
                "data": {
                    "access_token": "ACCESS",
                    "refresh_token": "TV_RT",
                    "expires_in": 7200
                }
            }),
        );
        let client = test_client(&server);
        let ticket = client.create_tv_qr_login().await?;

        assert_eq!(ticket.key, "AUTH");
        let mut wait_ticket = ticket.clone();
        wait_ticket.key = "WAIT".to_owned();
        assert_eq!(
            client.poll_tv_qr_login(&wait_ticket).await?,
            QrLoginState::WaitingForScan
        );
        let mut confirm_ticket = ticket.clone();
        confirm_ticket.key = "CONFIRM".to_owned();
        assert_eq!(
            client.poll_tv_qr_login(&confirm_ticket).await?,
            QrLoginState::WaitingForConfirm
        );
        let mut expired_ticket = ticket.clone();
        expired_ticket.key = "EXPIRED".to_owned();
        assert_eq!(
            client.poll_tv_qr_login(&expired_ticket).await?,
            QrLoginState::Expired
        );
        assert_eq!(
            client.poll_tv_qr_login(&ticket).await?,
            QrLoginState::Succeeded {
                credentials: Credentials {
                    cookie: None,
                    access_key: None,
                    tv_access_key: Some("ACCESS".to_owned()),
                }
            }
        );
        assert_eq!(
            client.poll_tv_qr_login_credentials(&ticket).await?,
            QrLoginCredentialsState::Succeeded {
                credentials: QrLoginCredentials::new(Credentials {
                    cookie: None,
                    access_key: None,
                    tv_access_key: Some("ACCESS".to_owned()),
                })
                .with_refresh_token("TV_RT")
                .with_expires_in(Some(7200))
            }
        );
        Ok(())
    }

    fn mock_tv_qr_poll(server: &MockServer, auth_code: &str, body: &serde_json::Value) {
        server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-tv-login/qrcode/poll")
                .header_missing("cookie")
                .form_urlencoded_tuple("auth_code", auth_code)
                .form_urlencoded_tuple_exists("sign");
            then.status(200).json_body_obj(&body);
        });
    }

    fn test_client(server: &MockServer) -> BiliClient {
        BiliClient::new(ClientConfig {
            endpoints: EndpointConfig {
                api_base: server.base_url(),
                pgc_base: server.base_url(),
                web_base: server.base_url(),
                intl_base: server.base_url(),
                intl_passport_base: server.base_url(),
                comment_base: server.base_url(),
                passport_base: server.base_url(),
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            access_key_provider: None,
            credentials: Credentials {
                cookie: Some("SESSDATA=old".to_owned()),
                access_key: None,
                tv_access_key: None,
            },
            restricted_area: RestrictedAreaConfig::default(),
            playurl_mode: PlayurlMode::Web,
            user_agent: "test".to_owned(),
            request_timeout: std::time::Duration::from_secs(30),
        })
    }

    fn param_value<'a>(params: &'a [(&str, String)], key: &str) -> Option<&'a str> {
        params
            .iter()
            .find_map(|(candidate, value)| (*candidate == key).then_some(value.as_str()))
    }
}

use crate::{BiliClient, Credentials, Error, Result};
use md5::Digest;
use reqwest::header::{HeaderMap, SET_COOKIE};
use serde::{Deserialize, Serialize};
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
            &self.message_origin,
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
            &self.message_origin,
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

impl BiliClient {
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
            WEB_QR_WAITING_SCAN => Ok(QrLoginState::WaitingForScan),
            WEB_QR_WAITING_CONFIRM => Ok(QrLoginState::WaitingForConfirm),
            WEB_QR_EXPIRED => Ok(QrLoginState::Expired),
            0 => {
                let cookie = if let Some(cookie) = header_cookie {
                    cookie
                } else {
                    let url = data.url.ok_or(Error::MissingField("url"))?;
                    cookie_from_success_url(&url)?
                };
                Ok(QrLoginState::Succeeded {
                    credentials: Credentials {
                        cookie: Some(cookie),
                        access_key: None,
                        tv_access_key: None,
                    },
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
            TV_QR_WAITING_SCAN => Ok(QrLoginState::WaitingForScan),
            TV_QR_WAITING_CONFIRM => Ok(QrLoginState::WaitingForConfirm),
            TV_QR_EXPIRED => Ok(QrLoginState::Expired),
            0 => {
                let data = response.data.ok_or(Error::MissingField("data"))?;
                Ok(QrLoginState::Succeeded {
                    credentials: Credentials {
                        cookie: None,
                        access_key: None,
                        tv_access_key: Some(data.access_token),
                    },
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
}

#[derive(Debug, Deserialize)]
struct WebQrGenerateData {
    url: String,
    qrcode_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebQrPollData {
    code: i64,
    message: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TvQrGenerateData {
    url: String,
    auth_code: String,
}

#[derive(Debug, Deserialize)]
struct TvQrPollData {
    access_token: String,
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
        json_u64_field(object, "oauth_expires_at")?.or(json_u64_field(object, "expires_at")?),
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
        query_u64_field(&params, "oauth_expires_at")?.or(query_u64_field(&params, "expires_at")?),
        query_u64_field(&params, "expires_in")?,
    )
}

fn access_key_callback_query(payload: &str) -> Result<String> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return Err(Error::MissingField("access-key callback query"));
    }
    if let Ok(url) = url::Url::parse(trimmed) {
        return url
            .query()
            .filter(|query| !query.is_empty())
            .map(str::to_owned)
            .ok_or(Error::MissingField("access-key callback query"));
    }
    let query = trimmed
        .split_once('?')
        .map_or(trimmed, |(_, query)| query)
        .split('#')
        .next()
        .unwrap_or_default()
        .trim_start_matches('?')
        .trim();
    if query.is_empty() {
        return Err(Error::MissingField("access-key callback query"));
    }
    Ok(query.to_owned())
}

fn credentials_from_trusted_message(
    expected_origin: &str,
    message_origin: &str,
    message: &str,
) -> Result<AccessKeyLoginCredentials> {
    let expected_origin = http_origin(expected_origin, "access-key expected message origin")?;
    let message_origin = http_origin(message_origin, "access-key message origin")?;
    if message_origin != expected_origin {
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
        serde_json::Value::Number(number) => number.as_u64().map(Some).ok_or_else(|| {
            Error::InvalidInput(format!(
                "access-key login field {key} must be a positive integer"
            ))
        }),
        serde_json::Value::String(raw) => parse_optional_u64(raw, key),
        _ => Err(Error::InvalidInput(format!(
            "access-key login field {key} must be a positive integer"
        ))),
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

fn cookie_pair_from_set_cookie(raw: &str) -> Option<String> {
    let pair = raw.split(';').next()?.trim();
    (!pair.is_empty()).then(|| pair.replace(',', "%2C"))
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
        AccessKeyLoginConfig, AccessKeyLoginCredentials, QrLoginState, QrLoginTicket,
        TvLoginContext, cookie_from_set_cookie_headers, cookie_from_success_url,
        qrcode_key_from_url, tv_login_params,
    };
    use crate::{
        BiliClient, ClientConfig, Credentials, EndpointConfig, Error, PlayurlMode,
        RestrictedAreaConfig,
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
                .output()
                .credentials_from_message("https://www.biliplus.com", message)?
                .access_key,
            "AK"
        );
        let error = ticket
            .credentials_from_message("https://evil.example", message)
            .err();
        assert!(
            matches!(error, Some(Error::InvalidInput(message)) if message.contains("does not match ticket"))
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
    fn parses_balh_raw_query_callback() -> anyhow::Result<()> {
        let credentials = AccessKeyLoginCredentials::from_balh_payload(
            "?access_key=AK&refresh_token=&expires_in=0#ignored",
        )?;

        assert_eq!(credentials.access_key, "AK");
        assert_eq!(credentials.refresh_token, None);
        assert_eq!(credentials.oauth_expires_at, None);
        assert_eq!(credentials.expires_in, None);
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
        server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-tv-login/qrcode/poll")
                .header_missing("cookie")
                .form_urlencoded_tuple("auth_code", "WAIT")
                .form_urlencoded_tuple_exists("sign");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 86039,
                "message": "waiting scan"
            }));
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-tv-login/qrcode/poll")
                .header_missing("cookie")
                .form_urlencoded_tuple("auth_code", "CONFIRM")
                .form_urlencoded_tuple_exists("sign");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 86090,
                "message": "waiting confirm"
            }));
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-tv-login/qrcode/poll")
                .header_missing("cookie")
                .form_urlencoded_tuple("auth_code", "EXPIRED")
                .form_urlencoded_tuple_exists("sign");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 86038,
                "message": "expired"
            }));
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/x/passport-tv-login/qrcode/poll")
                .header_missing("cookie")
                .form_urlencoded_tuple("auth_code", "AUTH")
                .form_urlencoded_tuple_exists("sign");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"access_token": "ACCESS"}
            }));
        });
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
        Ok(())
    }

    fn test_client(server: &MockServer) -> BiliClient {
        BiliClient::new(ClientConfig {
            endpoints: EndpointConfig {
                api_base: server.base_url(),
                pgc_base: server.base_url(),
                intl_base: server.base_url(),
                comment_base: server.base_url(),
                passport_base: server.base_url(),
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
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

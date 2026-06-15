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
        QrLoginState, QrLoginTicket, TvLoginContext, cookie_from_set_cookie_headers,
        cookie_from_success_url, qrcode_key_from_url, tv_login_params,
    };
    use crate::{
        BiliClient, ClientConfig, Credentials, EndpointConfig, PlayurlMode, RestrictedAreaConfig,
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

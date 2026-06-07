use crate::{BiliClient, Credentials, Error, Result};
use md5::Digest;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QrLoginTicket {
    pub kind: QrLoginKind,
    pub url: String,
    pub key: String,
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
            .map_err(BiliClient::http_error_without_url)?
            .json::<ApiData<WebQrPollData>>()
            .await
            .map_err(BiliClient::http_error_without_url)?;
        let data = response.into_data()?;
        match data.code {
            WEB_QR_WAITING_SCAN => Ok(QrLoginState::WaitingForScan),
            WEB_QR_WAITING_CONFIRM => Ok(QrLoginState::WaitingForConfirm),
            WEB_QR_EXPIRED => Ok(QrLoginState::Expired),
            0 => {
                let url = data.url.ok_or(Error::MissingField("url"))?;
                Ok(QrLoginState::Succeeded {
                    credentials: Credentials {
                        cookie: Some(cookie_from_success_url(&url)?),
                        access_key: None,
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
        let params = tv_login_params("", current_timestamp_seconds());
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
        })
    }

    pub async fn poll_tv_qr_login(&self, auth_code: &str) -> Result<QrLoginState> {
        let url = Self::endpoint_url(
            &self.config.endpoints.tv_passport_poll_base,
            "/x/passport-tv-login/qrcode/poll",
        )?;
        let params = tv_login_params(auth_code, current_timestamp_seconds());
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
                        access_key: Some(data.access_token),
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

fn tv_login_params(auth_code: &str, timestamp: u64) -> Vec<(&'static str, String)> {
    let device_id = device_token("device", timestamp, 20);
    let buvid = device_token("buvid", timestamp, 37);
    let fingerprint = format!(
        "{}{}",
        timestamp,
        device_token("fingerprint", timestamp, 45)
    );
    let mut params = vec![
        ("appkey", "4409e2ce8ffd12b8".to_owned()),
        ("auth_code", auth_code.to_owned()),
        ("bili_local_id", device_id.clone()),
        ("build", "102801".to_owned()),
        ("buvid", buvid.clone()),
        ("channel", "master".to_owned()),
        ("device", "OnePlus".to_owned()),
        ("device_id", device_id),
        ("device_name", "OnePlus7TPro".to_owned()),
        ("device_platform", "Android10OnePlusHD1910".to_owned()),
        ("fingerprint", fingerprint.clone()),
        ("guid", buvid.clone()),
        ("local_fingerprint", fingerprint),
        ("local_id", buvid),
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
    use super::{QrLoginState, cookie_from_success_url, qrcode_key_from_url, tv_login_params};
    use crate::{BiliClient, ClientConfig, Credentials, EndpointConfig};
    use httpmock::MockServer;
    use httpmock::prelude::*;

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
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "code": 0,
                    "url": "https://www.bilibili.com/?SESSDATA=sess&bili_jct=csrf"
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
                    cookie: Some("SESSDATA=sess;bili_jct=csrf".to_owned()),
                    access_key: None,
                }
            }
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
        assert_eq!(
            client.poll_tv_qr_login("CONFIRM").await?,
            QrLoginState::WaitingForConfirm
        );
        assert_eq!(
            client.poll_tv_qr_login(&ticket.key).await?,
            QrLoginState::Succeeded {
                credentials: Credentials {
                    cookie: None,
                    access_key: Some("ACCESS".to_owned()),
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
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            credentials: Credentials {
                cookie: Some("SESSDATA=old".to_owned()),
                access_key: None,
            },
            user_agent: "test".to_owned(),
            request_timeout: std::time::Duration::from_secs(30),
        })
    }
}

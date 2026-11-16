use super::BiliClient;
use crate::{Error, Result};
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub const DEFAULT_CHANNEL_MENU_MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const CHANNEL_MENU_ENDPOINT: &str = "/x/kv-frontend/namespace/data";
const CHANNEL_MENU_APP_KEY: &str = "333.1339";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelMenuOptions {
    /// Bounds decoded response bytes and cumulative selected record/key bytes.
    /// This data-size bound is not an exact heap-memory limit.
    pub max_response_bytes: usize,
}

impl ChannelMenuOptions {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_response_bytes: DEFAULT_CHANNEL_MENU_MAX_RESPONSE_BYTES,
        }
    }

    #[must_use]
    pub const fn with_max_response_bytes(mut self, max_response_bytes: usize) -> Self {
        self.max_response_bytes = max_response_bytes;
        self
    }

    fn validate(self) -> Result<()> {
        if self.max_response_bytes == 0 {
            return Err(Error::InvalidInput(
                "channel menu response byte limit must be greater than zero".to_owned(),
            ));
        }
        Ok(())
    }
}

impl Default for ChannelMenuOptions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelMenu {
    pub scope: String,
    pub version_id: Option<String>,
    pub app_version_id: Option<String>,
    pub entries: Vec<ChannelMenuEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelMenuEntry {
    pub key: Option<String>,
    pub name: String,
    pub url: String,
    pub route: String,
    pub channel_id: Option<u32>,
    pub sub_channel_id: Option<u32>,
    pub tid: Option<u32>,
    pub season_type: Option<u32>,
    pub enable_sub: Option<u32>,
    pub children: Vec<ChannelMenuEntry>,
}

#[derive(Debug, Deserialize)]
struct ChannelMenuApiEnvelope {
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelMenuEnvelopeData {
    version_id: Option<String>,
    app_version_id: Option<String>,
    data: Option<HashMap<String, Value>>,
}

#[derive(Debug, Deserialize)]
struct RawChannelMenuEntry {
    name: String,
    url: String,
    route: String,
    #[serde(rename = "channelId")]
    channel_id: Option<u32>,
    #[serde(rename = "subChannelId")]
    sub_channel_id: Option<u32>,
    tid: Option<u32>,
    #[serde(rename = "seasonType")]
    season_type: Option<u32>,
    config: Option<RawChannelMenuConfig>,
    sub: Option<Vec<RawChannelMenuEntry>>,
}

#[derive(Debug, Deserialize)]
struct RawChannelMenuConfig {
    #[serde(rename = "enableSub")]
    enable_sub: Option<u32>,
}

impl RawChannelMenuEntry {
    fn into_entry(self, key: Option<String>) -> Result<ChannelMenuEntry> {
        if self.name.trim().is_empty() {
            return Err(invalid_menu("entry name must not be blank"));
        }
        if self.route.trim().is_empty() {
            return Err(invalid_menu("entry route must not be blank"));
        }
        let children = self
            .sub
            .unwrap_or_default()
            .into_iter()
            .map(|child| child.into_entry(None))
            .collect::<Result<Vec<_>>>()?;
        Ok(ChannelMenuEntry {
            key,
            name: self.name,
            url: normalize_menu_url(self.url),
            route: self.route,
            channel_id: self.channel_id,
            sub_channel_id: self.sub_channel_id,
            tid: self.tid,
            season_type: self.season_type,
            enable_sub: self.config.and_then(|config| config.enable_sub),
            children,
        })
    }
}

pub(super) async fn channel_menu(
    client: &BiliClient,
    options: &ChannelMenuOptions,
) -> Result<ChannelMenu> {
    options.validate()?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(BiliClient::http_error_without_url)?;
    let mut url =
        BiliClient::endpoint_url(&client.config.endpoints.api_base, CHANNEL_MENU_ENDPOINT)?;
    url.query_pairs_mut()
        .append_pair("appKey", CHANNEL_MENU_APP_KEY)
        .append_pair("nscode", "10");
    let mut response = http
        .get(url)
        .headers(client.anonymous_headers()?)
        .header(ACCEPT, "application/json")
        .timeout(client.config.request_timeout)
        .send()
        .await
        .map_err(BiliClient::http_error_without_url)?;
    if response.status().is_redirection() {
        return Err(invalid_menu(
            "API endpoint redirected; redirect was not followed",
        ));
    }
    response = response
        .error_for_status()
        .map_err(BiliClient::http_error_without_url)?;
    if response
        .content_length()
        .is_some_and(|length| length > options.max_response_bytes as u64)
    {
        return Err(response_too_large(options.max_response_bytes));
    }
    let body = read_bounded_body(&mut response, options.max_response_bytes).await?;
    let api: ChannelMenuApiEnvelope = serde_json::from_slice(&body)?;
    decode_channel_menu(api, options.max_response_bytes)
}

fn decode_channel_menu(
    api: ChannelMenuApiEnvelope,
    max_response_bytes: usize,
) -> Result<ChannelMenu> {
    if api.code != 0 {
        return Err(Error::Api {
            code: api.code,
            message: api.message,
        });
    }
    let raw_data = api.data.ok_or(Error::MissingField("data"))?;
    let data: ChannelMenuEnvelopeData = serde_json::from_value(raw_data)?;
    let namespace = data.data.ok_or(Error::MissingField("data.data"))?;
    let sort_json = namespace
        .get("channel_list.sort")
        .and_then(Value::as_str)
        .ok_or(Error::MissingField("data.data.channel_list.sort"))?;
    let keys: Vec<String> = serde_json::from_str(sort_json)?;
    let mut entries = Vec::new();
    let mut selected_bytes = 0_usize;
    for key in keys {
        if key.trim().is_empty() {
            return Err(invalid_menu("channel_list.sort contains a blank key"));
        }
        let field = format!("channel_list.{key}");
        let raw = namespace
            .get(&field)
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_menu("channel_list.sort references a missing record"))?;
        selected_bytes = selected_bytes
            .checked_add(key.len())
            .and_then(|total| total.checked_add(raw.len()))
            .ok_or_else(|| expanded_records_too_large(max_response_bytes))?;
        if selected_bytes > max_response_bytes {
            return Err(expanded_records_too_large(max_response_bytes));
        }
        let entry: RawChannelMenuEntry = serde_json::from_str(raw)?;
        entries.push(entry.into_entry(Some(key))?);
    }
    Ok(ChannelMenu {
        scope: "remote_channel_menu_config".to_owned(),
        version_id: data.version_id,
        app_version_id: data.app_version_id,
        entries,
    })
}

fn normalize_menu_url(url: String) -> String {
    if let Some(remainder) = url.strip_prefix("//") {
        format!("https://{remainder}")
    } else {
        url
    }
}

async fn read_bounded_body(
    response: &mut reqwest::Response,
    max_response_bytes: usize,
) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(BiliClient::http_error_without_url)?
    {
        if body.len().saturating_add(chunk.len()) > max_response_bytes {
            return Err(response_too_large(max_response_bytes));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn response_too_large(max_response_bytes: usize) -> Error {
    invalid_menu(format!(
        "response exceeded the {max_response_bytes}-byte limit"
    ))
}

fn expanded_records_too_large(max_response_bytes: usize) -> Error {
    invalid_menu(format!(
        "cumulative selected record and root key bytes exceeded the {max_response_bytes}-byte limit"
    ))
}

fn invalid_menu(message: impl Into<String>) -> Error {
    Error::InvalidInput(format!("channel menu {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::{ChannelMenuOptions, DEFAULT_CHANNEL_MENU_MAX_RESPONSE_BYTES, channel_menu};
    use crate::{BiliClient, ClientConfig, Credentials, EndpointConfig, Error};
    use httpmock::prelude::*;
    use serde_json::{Value, json};
    use std::time::Duration;

    const COOKIE_FIXTURE: &str = "codex_synth_v1_access_a";
    const ACCESS_KEY_FIXTURE: &str = "codex_synth_v1_access_b";

    fn client(server: &MockServer) -> BiliClient {
        BiliClient::new(
            ClientConfig::default()
                .with_endpoints(EndpointConfig::default().with_api_base(server.base_url())),
        )
    }

    fn client_with_credentials(server: &MockServer) -> BiliClient {
        BiliClient::new(
            ClientConfig::default()
                .with_endpoints(EndpointConfig::default().with_api_base(server.base_url()))
                .with_user_agent("channel-menu-test/1.0")
                .with_credentials(Credentials {
                    cookie: Some(COOKIE_FIXTURE.to_owned()),
                    access_key: Some(ACCESS_KEY_FIXTURE.to_owned()),
                    ..Credentials::default()
                }),
        )
    }

    fn response(namespace: &Value) -> Value {
        json!({"code": 0, "message": "ok", "data": {
            "versionId": "snapshot-opaque-id", "appVersionId": "app-version-opaque",
            "data": namespace
        }})
    }

    fn raw_record(record: &Value) -> String {
        record.to_string()
    }

    fn repeated_record_response(repetitions: usize) -> (Value, usize, usize) {
        let key = "repeated";
        let record = json!({"name": "n".repeat(64), "url": "", "route": "route"});
        let encoded_record = raw_record(&record);
        let sort = json!(vec![key; repetitions]).to_string();
        let namespace = json!({
            "channel_list.sort": sort,
            "channel_list.repeated": encoded_record
        });
        let response_body = response(&namespace);
        let selected_bytes = repetitions * (key.len() + encoded_record.len());
        let response_bytes = response_body.to_string().len();
        (response_body, selected_bytes, response_bytes)
    }

    #[tokio::test]
    async fn requests_exact_anonymous_namespace_and_preserves_ordered_metadata()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let root = json!({
            "name": "Root", "url": "//www.bilibili.com/c/example/?x=1#frag",
            "route": "root-route", "channelId": 0, "tid": 0,
            "seasonType": 2, "config": {"enableSub": 0},
            "sub": [{"name": "Child", "url": "https://love.bilibili.com/a?q=2#child",
                "route": "child-route", "subChannelId": 0, "tid": 17}]
        });
        let first = json!({"name": "Blank URL", "url": "", "route": "blank-route",
            "channelId": 9, "tid": null, "sub": null});
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/kv-frontend/namespace/data")
                .query_param("appKey", "333.1339")
                .query_param("nscode", "10")
                .query_param_count(".*", ".*", 2)
                .query_param_missing("versionId")
                .header("user-agent", "channel-menu-test/1.0")
                .header("referer", "https://www.bilibili.com/")
                .header_missing("cookie")
                .header_missing("authorization");
            then.status(200).json_body(response(&json!({
                "channel_list.sort": r#"["root","blank","root"]"#,
                "channel_list.root": raw_record(&root),
                "channel_list.blank": raw_record(&first),
                "unselected_key": {"deliberately": "ignored"},
                "side_channel_list.sort": 45
            })));
        });

        let menu = channel_menu(
            &client_with_credentials(&server),
            &ChannelMenuOptions::default(),
        )
        .await?;
        assert_eq!(menu.scope, "remote_channel_menu_config");
        assert_eq!(menu.version_id.as_deref(), Some("snapshot-opaque-id"));
        assert_eq!(menu.app_version_id.as_deref(), Some("app-version-opaque"));
        assert_eq!(menu.entries.len(), 3);
        assert_eq!(menu.entries[0].key.as_deref(), Some("root"));
        assert_eq!(menu.entries[1].key.as_deref(), Some("blank"));
        assert_eq!(menu.entries[2], menu.entries[0]);
        assert_eq!(
            menu.entries[0].url,
            "https://www.bilibili.com/c/example/?x=1#frag"
        );
        assert_eq!(menu.entries[0].channel_id, Some(0));
        assert_eq!(menu.entries[0].tid, Some(0));
        assert_eq!(menu.entries[0].enable_sub, Some(0));
        assert_eq!(menu.entries[0].children.len(), 1);
        assert_eq!(menu.entries[0].children[0].key, None);
        assert_eq!(menu.entries[0].children[0].sub_channel_id, Some(0));
        assert_eq!(
            menu.entries[0].children[0].url,
            "https://love.bilibili.com/a?q=2#child"
        );
        assert_eq!(menu.entries[1].url, "");
        assert_eq!(menu.entries[1].tid, None);
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn bounds_repeated_record_expansion_and_preserves_duplicates_at_exact_limit()
    -> anyhow::Result<()> {
        let (body, selected_bytes, response_bytes) = repeated_record_response(4);
        assert!(response_bytes <= selected_bytes);
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/x/kv-frontend/namespace/data");
            then.status(200).json_body_obj(&body);
        });

        let below = channel_menu(
            &client(&server),
            &ChannelMenuOptions::new().with_max_response_bytes(selected_bytes - 1),
        )
        .await;
        assert!(matches!(
            below,
            Err(Error::InvalidInput(message)) if message.contains("cumulative selected")
        ));

        let exact = channel_menu(
            &client(&server),
            &ChannelMenuOptions::new().with_max_response_bytes(selected_bytes),
        )
        .await?;
        assert_eq!(exact.entries.len(), 4);
        assert!(
            exact
                .entries
                .iter()
                .all(|entry| entry.key.as_deref() == Some("repeated"))
        );

        let raised = channel_menu(
            &client(&server),
            &ChannelMenuOptions::new().with_max_response_bytes(selected_bytes + 1),
        )
        .await?;
        assert_eq!(raised.entries.len(), 4);
        mock.assert_calls(3);
        Ok(())
    }

    #[tokio::test]
    async fn empty_sort_is_valid_and_missing_metadata_stays_absent() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/x/kv-frontend/namespace/data");
            then.status(200).json_body(json!({"code": 0, "data": {
                "versionId": null, "appVersionId": null,
                "data": {"channel_list.sort": "[]", "side_channel_list.sort": "not json"}
            }}));
        });
        let menu = channel_menu(&client(&server), &ChannelMenuOptions::new()).await?;
        assert_eq!(menu.version_id, None);
        assert_eq!(menu.app_version_id, None);
        assert_eq!(menu.entries.len(), 0);
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn invalid_options_fail_before_network() {
        let server = MockServer::start();
        let any = server.mock(|when, then| {
            when.any_request();
            then.status(200);
        });
        let result = channel_menu(
            &client(&server),
            &ChannelMenuOptions::new().with_max_response_bytes(0),
        )
        .await;
        assert!(matches!(result, Err(Error::InvalidInput(_))));
        any.assert_calls(0);
    }

    #[tokio::test]
    async fn rejects_api_errors_and_missing_payload_fields() {
        let api_server = MockServer::start();
        let api_mock = api_server.mock(|when, then| {
            when.method(GET).path("/x/kv-frontend/namespace/data");
            then.status(200)
                .json_body(json!({"code": -12, "message": "unavailable"}));
        });
        assert!(matches!(
            channel_menu(&client(&api_server), &ChannelMenuOptions::default()).await,
            Err(Error::Api { code: -12, .. })
        ));
        api_mock.assert_calls(1);

        let payload_server = MockServer::start();
        let payload_mock = payload_server.mock(|when, then| {
            when.method(GET).path("/x/kv-frontend/namespace/data");
            then.status(200)
                .json_body(json!({"code": 0, "data": {"versionId": "v"}}));
        });
        assert!(matches!(
            channel_menu(&client(&payload_server), &ChannelMenuOptions::default()).await,
            Err(Error::MissingField("data.data"))
        ));
        payload_mock.assert_calls(1);
    }

    #[tokio::test]
    async fn rejects_malformed_order_and_selected_records() {
        let cases = [
            json!({}),
            json!({"channel_list.sort": 12}),
            json!({"channel_list.sort": "{"}),
            json!({"channel_list.sort": "[\" \"]"}),
            json!({"channel_list.sort": "[\"missing\"]"}),
            json!({"channel_list.sort": "[\"row\"]", "channel_list.row": {}}),
            json!({"channel_list.sort": "[\"row\"]", "channel_list.row": "{"}),
            json!({"channel_list.sort": "[\"row\"]", "channel_list.row": "{\"url\":\"\",\"route\":\"r\"}"}),
            json!({"channel_list.sort": "[\"row\"]", "channel_list.row": "{\"name\":\"n\",\"url\":\"\",\"route\":\" \"}"}),
            json!({"channel_list.sort": "[\"row\"]", "channel_list.row": "{\"name\":\"n\",\"url\":\"\",\"route\":\"r\",\"tid\":\"17\"}"}),
            json!({"channel_list.sort": "[\"row\"]", "channel_list.row": "{\"name\":\"n\",\"url\":\"\",\"route\":\"r\",\"sub\":{}}"}),
            json!({"channel_list.sort": "[\"row\"]", "channel_list.row": "{\"name\":\"n\",\"url\":\"\",\"route\":\"r\",\"config\":{\"enableSub\":\"0\"}}"}),
        ];
        for namespace in cases {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(GET).path("/x/kv-frontend/namespace/data");
                then.status(200).json_body(response(&namespace));
            });
            assert!(
                channel_menu(&client(&server), &ChannelMenuOptions::default())
                    .await
                    .is_err()
            );
            mock.assert_calls(1);
        }
    }

    #[tokio::test]
    async fn rejects_redirect_and_declared_oversized_response() {
        let redirect_server = MockServer::start();
        let redirect = redirect_server.mock(|when, then| {
            when.method(GET).path("/x/kv-frontend/namespace/data");
            then.status(302).header("location", "/elsewhere");
        });
        assert!(matches!(
            channel_menu(&client(&redirect_server), &ChannelMenuOptions::default()).await,
            Err(Error::InvalidInput(message)) if message.contains("redirected")
        ));
        redirect.assert_calls(1);

        let large_server = MockServer::start();
        let large = large_server.mock(|when, then| {
            when.method(GET).path("/x/kv-frontend/namespace/data");
            then.status(200).body("x".repeat(128));
        });
        assert!(matches!(
            channel_menu(
                &client(&large_server),
                &ChannelMenuOptions::new().with_max_response_bytes(64)
            )
            .await,
            Err(Error::InvalidInput(message)) if message.contains("byte limit")
        ));
        large.assert_calls(1);
    }

    #[tokio::test]
    async fn uses_configured_request_timeout() {
        let server = MockServer::start();
        let delayed = server.mock(|when, then| {
            when.method(GET).path("/x/kv-frontend/namespace/data");
            then.delay(Duration::from_millis(150))
                .status(200)
                .json_body(response(&json!({"channel_list.sort": "[]"})));
        });
        let client = BiliClient::new(
            ClientConfig::default()
                .with_endpoints(EndpointConfig::default().with_api_base(server.base_url()))
                .with_request_timeout(Duration::from_millis(20)),
        );
        assert!(
            channel_menu(&client, &ChannelMenuOptions::default())
                .await
                .is_err()
        );
        delayed.assert_calls(1);
    }

    fn chunked_response_server(
        body: String,
    ) -> anyhow::Result<(String, std::thread::JoinHandle<std::io::Result<()>>)> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || -> std::io::Result<()> {
            use std::io::{Read, Write};
            let (mut stream, _) = listener.accept()?;
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request)?;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
                body.len(),
                body
            )?;
            Ok(())
        });
        Ok((format!("http://{address}"), handle))
    }

    #[tokio::test]
    async fn caps_streamed_body_without_content_length() -> anyhow::Result<()> {
        let padding = "x".repeat(DEFAULT_CHANNEL_MENU_MAX_RESPONSE_BYTES);
        let body = response(&json!({"channel_list.sort": "[]", "padding": padding})).to_string();
        let (base, handle) = chunked_response_server(body)?;
        let client = BiliClient::new(
            ClientConfig::default().with_endpoints(EndpointConfig::default().with_api_base(base)),
        );
        let result = channel_menu(&client, &ChannelMenuOptions::default()).await;
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("test server panicked"))??;
        assert!(
            matches!(result, Err(Error::InvalidInput(message)) if message.contains("byte limit"))
        );
        Ok(())
    }

    #[test]
    fn defaults_and_public_serialization_are_stable() -> anyhow::Result<()> {
        let options = ChannelMenuOptions::default();
        assert_eq!(
            options.max_response_bytes,
            DEFAULT_CHANNEL_MENU_MAX_RESPONSE_BYTES
        );
        let serialized = serde_json::to_value(options)?;
        assert_eq!(serialized["max_response_bytes"], 1024 * 1024);
        assert_eq!(DEFAULT_CHANNEL_MENU_MAX_RESPONSE_BYTES, 1024 * 1024);
        Ok(())
    }
}

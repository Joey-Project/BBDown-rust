use super::{ApiData, BiliClient};
use crate::{Error, Result, bv};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CATEGORY_VIDEO_BATCH_SIZE: u32 = 20;
pub const DEFAULT_CATEGORY_VIDEO_MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const CATEGORY_VIDEO_ENDPOINT: &str = "/x/web-interface/newlist";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CategoryVideoQuery {
    pub rid: u32,
    pub batch_size: u32,
    pub max_response_bytes: usize,
}

impl CategoryVideoQuery {
    #[must_use]
    pub const fn new(rid: u32) -> Self {
        Self {
            rid,
            batch_size: DEFAULT_CATEGORY_VIDEO_BATCH_SIZE,
            max_response_bytes: DEFAULT_CATEGORY_VIDEO_MAX_RESPONSE_BYTES,
        }
    }

    #[must_use]
    pub const fn with_batch_size(mut self, batch_size: u32) -> Self {
        self.batch_size = batch_size;
        self
    }

    #[must_use]
    pub const fn with_max_response_bytes(mut self, max_response_bytes: usize) -> Self {
        self.max_response_bytes = max_response_bytes;
        self
    }

    fn validate(&self) -> Result<()> {
        if self.rid == 0 {
            return Err(Error::InvalidInput(
                "category video rid must be greater than zero".to_owned(),
            ));
        }
        if self.batch_size == 0 {
            return Err(Error::InvalidInput(
                "category video batch size must be greater than zero".to_owned(),
            ));
        }
        if self.max_response_bytes == 0 {
            return Err(Error::InvalidInput(
                "category video response byte limit must be greater than zero".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CategoryVideoBatch {
    /// The category ID supplied by the caller.
    pub requested_rid: u32,
    /// The batch size sent to the upstream API, not a guarantee on row count.
    pub requested_batch_size: u32,
    pub items: Vec<CategoryVideoItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CategoryVideoItem {
    pub aid: u64,
    pub bvid: Option<String>,
    pub title: String,
    pub author: Option<String>,
    pub author_mid: Option<u64>,
    pub duration_seconds: Option<u64>,
    pub pub_time: Option<i64>,
    pub cover_url: Option<String>,
    pub description: Option<String>,
    pub category_id: Option<u32>,
    pub category_name: Option<String>,
    pub category_v2_id: Option<u32>,
    pub category_v2_name: Option<String>,
    pub stats: Option<CategoryVideoStats>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CategoryVideoStats {
    pub view: Option<u64>,
    pub danmaku: Option<u64>,
    pub reply: Option<u64>,
    pub favorite: Option<u64>,
    pub coin: Option<u64>,
    pub share: Option<u64>,
    pub like: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CategoryVideoData {
    archives: Option<Vec<CategoryVideoRow>>,
}

#[derive(Debug, Deserialize)]
struct CategoryVideoRow {
    aid: u64,
    bvid: Option<String>,
    title: String,
    desc: Option<String>,
    duration: Option<u64>,
    pubdate: Option<i64>,
    pic: Option<String>,
    tid: Option<u32>,
    tname: Option<String>,
    tidv2: Option<u32>,
    tnamev2: Option<String>,
    owner: Option<CategoryVideoOwner>,
    stat: Option<CategoryVideoStats>,
}

#[derive(Debug, Deserialize)]
struct CategoryVideoOwner {
    mid: Option<u64>,
    name: Option<String>,
}

impl CategoryVideoRow {
    fn into_item(self) -> Result<CategoryVideoItem> {
        if self.aid == 0 {
            return Err(Error::InvalidInput(
                "category video returned a non-positive aid".to_owned(),
            ));
        }
        if self.title.trim().is_empty() {
            return Err(Error::InvalidInput(
                "category video returned an empty title".to_owned(),
            ));
        }
        let bvid = self
            .bvid
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        if let Some(value) = bvid.as_deref()
            && (!value.starts_with("BV") || bv::decode(value)? != self.aid)
        {
            return Err(Error::InvalidInput(
                "category video returned an invalid or mismatched bvid".to_owned(),
            ));
        }
        let owner = self.owner;
        Ok(CategoryVideoItem {
            aid: self.aid,
            bvid,
            title: self.title,
            author: owner
                .as_ref()
                .and_then(|owner| owner.name.clone())
                .and_then(nonempty),
            author_mid: owner.and_then(|owner| owner.mid).filter(|mid| *mid > 0),
            duration_seconds: self.duration,
            pub_time: self.pubdate,
            cover_url: self.pic.and_then(nonempty),
            description: self.desc.and_then(nonempty),
            category_id: self.tid.filter(|id| *id > 0),
            category_name: self.tname.and_then(nonempty),
            category_v2_id: self.tidv2.filter(|id| *id > 0),
            category_v2_name: self.tnamev2.and_then(nonempty),
            stats: self.stat,
        })
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

pub(super) async fn category_videos(
    client: &BiliClient,
    query: CategoryVideoQuery,
) -> Result<CategoryVideoBatch> {
    query.validate()?;
    let mut bounded_client = client.clone();
    bounded_client.http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(BiliClient::http_error_without_url)?;
    let mut url =
        BiliClient::endpoint_url(&client.config.endpoints.api_base, CATEGORY_VIDEO_ENDPOINT)?;
    url.query_pairs_mut()
        .append_pair("rid", &query.rid.to_string())
        .append_pair("pn", "1")
        .append_pair("ps", &query.batch_size.to_string());
    let items = get_category_rows(&bounded_client, url, query.max_response_bytes).await?;
    let items = items
        .into_iter()
        .map(CategoryVideoRow::into_item)
        .collect::<Result<Vec<_>>>()?;
    Ok(CategoryVideoBatch {
        requested_rid: query.rid,
        requested_batch_size: query.batch_size,
        items,
    })
}

async fn get_category_rows(
    client: &BiliClient,
    url: url::Url,
    max_response_bytes: usize,
) -> Result<Vec<CategoryVideoRow>> {
    let response = client
        .http
        .get(url)
        .headers(client.anonymous_headers()?)
        .timeout(client.config.request_timeout)
        .send()
        .await
        .map_err(BiliClient::http_error_without_url)?;
    if response.status().is_redirection() {
        return Err(Error::InvalidInput(
            "category video API endpoint redirected; redirect was not followed".to_owned(),
        ));
    }
    let mut response = response
        .error_for_status()
        .map_err(BiliClient::http_error_without_url)?;
    if response
        .content_length()
        .is_some_and(|length| length > max_response_bytes as u64)
    {
        return Err(response_too_large(max_response_bytes));
    }
    let body = read_bounded_body(&mut response, max_response_bytes).await?;
    let envelope: ApiData<CategoryVideoData> = serde_json::from_slice(&body)?;
    let data = envelope.into_data()?;
    data.archives.ok_or(Error::MissingField("data.archives"))
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
    Error::InvalidInput(format!(
        "category video response exceeded the {max_response_bytes}-byte limit"
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        CategoryVideoQuery, DEFAULT_CATEGORY_VIDEO_BATCH_SIZE,
        DEFAULT_CATEGORY_VIDEO_MAX_RESPONSE_BYTES, category_videos,
    };
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
                .with_credentials(Credentials {
                    cookie: Some(COOKIE_FIXTURE.to_owned()),
                    access_key: Some(ACCESS_KEY_FIXTURE.to_owned()),
                    ..Credentials::default()
                }),
        )
    }

    fn row(aid: u64) -> Value {
        json!({"aid": aid, "title": format!("Video {aid}")})
    }

    #[tokio::test]
    async fn requests_one_anonymous_fixed_first_batch_and_keeps_all_rows() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/newlist")
                .query_param("rid", "7")
                .query_param("pn", "1")
                .query_param("ps", "1")
                .query_param_missing("type")
                .query_param_missing("tid")
                .header_missing("cookie");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {"page": {"count": 0, "num": "invalid", "size": null},
                    "archives": [row(10), row(11)]}
            }));
        });
        let batch = category_videos(
            &client(&server),
            CategoryVideoQuery::new(7).with_batch_size(1),
        )
        .await?;
        assert_eq!(batch.requested_rid, 7);
        assert_eq!(batch.requested_batch_size, 1);
        assert_eq!(
            batch.items.iter().map(|item| item.aid).collect::<Vec<_>>(),
            [10, 11]
        );
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn maps_fields_and_normalizes_optional_values() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/newlist");
            then.status(200)
                .json_body(json!({"code": 0, "data": {"archives": [{
                    "aid": 42,
                    "bvid": "   ",
                    "title": "Title",
                    "desc": "Description",
                    "duration": 123,
                    "pubdate": 1_700_000_000,
                    "pic": "https://example.test/cover.jpg",
                    "tid": 0,
                    "tname": "Legacy",
                    "tidv2": 8,
                    "tnamev2": "Modern",
                    "owner": {"mid": 9, "name": "Creator"},
                    "stat": {"view": 50, "like": 7}
                }]}}));
        });
        let batch = category_videos(&client(&server), CategoryVideoQuery::new(7)).await?;
        let item = &batch.items[0];
        assert_eq!(item.aid, 42);
        assert_eq!(item.bvid, None);
        assert_eq!(item.author.as_deref(), Some("Creator"));
        assert_eq!(item.author_mid, Some(9));
        assert_eq!(item.duration_seconds, Some(123));
        assert_eq!(item.pub_time, Some(1_700_000_000));
        assert_eq!(item.category_id, None);
        assert_eq!(item.category_name.as_deref(), Some("Legacy"));
        assert_eq!(item.category_v2_id, Some(8));
        assert_eq!(item.category_v2_name.as_deref(), Some("Modern"));
        assert_eq!(item.stats.as_ref().and_then(|stats| stats.like), Some(7));
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn omits_missing_and_null_bvid_and_accepts_a_consistent_bvid() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/newlist");
            then.status(200)
                .json_body(json!({"code": 0, "data": {"archives": [
                    {"aid": 455_017_605, "bvid": "BV1Q541167Qg", "title": "Known"},
                    {"aid": 42, "title": "Missing"},
                    {"aid": 43, "bvid": null, "title": "Null"}
                ]}}));
        });
        let batch = category_videos(&client(&server), CategoryVideoQuery::new(7)).await?;
        assert_eq!(batch.items[0].bvid.as_deref(), Some("BV1Q541167Qg"));
        assert_eq!(batch.items[1].bvid, None);
        assert_eq!(batch.items[2].bvid, None);
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_a_valid_bvid_that_does_not_match_the_aid() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/newlist");
            then.status(200)
                .json_body(json!({"code": 0, "data": {"archives": [{
                    "aid": 42, "bvid": "BV1Q541167Qg", "title": "Mismatch"
                }]}}));
        });
        assert!(matches!(
            category_videos(&client(&server), CategoryVideoQuery::new(1)).await,
            Err(Error::InvalidInput(message)) if message.contains("mismatched bvid")
        ));
        mock.assert_calls(1);
    }

    #[tokio::test]
    async fn configured_credentials_are_not_forwarded_and_query_has_only_contract_fields()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/newlist")
                .query_param("rid", "7")
                .query_param("pn", "1")
                .query_param("ps", "20")
                .query_param_count(".*", ".*", 3)
                .query_param_missing("access_key")
                .header_missing("cookie")
                .header_missing("authorization");
            then.status(200)
                .json_body(json!({"code": 0, "data": {"archives": []}}));
        });
        let batch = category_videos(
            &client_with_credentials(&server),
            CategoryVideoQuery::new(7),
        )
        .await?;
        assert_eq!(batch.items, Vec::new());
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn empty_archives_succeeds_and_missing_archives_fails() {
        let empty_server = MockServer::start();
        let empty = empty_server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/newlist");
            then.status(200).json_body(json!({"code": 0, "data": {
                "page": {"count": 0}, "archives": []
            }}));
        });
        let batch = category_videos(&client(&empty_server), CategoryVideoQuery::new(1)).await;
        assert!(matches!(batch, Ok(ref value) if value.items.is_empty()));
        empty.assert_calls(1);

        let missing_server = MockServer::start();
        let missing = missing_server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/newlist");
            then.status(200)
                .json_body(json!({"code": 0, "data": {"page": {}}}));
        });
        assert!(matches!(
            category_videos(&client(&missing_server), CategoryVideoQuery::new(1)).await,
            Err(Error::MissingField("data.archives"))
        ));
        missing.assert_calls(1);
    }

    #[tokio::test]
    async fn rejects_invalid_queries_and_rows() {
        let server = MockServer::start();
        let any = server.mock(|when, then| {
            when.any_request();
            then.status(200);
        });
        for query in [
            CategoryVideoQuery::new(0),
            CategoryVideoQuery::new(1).with_batch_size(0),
            CategoryVideoQuery::new(1).with_max_response_bytes(0),
        ] {
            assert!(matches!(
                category_videos(&client(&server), query).await,
                Err(Error::InvalidInput(_))
            ));
        }
        any.assert_calls(0);

        for archive in [
            json!({"aid": 0, "title": "Invalid"}),
            json!({"aid": 42, "title": "   "}),
            json!({"aid": 42, "bvid": "BV00000000", "title": "Mismatch"}),
        ] {
            let row_server = MockServer::start();
            let mock = row_server.mock(|when, then| {
                when.method(GET).path("/x/web-interface/newlist");
                then.status(200).json_body(json!({"code": 0, "data": {
                    "archives": [archive]
                }}));
            });
            assert!(
                category_videos(&client(&row_server), CategoryVideoQuery::new(1))
                    .await
                    .is_err()
            );
            mock.assert_calls(1);
        }
    }

    #[tokio::test]
    async fn handles_api_errors_redirects_timeout_and_response_cap() {
        let api_server = MockServer::start();
        let api = api_server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/newlist");
            then.status(200)
                .json_body(json!({"code": -1, "message": "failed"}));
        });
        assert!(matches!(
            category_videos(&client(&api_server), CategoryVideoQuery::new(1)).await,
            Err(Error::Api { code: -1, .. })
        ));
        api.assert_calls(1);

        let redirect_server = MockServer::start();
        let redirect = redirect_server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/newlist");
            then.status(302).header("location", "/target");
        });
        assert!(matches!(
            category_videos(&client(&redirect_server), CategoryVideoQuery::new(1)).await,
            Err(Error::InvalidInput(message)) if message.contains("redirected")
        ));
        redirect.assert_calls(1);

        let timeout_server = MockServer::start();
        let slow = timeout_server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/newlist");
            then.delay(Duration::from_millis(300))
                .status(200)
                .json_body(json!({
                    "code": 0, "data": {"archives": []}
                }));
        });
        let timeout_client = BiliClient::new(
            ClientConfig::default()
                .with_endpoints(EndpointConfig::default().with_api_base(timeout_server.base_url()))
                .with_request_timeout(Duration::from_millis(50)),
        );
        assert!(
            category_videos(&timeout_client, CategoryVideoQuery::new(1))
                .await
                .is_err()
        );
        slow.assert_calls(1);

        let cap_server = MockServer::start();
        let cap = cap_server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/newlist");
            then.status(200).json_body(json!({"code": 0, "data": {
                "archives": [], "padding": "xxxxxxxxxxxxxxxx"
            }}));
        });
        assert!(matches!(
            category_videos(
                &client(&cap_server),
                CategoryVideoQuery::new(1).with_max_response_bytes(16)
            )
            .await,
            Err(Error::InvalidInput(message)) if message.contains("byte limit")
        ));
        cap.assert_calls(1);
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

    fn client_with_api_base(api_base: String) -> BiliClient {
        BiliClient::new(
            ClientConfig::default()
                .with_endpoints(EndpointConfig::default().with_api_base(api_base)),
        )
    }

    #[tokio::test]
    async fn streamed_response_without_content_length_is_capped_and_custom_cap_can_accept_it()
    -> anyhow::Result<()> {
        let body = json!({"code": 0, "data": {
            "archives": [], "padding": "x".repeat(DEFAULT_CATEGORY_VIDEO_MAX_RESPONSE_BYTES)
        }})
        .to_string();
        let (api_base, handle) = chunked_response_server(body.clone())?;
        let error = category_videos(
            &client_with_api_base(api_base.clone()),
            CategoryVideoQuery::new(1),
        )
        .await
        .err()
        .ok_or_else(|| {
            anyhow::anyhow!("streamed response without Content-Length must be capped")
        })?;
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("chunked category test server panicked"))??;
        assert!(error.to_string().contains("byte limit"));

        let (api_base, handle) = chunked_response_server(body.clone())?;
        let max_response_bytes = body.len() + 1;
        let batch = category_videos(
            &client_with_api_base(api_base),
            CategoryVideoQuery::new(1).with_max_response_bytes(max_response_bytes),
        )
        .await?;
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("chunked category test server panicked"))??;
        assert_eq!(batch.items, Vec::new());
        Ok(())
    }

    #[test]
    fn defaults_and_public_query_serialization_are_stable() -> anyhow::Result<()> {
        let query = CategoryVideoQuery::new(17);
        assert_eq!(query.batch_size, DEFAULT_CATEGORY_VIDEO_BATCH_SIZE);
        assert_eq!(
            query.max_response_bytes,
            DEFAULT_CATEGORY_VIDEO_MAX_RESPONSE_BYTES
        );
        let value = serde_json::to_value(query)?;
        assert_eq!(value["rid"], 17);
        assert_eq!(value["batch_size"], 20);
        assert_eq!(value["max_response_bytes"], 1024 * 1024);
        Ok(())
    }
}

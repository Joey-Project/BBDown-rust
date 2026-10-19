use super::{ApiData, BiliClient};
use crate::{Error, Result, bv};
use serde::{Deserialize, Serialize};

pub const POPULAR_VIDEO_MAX_PAGE_SIZE: u8 = 20;
const POPULAR_VIDEO_RESPONSE_MAX_BYTES: usize = 1024 * 1024;
const POPULAR_VIDEO_ENDPOINT: &str = "/x/web-interface/popular";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PopularVideoQuery {
    pub page: u32,
    pub page_size: u8,
}

impl Default for PopularVideoQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl PopularVideoQuery {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            page: 1,
            page_size: POPULAR_VIDEO_MAX_PAGE_SIZE,
        }
    }

    #[must_use]
    pub const fn with_page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }

    #[must_use]
    pub const fn with_page_size(mut self, page_size: u8) -> Self {
        self.page_size = page_size;
        self
    }

    fn validate(&self) -> Result<()> {
        if self.page == 0 {
            return Err(Error::InvalidInput(
                "popular video page must be greater than zero".to_owned(),
            ));
        }
        if self.page_size == 0 || self.page_size > POPULAR_VIDEO_MAX_PAGE_SIZE {
            return Err(Error::InvalidInput(format!(
                "popular video page size must be between 1 and {POPULAR_VIDEO_MAX_PAGE_SIZE}"
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PopularVideoPage {
    /// The page number requested from the upstream API.
    pub requested_page: u32,
    /// The page size requested from the upstream API.
    pub requested_page_size: u8,
    pub has_next: bool,
    pub next_page: Option<u32>,
    pub items: Vec<PopularVideoItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PopularVideoItem {
    pub aid: u64,
    pub bvid: Option<String>,
    pub title: String,
    pub author: Option<String>,
    pub author_mid: Option<u64>,
    pub duration_seconds: Option<u64>,
    pub pub_time: Option<i64>,
    pub cover_url: Option<String>,
    pub description: Option<String>,
    pub category_name: Option<String>,
    pub recommendation_reason: Option<String>,
    pub stats: Option<PopularVideoStats>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PopularVideoStats {
    pub view: Option<u64>,
    pub danmaku: Option<u64>,
    pub reply: Option<u64>,
    pub favorite: Option<u64>,
    pub coin: Option<u64>,
    pub share: Option<u64>,
    pub like: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PopularData {
    list: Option<Vec<PopularVideoRow>>,
    no_more: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PopularVideoRow {
    aid: u64,
    bvid: Option<String>,
    title: String,
    desc: Option<String>,
    duration: Option<u64>,
    pubdate: Option<i64>,
    pic: Option<String>,
    tname: Option<String>,
    owner: Option<PopularVideoOwner>,
    stat: Option<PopularVideoStats>,
    rcmd_reason: Option<RecommendationReason>,
}

#[derive(Debug, Deserialize)]
struct PopularVideoOwner {
    mid: Option<u64>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RecommendationReason {
    content: Option<String>,
}

impl PopularVideoRow {
    fn into_item(self) -> Result<PopularVideoItem> {
        if self.aid == 0 {
            return Err(Error::InvalidInput(
                "popular video returned a non-positive aid".to_owned(),
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
                "popular video returned an invalid or mismatched bvid".to_owned(),
            ));
        }
        let owner = self.owner;
        let recommendation_reason = self
            .rcmd_reason
            .and_then(|reason| reason.content)
            .filter(|value| !value.trim().is_empty());
        Ok(PopularVideoItem {
            aid: self.aid,
            bvid,
            title: self.title,
            author: owner
                .as_ref()
                .and_then(|owner| owner.name.clone())
                .filter(|value| !value.trim().is_empty()),
            author_mid: owner.and_then(|owner| owner.mid).filter(|mid| *mid > 0),
            duration_seconds: self.duration,
            pub_time: self.pubdate,
            cover_url: nonempty(self.pic),
            description: nonempty(self.desc),
            category_name: nonempty(self.tname),
            recommendation_reason,
            stats: self.stat,
        })
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

pub(super) async fn popular_videos(
    client: &BiliClient,
    query: PopularVideoQuery,
) -> Result<PopularVideoPage> {
    query.validate()?;
    let mut bounded_client = client.clone();
    bounded_client.http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(BiliClient::http_error_without_url)?;

    let mut url =
        BiliClient::endpoint_url(&client.config.endpoints.api_base, POPULAR_VIDEO_ENDPOINT)?;
    url.query_pairs_mut()
        .append_pair("pn", &query.page.to_string())
        .append_pair("ps", &query.page_size.to_string());

    let response = bounded_client
        .http
        .get(url)
        .headers(bounded_client.anonymous_headers()?)
        .timeout(client.config.request_timeout)
        .send()
        .await
        .map_err(BiliClient::http_error_without_url)?;
    if response.status().is_redirection() {
        return Err(Error::InvalidInput(
            "popular video API endpoint redirected; redirect was not followed".to_owned(),
        ));
    }
    let mut response = response
        .error_for_status()
        .map_err(BiliClient::http_error_without_url)?;
    if response
        .content_length()
        .is_some_and(|length| length > POPULAR_VIDEO_RESPONSE_MAX_BYTES as u64)
    {
        return Err(response_too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(BiliClient::http_error_without_url)?
    {
        if body.len().saturating_add(chunk.len()) > POPULAR_VIDEO_RESPONSE_MAX_BYTES {
            return Err(response_too_large());
        }
        body.extend_from_slice(&chunk);
    }

    let envelope: ApiData<PopularData> = serde_json::from_slice(&body)?;
    let data = envelope.into_data()?;
    let list = data.list.ok_or(Error::MissingField("data.list"))?;
    if list.len() > usize::from(query.page_size)
        || list.len() > usize::from(POPULAR_VIDEO_MAX_PAGE_SIZE)
    {
        return Err(Error::InvalidInput(
            "popular video response contained more rows than the requested page size".to_owned(),
        ));
    }
    let no_more = data.no_more.ok_or(Error::MissingField("data.no_more"))?;
    let has_next = !no_more;
    let next_page = if has_next {
        Some(query.page.checked_add(1).ok_or_else(|| {
            Error::InvalidInput("popular video next page number overflowed".to_owned())
        })?)
    } else {
        None
    };
    let items = list
        .into_iter()
        .map(PopularVideoRow::into_item)
        .collect::<Result<Vec<_>>>()?;

    Ok(PopularVideoPage {
        requested_page: query.page,
        requested_page_size: query.page_size,
        has_next,
        next_page,
        items,
    })
}

fn response_too_large() -> Error {
    Error::InvalidInput(format!(
        "popular video response exceeded the {POPULAR_VIDEO_RESPONSE_MAX_BYTES}-byte limit"
    ))
}

#[cfg(test)]
mod tests {
    use super::{POPULAR_VIDEO_MAX_PAGE_SIZE, PopularVideoQuery, popular_videos};
    use crate::{BiliClient, ClientConfig, Credentials, EndpointConfig, Error};
    use httpmock::prelude::*;
    use serde_json::{Value, json};

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
    async fn requests_configured_api_base_anonymously_and_keeps_upstream_page_contract()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/popular")
                .query_param("pn", "3")
                .query_param("ps", "2")
                .query_param_missing("access_key")
                .header_missing("cookie");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {"list": [row(10), row(11)], "no_more": false}
            }));
        });
        let page = popular_videos(
            &client_with_credentials(&server),
            PopularVideoQuery::new().with_page(3).with_page_size(2),
        )
        .await?;
        assert_eq!(page.requested_page, 3);
        assert_eq!(page.requested_page_size, 2);
        assert!(page.has_next);
        assert_eq!(page.next_page, Some(4));
        assert_eq!(
            page.items.iter().map(|item| item.aid).collect::<Vec<_>>(),
            [10, 11]
        );
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn preserves_optional_fields_and_empty_terminal_page() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/popular")
                .query_param("pn", "1")
                .query_param("ps", "20");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {"list": [], "no_more": true}
            }));
        });
        let page = popular_videos(&client(&server), PopularVideoQuery::default()).await?;
        assert!(!page.has_next);
        assert_eq!(page.next_page, None);
        assert_eq!(page.items, Vec::new());
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn maps_optional_metadata_and_stats_without_inventing_values() -> anyhow::Result<()> {
        let server = MockServer::start();
        let aid = 455_017_605_u64;
        let mock = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/popular");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {"no_more": true, "list": [{
                    "aid": aid,
                    "bvid": "BV1Q541167Qg",
                    "title": "Title",
                    "desc": "Description",
                    "duration": 123,
                    "pubdate": 1_700_000_000,
                    "pic": "https://example.test/cover.jpg",
                    "tname": "Category",
                    "owner": {"mid": 42, "name": "Creator"},
                    "stat": {"view": 50, "like": 7},
                    "rcmd_reason": {"content": "Recommended"}
                }]}
            }));
        });
        let page = popular_videos(&client(&server), PopularVideoQuery::new()).await?;
        let item = &page.items[0];
        assert_eq!(item.aid, aid);
        assert_eq!(item.bvid.as_deref(), Some("BV1Q541167Qg"));
        assert_eq!(item.author.as_deref(), Some("Creator"));
        assert_eq!(item.author_mid, Some(42));
        assert_eq!(item.duration_seconds, Some(123));
        assert_eq!(item.pub_time, Some(1_700_000_000));
        assert_eq!(item.category_name.as_deref(), Some("Category"));
        assert_eq!(item.recommendation_reason.as_deref(), Some("Recommended"));
        let stats = item
            .stats
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("stat object is present"))?;
        assert_eq!(stats.view, Some(50));
        assert_eq!(stats.like, Some(7));
        assert_eq!(stats.coin, None);
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_invalid_queries_before_request() {
        let server = MockServer::start();
        let any_request = server.mock(|when, then| {
            when.any_request();
            then.status(200);
        });
        for query in [
            PopularVideoQuery::new().with_page(0),
            PopularVideoQuery::new().with_page_size(0),
            PopularVideoQuery::new().with_page_size(POPULAR_VIDEO_MAX_PAGE_SIZE + 1),
        ] {
            assert!(matches!(
                popular_videos(&client(&server), query).await,
                Err(Error::InvalidInput(_))
            ));
        }
        any_request.assert_calls(0);
    }

    #[tokio::test]
    async fn rejects_api_and_missing_response_fields() {
        for (body, expected) in [
            (
                json!({"code": -1, "message": "failed", "data": null}),
                "api",
            ),
            (json!({"code": 0, "data": {"no_more": true}}), "list"),
            (json!({"code": 0, "data": {"list": []}}), "no_more"),
        ] {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(GET).path("/x/web-interface/popular");
                then.status(200).json_body(body);
            });
            let result = popular_videos(&client(&server), PopularVideoQuery::new()).await;
            assert!(result.is_err(), "expected {expected} response failure");
            mock.assert_calls(1);
        }
    }

    #[tokio::test]
    async fn rejects_invalid_ids_and_rows_over_page_limit() {
        for list in [
            json!([{"aid": 0, "title": "Invalid"}]),
            json!([{"aid": 455_017_605, "bvid": "BV00000000", "title": "Mismatch"}]),
        ] {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(GET).path("/x/web-interface/popular");
                then.status(200).json_body(json!({
                    "code": 0, "data": {"list": list, "no_more": true}
                }));
            });
            assert!(
                popular_videos(&client(&server), PopularVideoQuery::new())
                    .await
                    .is_err()
            );
            mock.assert_calls(1);
        }

        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/popular");
            then.status(200).json_body(json!({
                "code": 0, "data": {"list": [row(1), row(2)], "no_more": true}
            }));
        });
        assert!(
            popular_videos(&client(&server), PopularVideoQuery::new().with_page_size(1))
                .await
                .is_err()
        );
        mock.assert_calls(1);
    }

    #[tokio::test]
    async fn rejects_next_page_overflow() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/popular")
                .query_param("pn", u32::MAX.to_string());
            then.status(200).json_body(json!({
                "code": 0, "data": {"list": [], "no_more": false}
            }));
        });
        assert!(matches!(
            popular_videos(
                &client(&server),
                PopularVideoQuery::new().with_page(u32::MAX)
            )
            .await,
            Err(Error::InvalidInput(_))
        ));
        mock.assert_calls(1);
    }

    #[tokio::test]
    async fn rejects_oversized_body_and_does_not_follow_redirect() {
        let server = MockServer::start();
        let oversized_body = format!(
            "{{\"code\":0,\"data\":{{\"list\":[],\"no_more\":true,\"padding\":\"{}\"}}}}",
            "x".repeat(1024 * 1024)
        );
        let oversized = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/popular");
            then.status(200)
                .header("content-type", "application/json")
                .body(oversized_body);
        });
        assert!(matches!(
            popular_videos(&client(&server), PopularVideoQuery::new()).await,
            Err(Error::InvalidInput(_))
        ));
        oversized.assert_calls(1);

        let redirect_server = MockServer::start();
        let downstream_server = MockServer::start();
        let downstream = downstream_server.mock(|when, then| {
            when.any_request();
            then.status(200);
        });
        let redirect = redirect_server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/popular");
            then.status(302).header(
                "location",
                format!("{}/unexpected", downstream_server.base_url()),
            );
        });
        assert!(
            popular_videos(&client(&redirect_server), PopularVideoQuery::new())
                .await
                .is_err()
        );
        redirect.assert_calls(1);
        downstream.assert_calls(0);
    }
}

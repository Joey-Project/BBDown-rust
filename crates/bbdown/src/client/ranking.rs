use super::{ApiData, BiliClient};
use crate::{Error, Result, bv};
use serde::{Deserialize, Serialize};

pub const RANKING_VIDEO_MAX_ITEMS: usize = 100;
const RANKING_VIDEO_RESPONSE_MAX_BYTES: usize = 1024 * 1024;
const RANKING_VIDEO_ENDPOINT: &str = "/x/web-interface/ranking/v2";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RankingVideoPage {
    pub note: Option<String>,
    pub items: Vec<RankingVideoItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RankingVideoItem {
    /// The video's position in the upstream list, starting at one.
    pub rank: u32,
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
    pub stats: Option<RankingVideoStats>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RankingVideoStats {
    pub view: Option<u64>,
    pub danmaku: Option<u64>,
    pub reply: Option<u64>,
    pub favorite: Option<u64>,
    pub coin: Option<u64>,
    pub share: Option<u64>,
    pub like: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RankingData {
    note: Option<String>,
    list: Option<Vec<RankingVideoRow>>,
}

#[derive(Debug, Deserialize)]
struct RankingVideoRow {
    aid: u64,
    bvid: Option<String>,
    title: String,
    desc: Option<String>,
    duration: Option<u64>,
    pubdate: Option<i64>,
    pic: Option<String>,
    tname: Option<String>,
    owner: Option<RankingVideoOwner>,
    stat: Option<RankingVideoStats>,
}

#[derive(Debug, Deserialize)]
struct RankingVideoOwner {
    mid: Option<u64>,
    name: Option<String>,
}

impl RankingVideoRow {
    fn into_item(self, rank: u32) -> Result<RankingVideoItem> {
        if self.aid == 0 {
            return Err(Error::InvalidInput(
                "ranking video returned a non-positive aid".to_owned(),
            ));
        }
        if self.title.trim().is_empty() {
            return Err(Error::InvalidInput(
                "ranking video returned an empty title".to_owned(),
            ));
        }
        let bvid = self.bvid.map(|value| value.trim().to_owned());
        if let Some(value) = bvid.as_deref()
            && (!value.starts_with("BV") || bv::decode(value)? != self.aid)
        {
            return Err(Error::InvalidInput(
                "ranking video returned an invalid or mismatched bvid".to_owned(),
            ));
        }
        let owner = self.owner;
        Ok(RankingVideoItem {
            rank,
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
            category_name: self.tname.and_then(nonempty),
            stats: self.stat,
        })
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

pub(super) async fn ranking_videos(client: &BiliClient) -> Result<RankingVideoPage> {
    let mut bounded_client = client.clone();
    bounded_client.http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(BiliClient::http_error_without_url)?;

    let mut url =
        BiliClient::endpoint_url(&client.config.endpoints.api_base, RANKING_VIDEO_ENDPOINT)?;
    url.query_pairs_mut()
        .append_pair("rid", "0")
        .append_pair("type", "all");

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
            "ranking video API endpoint redirected; redirect was not followed".to_owned(),
        ));
    }
    let mut response = response
        .error_for_status()
        .map_err(BiliClient::http_error_without_url)?;
    if response
        .content_length()
        .is_some_and(|length| length > RANKING_VIDEO_RESPONSE_MAX_BYTES as u64)
    {
        return Err(response_too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(BiliClient::http_error_without_url)?
    {
        if body.len().saturating_add(chunk.len()) > RANKING_VIDEO_RESPONSE_MAX_BYTES {
            return Err(response_too_large());
        }
        body.extend_from_slice(&chunk);
    }

    let envelope: ApiData<RankingData> = serde_json::from_slice(&body)?;
    let data = envelope.into_data()?;
    let list = data.list.ok_or(Error::MissingField("data.list"))?;
    if list.len() > RANKING_VIDEO_MAX_ITEMS {
        return Err(Error::InvalidInput(format!(
            "ranking video response contained more than {RANKING_VIDEO_MAX_ITEMS} rows"
        )));
    }
    let items = list
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let rank = u32::try_from(index + 1).map_err(|_| {
                Error::InvalidInput("ranking video rank exceeded the supported range".to_owned())
            })?;
            row.into_item(rank)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(RankingVideoPage {
        note: data.note.and_then(nonempty),
        items,
    })
}

fn response_too_large() -> Error {
    Error::InvalidInput(format!(
        "ranking video response exceeded the {RANKING_VIDEO_RESPONSE_MAX_BYTES}-byte limit"
    ))
}

#[cfg(test)]
mod tests {
    use super::{RANKING_VIDEO_MAX_ITEMS, RankingVideoStats, ranking_videos};
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
    async fn requests_fixed_all_site_ranking_anonymously_once_and_uses_source_position()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let ranking = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/ranking/v2")
                .query_param("rid", "0")
                .query_param("type", "all")
                .query_param_count(".*", ".*", 2)
                .query_param_missing("access_key")
                .header_missing("cookie");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {"note": "  editorial note  ", "list": [
                    {"aid": 10, "title": "First", "rank": 99, "now_rank": 81, "his_rank": 4},
                    {"aid": 10, "title": "Repeated", "rank": 1, "now_rank": 1, "his_rank": 99}
                ]}
            }));
        });
        let unexpected_request = server.mock(|when, then| {
            when.any_request();
            then.status(500);
        });
        let page = ranking_videos(&client_with_credentials(&server)).await?;
        assert_eq!(page.note.as_deref(), Some("  editorial note  "));
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].rank, 1);
        assert_eq!(page.items[1].rank, 2);
        assert_eq!(page.items[0].aid, page.items[1].aid);
        ranking.assert_calls(1);
        unexpected_request.assert_calls(0);
        Ok(())
    }

    #[tokio::test]
    async fn supports_missing_note_optional_metadata_and_empty_list() -> anyhow::Result<()> {
        for body in [
            json!({"code": 0, "data": {"list": [row(7)]}}),
            json!({"code": 0, "data": {"note": "  ", "list": []}}),
        ] {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(GET)
                    .path("/x/web-interface/ranking/v2")
                    .query_param("rid", "0")
                    .query_param("type", "all");
                then.status(200).json_body(body);
            });
            let page = ranking_videos(&client(&server)).await?;
            assert_eq!(page.note, None);
            if page.items.is_empty() {
                assert_eq!(page.items, Vec::new());
            } else {
                assert_eq!(page.items[0].author, None);
                assert_eq!(page.items[0].stats, None);
            }
            mock.assert_calls(1);
        }
        Ok(())
    }

    #[tokio::test]
    async fn maps_optional_metadata_and_stats() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/ranking/v2");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {"note": "Notice", "list": [{
                    "aid": 455_017_605,
                    "bvid": "BV1Q541167Qg",
                    "title": "Title",
                    "desc": "Description",
                    "duration": 123,
                    "pubdate": 1_700_000_000,
                    "pic": "https://example.test/cover.jpg",
                    "tname": "Category",
                    "owner": {"mid": 42, "name": "Creator"},
                    "stat": {"view": 50, "like": 7}
                }]}
            }));
        });
        let page = ranking_videos(&client(&server)).await?;
        let item = &page.items[0];
        assert_eq!(item.rank, 1);
        assert_eq!(item.bvid.as_deref(), Some("BV1Q541167Qg"));
        assert_eq!(item.author.as_deref(), Some("Creator"));
        assert_eq!(item.author_mid, Some(42));
        assert_eq!(item.duration_seconds, Some(123));
        assert_eq!(item.pub_time, Some(1_700_000_000));
        assert_eq!(
            item.cover_url.as_deref(),
            Some("https://example.test/cover.jpg")
        );
        assert_eq!(item.description.as_deref(), Some("Description"));
        assert_eq!(item.category_name.as_deref(), Some("Category"));
        assert_eq!(
            item.stats,
            Some(RankingVideoStats {
                view: Some(50),
                like: Some(7),
                ..RankingVideoStats::default()
            })
        );
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_malformed_ids_required_fields_and_missing_list() {
        let aid = 455_017_605;
        let invalid_responses = [
            (json!({"aid": 0, "title": "Invalid"}), "aid"),
            (
                json!({"aid": aid, "bvid": "BV00000000", "title": "Mismatch"}),
                "bvid",
            ),
            (json!({"aid": aid, "title": "  "}), "title"),
            (json!({"aid": aid}), "title"),
        ];
        for (invalid_row, _) in invalid_responses {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(GET).path("/x/web-interface/ranking/v2");
                then.status(200).json_body(json!({
                    "code": 0, "data": {"list": [invalid_row]}
                }));
            });
            assert!(ranking_videos(&client(&server)).await.is_err());
            mock.assert_calls(1);
        }

        let server = MockServer::start();
        let missing_list = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/ranking/v2");
            then.status(200)
                .json_body(json!({"code": 0, "data": {"note": "note"}}));
        });
        assert!(matches!(
            ranking_videos(&client(&server)).await,
            Err(Error::MissingField("data.list"))
        ));
        missing_list.assert_calls(1);
    }

    #[tokio::test]
    async fn rejects_more_than_max_items_and_oversized_body() {
        let server = MockServer::start();
        let rows = (1..=RANKING_VIDEO_MAX_ITEMS + 1)
            .map(|aid| row(aid as u64))
            .collect::<Vec<_>>();
        let too_many = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/ranking/v2");
            then.status(200)
                .json_body(json!({"code": 0, "data": {"list": rows}}));
        });
        assert!(matches!(
            ranking_videos(&client(&server)).await,
            Err(Error::InvalidInput(_))
        ));
        too_many.assert_calls(1);

        let server = MockServer::start();
        let oversized_body = format!(
            "{{\"code\":0,\"data\":{{\"list\":[],\"padding\":\"{}\"}}}}",
            "x".repeat(1024 * 1024)
        );
        let oversized = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/ranking/v2");
            then.status(200)
                .header("content-type", "application/json")
                .body(oversized_body);
        });
        assert!(matches!(
            ranking_videos(&client(&server)).await,
            Err(Error::InvalidInput(_))
        ));
        oversized.assert_calls(1);
    }

    #[tokio::test]
    async fn rejects_api_errors_redirects_and_timeouts() {
        let server = MockServer::start();
        let api_error = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/ranking/v2");
            then.status(200)
                .json_body(json!({"code": -1, "message": "failed", "data": null}));
        });
        assert!(ranking_videos(&client(&server)).await.is_err());
        api_error.assert_calls(1);

        let redirect_server = MockServer::start();
        let downstream_server = MockServer::start();
        let downstream = downstream_server.mock(|when, then| {
            when.any_request();
            then.status(200);
        });
        let redirect = redirect_server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/ranking/v2");
            then.status(302).header(
                "location",
                format!("{}/unexpected", downstream_server.base_url()),
            );
        });
        assert!(ranking_videos(&client(&redirect_server)).await.is_err());
        redirect.assert_calls(1);
        downstream.assert_calls(0);

        let timeout_server = MockServer::start();
        let slow = timeout_server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/ranking/v2");
            then.delay(Duration::from_millis(100))
                .status(200)
                .json_body(json!({
                    "code": 0, "data": {"list": []}
                }));
        });
        let timeout_client = BiliClient::new(
            ClientConfig::default()
                .with_endpoints(EndpointConfig::default().with_api_base(timeout_server.base_url()))
                .with_request_timeout(Duration::from_millis(10)),
        );
        assert!(ranking_videos(&timeout_client).await.is_err());
        slow.assert_calls(1);
    }
}

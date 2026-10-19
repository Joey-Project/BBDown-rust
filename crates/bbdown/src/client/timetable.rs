use super::{ApiResult, BiliClient};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

pub const PGC_TIMETABLE_MAX_DAYS: usize = 32;
pub const PGC_TIMETABLE_MAX_ROWS: usize = 4096;
const PGC_TIMETABLE_RESPONSE_MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PgcTimetableKind {
    Anime,
    Guochuang,
}

impl PgcTimetableKind {
    const fn season_type(self) -> u8 {
        match self {
            Self::Anime => 1,
            Self::Guochuang => 4,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcTimetable {
    pub kind: PgcTimetableKind,
    pub latest: Vec<PgcTimetableEpisode>,
    pub days: Vec<PgcTimetableDay>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcTimetableDay {
    pub date: String,
    pub date_ts: i64,
    pub day_of_week: i32,
    pub is_today: bool,
    pub episodes: Vec<PgcTimetableEpisode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcTimetableEpisode {
    /// The upstream episode ID, absent when the API does not provide a positive ID.
    pub episode_id: Option<u64>,
    pub season_id: u64,
    pub title: String,
    pub pub_index: String,
    pub pub_time: String,
    pub pub_ts: i64,
    pub published: bool,
    pub delayed: bool,
    pub delay_index: Option<String>,
    pub delay_reason: Option<String>,
    pub cover_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TimetableResult {
    latest: Vec<TimetableEpisode>,
    timeline: Vec<TimetableDay>,
}

#[derive(Debug, Deserialize)]
struct TimetableDay {
    date: String,
    date_ts: i64,
    day_of_week: i32,
    is_today: u8,
    episodes: Vec<TimetableEpisode>,
}

#[derive(Debug, Deserialize)]
struct TimetableEpisode {
    episode_id: Option<u64>,
    season_id: u64,
    title: String,
    pub_index: String,
    pub_time: String,
    pub_ts: i64,
    published: u8,
    delay: u8,
    delay_index: Option<String>,
    delay_reason: Option<String>,
    cover: Option<String>,
    ep_cover: Option<String>,
}

pub(super) async fn pgc_timetable(
    client: &BiliClient,
    kind: PgcTimetableKind,
) -> Result<PgcTimetable> {
    let mut bounded_client = client.clone();
    bounded_client.http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(BiliClient::http_error_without_url)?;

    let mut url =
        BiliClient::endpoint_url(&client.config.endpoints.pgc_base, "/pgc/web/timeline/v2")?;
    url.query_pairs_mut()
        .append_pair("season_type", &kind.season_type().to_string());

    let response = bounded_client
        .http
        .get(url)
        .headers(bounded_client.headers(false)?)
        .timeout(client.config.request_timeout)
        .send()
        .await
        .map_err(BiliClient::http_error_without_url)?;
    if response.status().is_redirection() {
        return Err(Error::InvalidInput(
            "PGC timetable API endpoint redirected; redirect was not followed".to_owned(),
        ));
    }
    let mut response = response
        .error_for_status()
        .map_err(BiliClient::http_error_without_url)?;
    if response
        .content_length()
        .is_some_and(|length| length > PGC_TIMETABLE_RESPONSE_MAX_BYTES as u64)
    {
        return Err(response_too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(BiliClient::http_error_without_url)?
    {
        if body.len().saturating_add(chunk.len()) > PGC_TIMETABLE_RESPONSE_MAX_BYTES {
            return Err(response_too_large());
        }
        body.extend_from_slice(&chunk);
    }

    let envelope: ApiResult<TimetableResult> = serde_json::from_slice(&body)?;
    let result = envelope.into_result()?;
    if result.timeline.len() > PGC_TIMETABLE_MAX_DAYS {
        return Err(Error::InvalidInput(format!(
            "PGC timetable exceeded the {PGC_TIMETABLE_MAX_DAYS}-day limit"
        )));
    }
    let row_count = result.latest.len().saturating_add(
        result
            .timeline
            .iter()
            .map(|day| day.episodes.len())
            .sum::<usize>(),
    );
    if row_count > PGC_TIMETABLE_MAX_ROWS {
        return Err(Error::InvalidInput(format!(
            "PGC timetable exceeded the {PGC_TIMETABLE_MAX_ROWS}-row limit"
        )));
    }

    let latest = result
        .latest
        .into_iter()
        .map(TimetableEpisode::into_public)
        .collect::<Result<Vec<_>>>()?;
    let days = result
        .timeline
        .into_iter()
        .map(TimetableDay::into_public)
        .collect::<Result<Vec<_>>>()?;
    Ok(PgcTimetable { kind, latest, days })
}

impl TimetableDay {
    fn into_public(self) -> Result<PgcTimetableDay> {
        Ok(PgcTimetableDay {
            date: self.date,
            date_ts: self.date_ts,
            day_of_week: self.day_of_week,
            is_today: parse_binary_flag(self.is_today, "result.timeline[].is_today")?,
            episodes: self
                .episodes
                .into_iter()
                .map(TimetableEpisode::into_public)
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

impl TimetableEpisode {
    fn into_public(self) -> Result<PgcTimetableEpisode> {
        if self.season_id == 0 {
            return Err(Error::InvalidInput(
                "PGC timetable returned a non-positive season ID".to_owned(),
            ));
        }
        Ok(PgcTimetableEpisode {
            episode_id: self.episode_id.filter(|id| *id > 0),
            season_id: self.season_id,
            title: self.title,
            pub_index: self.pub_index,
            pub_time: self.pub_time,
            pub_ts: self.pub_ts,
            published: parse_binary_flag(self.published, "result.timeline[].episodes[].published")?,
            delayed: parse_binary_flag(self.delay, "result.timeline[].episodes[].delay")?,
            delay_index: self.delay_index,
            delay_reason: self.delay_reason,
            cover_url: self.ep_cover.or(self.cover),
        })
    }
}

fn parse_binary_flag(value: u8, field: &'static str) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::InvalidInput(format!(
            "PGC timetable returned an invalid {field} value"
        ))),
    }
}

fn response_too_large() -> Error {
    Error::InvalidInput(format!(
        "bounded PGC timetable API response exceeded the {PGC_TIMETABLE_RESPONSE_MAX_BYTES}-byte limit"
    ))
}

#[cfg(test)]
mod tests {
    use super::{PgcTimetableKind, pgc_timetable};
    use crate::{BiliClient, ClientConfig, Credentials, EndpointConfig, Error};
    use httpmock::prelude::*;
    use serde_json::json;

    fn client(server: &MockServer) -> BiliClient {
        BiliClient::new(
            ClientConfig::default()
                .with_endpoints(EndpointConfig::default().with_pgc_base(server.base_url()))
                .with_credentials(Credentials {
                    cookie: Some("codex_synth_v1_access_a".to_owned()),
                    access_key: Some("codex_synth_v1_access_b".to_owned()),
                    ..Credentials::default()
                }),
        )
    }

    fn episode(episode_id: Option<u64>, season_id: u64, published: u8) -> serde_json::Value {
        json!({
            "episode_id": episode_id,
            "season_id": season_id,
            "title": "Example episode",
            "pub_index": "第1话",
            "pub_time": "20:00",
            "pub_ts": 1_792_430_400_i64,
            "published": published,
            "delay": 0,
            "delay_index": null,
            "delay_reason": null,
            "cover": "https://example.test/cover.jpg",
            "ep_cover": null
        })
    }

    #[tokio::test]
    async fn preserves_latest_days_order_and_exact_ids() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/web/timeline/v2")
                .query_param("season_type", "1")
                .query_param_missing("access_key")
                .header_exists("user-agent")
                .header_missing("cookie");
            then.status(200).json_body(json!({
                "code": 0,
                "message": "success",
                "result": {
                    "latest": [episode(Some(123), 456, 1)],
                    "timeline": [
                        {"date": "10-20", "date_ts": 1_792_454_400_i64, "day_of_week": 2, "is_today": 1, "episodes": [episode(Some(123), 456, 1)]},
                        {"date": "10-21", "date_ts": 1_792_540_800_i64, "day_of_week": 3, "is_today": 0, "episodes": []},
                        {"date": "10-22", "date_ts": 1_792_627_200_i64, "day_of_week": 4, "is_today": 0, "episodes": [{"episode_id": null, "season_id": 789, "title": "Example episode", "pub_index": "第1话", "pub_time": "20:00", "pub_ts": 1_792_430_400_i64, "published": 0, "delay": 1, "delay_index": "延期", "delay_reason": "schedule change", "cover": "https://example.test/cover.jpg", "ep_cover": "https://example.test/episode.jpg"}]}
                    ]
                }
            }));
        });

        let timetable = pgc_timetable(&client(&server), PgcTimetableKind::Anime).await?;
        mock.assert();
        assert_eq!(timetable.kind, PgcTimetableKind::Anime);
        assert_eq!(timetable.latest[0].episode_id, Some(123));
        assert_eq!(timetable.days.len(), 3);
        assert_eq!(timetable.days[0].date, "10-20");
        assert!(timetable.days[0].is_today);
        assert_eq!(timetable.days[1].date, "10-21");
        assert_eq!(timetable.days[1].episodes.as_slice(), &[]);
        assert_eq!(timetable.days[2].episodes[0].episode_id, None);
        assert_eq!(timetable.days[2].episodes[0].season_id, 789);
        assert!(!timetable.days[2].episodes[0].published);
        assert!(timetable.days[2].episodes[0].delayed);
        assert_eq!(
            timetable.days[2].episodes[0].delay_reason.as_deref(),
            Some("schedule change")
        );
        assert_eq!(
            timetable.days[2].episodes[0].cover_url.as_deref(),
            Some("https://example.test/episode.jpg")
        );
        Ok(())
    }

    #[tokio::test]
    async fn requests_guochuang_with_only_season_type() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/web/timeline/v2")
                .query_param("season_type", "4");
            then.status(200).json_body(json!({
                "code": 0,
                "message": "success",
                "result": {"latest": [], "timeline": []}
            }));
        });
        let timetable = pgc_timetable(&client(&server), PgcTimetableKind::Guochuang).await?;
        mock.assert();
        assert_eq!(timetable.kind, PgcTimetableKind::Guochuang);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_invalid_envelope_and_service_errors() {
        let server = MockServer::start();
        let mut invalid = server.mock(|when, then| {
            when.method(GET).path("/pgc/web/timeline/v2");
            then.status(200)
                .json_body(json!({"code": 0, "result": {"latest": []}}));
        });
        assert!(matches!(
            pgc_timetable(&client(&server), PgcTimetableKind::Anime).await,
            Err(Error::Json(_) | Error::MissingField(_))
        ));
        invalid.assert();
        invalid.delete();

        let failure = server.mock(|when, then| {
            when.method(GET).path("/pgc/web/timeline/v2");
            then.status(200)
                .json_body(json!({"code": -400, "message": "invalid season type"}));
        });
        assert!(matches!(
            pgc_timetable(&client(&server), PgcTimetableKind::Anime).await,
            Err(Error::Api { code: -400, .. })
        ));
        failure.assert();
    }

    #[tokio::test]
    async fn rejects_missing_or_invalid_publication_status() {
        let server = MockServer::start();
        let missing_status = json!({
            "episode_id": 123,
            "season_id": 456,
            "title": "Example episode",
            "pub_index": "第1话",
            "pub_time": "20:00",
            "pub_ts": 1_792_430_400_i64,
            "delay": 0
        });
        let mut missing = server.mock(|when, then| {
            when.method(GET).path("/pgc/web/timeline/v2");
            then.status(200).json_body(json!({
                "code": 0,
                "message": "success",
                "result": {"latest": [missing_status], "timeline": []}
            }));
        });
        assert!(matches!(
            pgc_timetable(&client(&server), PgcTimetableKind::Anime).await,
            Err(Error::Json(_))
        ));
        missing.assert();
        missing.delete();

        let invalid = server.mock(|when, then| {
            when.method(GET).path("/pgc/web/timeline/v2");
            then.status(200).json_body(json!({
                "code": 0,
                "message": "success",
                "result": {"latest": [episode(Some(123), 456, 2)], "timeline": []}
            }));
        });
        assert!(matches!(
            pgc_timetable(&client(&server), PgcTimetableKind::Anime).await,
            Err(Error::InvalidInput(message)) if message.contains("published")
        ));
        invalid.assert();
    }

    #[tokio::test]
    async fn rejects_redirects_without_following_them() {
        let server = MockServer::start();
        let redirect = server.mock(|when, then| {
            when.method(GET).path("/pgc/web/timeline/v2");
            then.status(302).header("location", "/redirect-target");
        });
        let downstream = server.mock(|when, then| {
            when.method(GET).path("/redirect-target");
            then.status(200).body("unexpected redirect follow");
        });
        assert!(matches!(
            pgc_timetable(&client(&server), PgcTimetableKind::Anime).await,
            Err(Error::InvalidInput(message)) if message.contains("redirected")
        ));
        redirect.assert_calls(1);
        downstream.assert_calls(0);
    }

    #[tokio::test]
    async fn rejects_oversized_body_and_does_not_request_downstream_data() {
        let server = MockServer::start();
        let oversized = " ".repeat(1024 * 1024 + 1);
        let body = format!(
            "{{\"code\":0,\"message\":\"success\",\"result\":{{\"latest\":[],\"timeline\":[]}},\"padding\":\"{oversized}\"}}"
        );
        let mock = server.mock(|when, then| {
            when.method(GET).path("/pgc/web/timeline/v2");
            then.status(200).body(body);
        });
        let downstream = server.mock(|when, then| {
            when.method(GET).path("/pgc/player/web/v2/playurl");
            then.status(200).body("unexpected downstream request");
        });
        assert!(matches!(
            pgc_timetable(&client(&server), PgcTimetableKind::Anime).await,
            Err(Error::InvalidInput(message)) if message.contains("1048576-byte limit")
        ));
        mock.assert_calls(1);
        downstream.assert_calls(0);
    }

    #[tokio::test]
    async fn rejects_more_than_the_day_limit_without_truncating() {
        let server = MockServer::start();
        let days = (0..33)
            .map(|index| {
                json!({
                    "date": format!("10-{index:02}"),
                    "date_ts": index,
                    "day_of_week": index % 7,
                    "is_today": 0,
                    "episodes": []
                })
            })
            .collect::<Vec<_>>();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/pgc/web/timeline/v2");
            then.status(200).json_body(json!({
                "code": 0,
                "message": "success",
                "result": {"latest": [], "timeline": days}
            }));
        });
        assert!(matches!(
            pgc_timetable(&client(&server), PgcTimetableKind::Anime).await,
            Err(Error::InvalidInput(message)) if message.contains("32-day limit")
        ));
        mock.assert_calls(1);
    }

    #[tokio::test]
    async fn rejects_aggregate_latest_and_timeline_row_limit_below_body_limit() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        let row = json!({
            "episode_id": 1,
            "season_id": 2,
            "title": "x",
            "pub_index": "1",
            "pub_time": "1",
            "pub_ts": 1,
            "published": 0,
            "delay": 0
        });
        let payload = json!({
            "code": 0,
            "message": "success",
            "result": {
                "latest": vec![row.clone(); 2048],
                "timeline": [{
                    "date": "10-20",
                    "date_ts": 1,
                    "day_of_week": 2,
                    "is_today": 1,
                    "episodes": vec![row; 2049]
                }]
            }
        });
        let body = serde_json::to_vec(&payload)?;
        assert!(body.len() < 1024 * 1024);
        let mock = server.mock(|when, then| {
            when.method(GET).path("/pgc/web/timeline/v2");
            then.status(200).body(body);
        });
        assert!(matches!(
            pgc_timetable(&client(&server), PgcTimetableKind::Anime).await,
            Err(Error::InvalidInput(message)) if message.contains("4096-row limit")
        ));
        mock.assert_calls(1);
        Ok(())
    }
}

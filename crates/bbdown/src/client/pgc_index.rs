use super::BiliClient;
use crate::{Error, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};

pub const PGC_INDEX_MAX_PAGE_SIZE: u8 = 20;
pub const PGC_INDEX_MAX_FILTER_GROUPS: usize = 32;
pub const PGC_INDEX_MAX_FILTER_VALUES: usize = 4_096;
pub const PGC_INDEX_MAX_ORDERS: usize = 64;
const PGC_INDEX_RESPONSE_MAX_BYTES: usize = 1024 * 1024;
const PGC_INDEX_MAX_FILTER_VALUE_BYTES: usize = 256;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PgcIndexKind {
    Movie,
    Tv,
}

impl PgcIndexKind {
    const fn season_type(self) -> u8 {
        match self {
            Self::Movie => 2,
            Self::Tv => 5,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PgcIndexOrder {
    Play,
    Updated,
    Score,
    ReleaseDate,
    Danmaku,
    Follow,
}

impl PgcIndexOrder {
    const fn value(self) -> u8 {
        match self {
            Self::Play => 2,
            Self::Updated => 0,
            Self::Score => 4,
            Self::ReleaseDate => 6,
            Self::Danmaku => 1,
            Self::Follow => 3,
        }
    }

    const fn supports(self, kind: PgcIndexKind) -> bool {
        match self {
            Self::Play | Self::Updated | Self::Score => true,
            Self::ReleaseDate => matches!(kind, PgcIndexKind::Movie),
            Self::Danmaku | Self::Follow => matches!(kind, PgcIndexKind::Tv),
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PgcIndexSort {
    Descending,
    Ascending,
}

impl PgcIndexSort {
    const fn value(self) -> u8 {
        match self {
            Self::Descending => 0,
            Self::Ascending => 1,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcIndexFilterSelection {
    /// Opaque upstream keyword for the area filter.
    pub area: Option<String>,
    /// Opaque upstream keyword for the style filter (`style_id` in the API).
    pub style_id: Option<String>,
    /// Opaque upstream keyword for the release-date filter.
    pub release_date: Option<String>,
    /// Opaque upstream keyword for the season-status filter.
    pub season_status: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcIndexQuery {
    pub kind: PgcIndexKind,
    pub order: PgcIndexOrder,
    pub sort: PgcIndexSort,
    pub page: u32,
    pub page_size: u8,
    pub filters: PgcIndexFilterSelection,
}

impl PgcIndexQuery {
    #[must_use]
    pub fn new(kind: PgcIndexKind) -> Self {
        Self {
            kind,
            order: PgcIndexOrder::Play,
            sort: PgcIndexSort::Descending,
            page: 1,
            page_size: PGC_INDEX_MAX_PAGE_SIZE,
            filters: PgcIndexFilterSelection::default(),
        }
    }

    #[must_use]
    pub fn with_page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }

    #[must_use]
    pub fn with_order(mut self, order: PgcIndexOrder) -> Self {
        self.order = order;
        self
    }

    #[must_use]
    pub fn with_sort(mut self, sort: PgcIndexSort) -> Self {
        self.sort = sort;
        self
    }

    #[must_use]
    pub fn with_page_size(mut self, page_size: u8) -> Self {
        self.page_size = page_size;
        self
    }

    #[must_use]
    pub fn with_filters(mut self, filters: PgcIndexFilterSelection) -> Self {
        self.filters = filters;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcIndexPage {
    pub kind: PgcIndexKind,
    pub order: PgcIndexOrder,
    pub sort: PgcIndexSort,
    /// Current page number reported by the service.
    pub page: u32,
    /// Page size reported by the service.
    pub page_size: u32,
    pub has_next: bool,
    pub next_page: Option<u32>,
    pub total: u64,
    pub items: Vec<PgcIndexItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcIndexItem {
    pub season_id: u64,
    pub media_id: Option<u64>,
    /// The first episode ID, not necessarily the latest episode.
    pub first_episode_id: Option<u64>,
    pub title: String,
    pub subtitle: Option<String>,
    pub cover_url: Option<String>,
    pub index_show: Option<String>,
    pub finished: Option<bool>,
    pub score: Option<String>,
    pub order_text: Option<String>,
    pub season_status: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcIndexConditions {
    pub kind: PgcIndexKind,
    pub filters: Vec<PgcIndexFilter>,
    pub orders: Vec<PgcIndexOrderOption>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcIndexFilter {
    pub field: String,
    pub name: String,
    pub values: Vec<PgcIndexFilterValue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcIndexFilterValue {
    pub keyword: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PgcIndexOrderOption {
    pub field: String,
    pub name: String,
    pub sort: String,
}

#[derive(Debug, Deserialize)]
struct DataEnvelope<T> {
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<T>,
}

impl<T> DataEnvelope<T> {
    fn into_result(self) -> Result<T> {
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
struct ConditionsResponse {
    filter: Vec<ConditionFilter>,
    order: Vec<ConditionOrder>,
}

#[derive(Debug, Deserialize)]
struct ConditionFilter {
    field: String,
    name: String,
    values: Vec<ConditionFilterValue>,
}

#[derive(Debug, Deserialize)]
struct ConditionFilterValue {
    keyword: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ConditionOrder {
    field: String,
    name: String,
    sort: String,
}

#[derive(Debug, Deserialize)]
struct IndexResponse {
    has_next: u8,
    list: Vec<IndexItem>,
    num: u32,
    size: u32,
    total: u64,
}

#[derive(Debug, Deserialize)]
struct IndexItem {
    season_id: u64,
    media_id: Option<u64>,
    first_ep: Option<FirstEpisode>,
    title: String,
    #[serde(rename = "subTitle")]
    subtitle: Option<String>,
    cover: Option<String>,
    index_show: Option<String>,
    is_finish: Option<u8>,
    score: Option<String>,
    order: Option<String>,
    season_status: Option<i64>,
    season_type: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct FirstEpisode {
    ep_id: Option<u64>,
}

impl IndexItem {
    fn into_public(self, kind: PgcIndexKind) -> Result<PgcIndexItem> {
        if self.season_id == 0 || self.title.trim().is_empty() {
            return Err(Error::InvalidInput(
                "PGC index returned an item without a valid season ID and title".to_owned(),
            ));
        }
        if self
            .season_type
            .is_some_and(|season_type| season_type != kind.season_type())
        {
            return Err(Error::InvalidInput(
                "PGC index returned an item for a different season type".to_owned(),
            ));
        }
        let finished = self
            .is_finish
            .map(|value| parse_binary_flag(value, "data.list[].is_finish"))
            .transpose()?;
        Ok(PgcIndexItem {
            season_id: self.season_id,
            media_id: self.media_id.filter(|id| *id > 0),
            first_episode_id: self
                .first_ep
                .and_then(|episode| episode.ep_id)
                .filter(|id| *id > 0),
            title: self.title,
            subtitle: self.subtitle,
            cover_url: self.cover,
            index_show: self.index_show,
            finished,
            score: self.score,
            order_text: self.order,
            season_status: self.season_status,
        })
    }
}

pub(super) async fn pgc_index(client: &BiliClient, query: PgcIndexQuery) -> Result<PgcIndexPage> {
    validate_query(&query)?;
    let mut bounded_client = client.clone();
    bounded_client.http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(BiliClient::http_error_without_url)?;

    let mut url = BiliClient::endpoint_url(
        &client.config.endpoints.pgc_base,
        "/pgc/season/index/result",
    )?;
    append_common_query(&mut url, query.kind);
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("order", &query.order.value().to_string())
            .append_pair("sort", &query.sort.value().to_string())
            .append_pair("page", &query.page.to_string())
            .append_pair("pagesize", &query.page_size.to_string());
        if let Some(value) = query.filters.area.as_deref() {
            pairs.append_pair("area", value);
        }
        if let Some(value) = query.filters.style_id.as_deref() {
            pairs.append_pair("style_id", value);
        }
        if let Some(value) = query.filters.release_date.as_deref() {
            pairs.append_pair("release_date", value);
        }
        if let Some(value) = query.filters.season_status.as_deref() {
            pairs.append_pair("season_status", value);
        }
    }

    let response = send_bounded(&bounded_client, client, url).await?;
    let envelope: DataEnvelope<IndexResponse> = serde_json::from_slice(&response)?;
    let result = envelope.into_result()?;
    if result.has_next > 1 {
        return Err(Error::InvalidInput(
            "PGC index returned an invalid has_next value".to_owned(),
        ));
    }
    if result.num == 0 {
        return Err(Error::InvalidInput(
            "PGC index returned a non-positive current page number".to_owned(),
        ));
    }
    if result.size == 0
        || result.size > u32::from(PGC_INDEX_MAX_PAGE_SIZE)
        || result.size > u32::from(query.page_size)
        || result.list.len() > usize::from(query.page_size)
        || result.list.len() > usize::try_from(result.size).unwrap_or(usize::MAX)
    {
        return Err(Error::InvalidInput(
            "PGC index returned an invalid or oversized page".to_owned(),
        ));
    }
    let items = result
        .list
        .into_iter()
        .map(|item| item.into_public(query.kind))
        .collect::<Result<Vec<_>>>()?;
    let has_next = result.has_next == 1;
    let next_page = if has_next {
        Some(result.num.checked_add(1).ok_or_else(|| {
            Error::InvalidInput("PGC index next page number overflowed".to_owned())
        })?)
    } else {
        None
    };
    Ok(PgcIndexPage {
        kind: query.kind,
        order: query.order,
        sort: query.sort,
        page: result.num,
        page_size: result.size,
        has_next,
        next_page,
        total: result.total,
        items,
    })
}

pub(super) async fn pgc_index_conditions(
    client: &BiliClient,
    kind: PgcIndexKind,
) -> Result<PgcIndexConditions> {
    let mut bounded_client = client.clone();
    bounded_client.http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(BiliClient::http_error_without_url)?;

    let mut url = BiliClient::endpoint_url(
        &client.config.endpoints.pgc_base,
        "/pgc/season/index/condition",
    )?;
    append_common_query(&mut url, kind);
    let response = send_bounded(&bounded_client, client, url).await?;
    let envelope: DataEnvelope<ConditionsResponse> = serde_json::from_slice(&response)?;
    let result = envelope.into_result()?;
    if result.filter.len() > PGC_INDEX_MAX_FILTER_GROUPS {
        return Err(Error::InvalidInput(format!(
            "PGC index conditions exceeded the {PGC_INDEX_MAX_FILTER_GROUPS}-filter limit"
        )));
    }
    let value_count = result
        .filter
        .iter()
        .map(|filter| filter.values.len())
        .sum::<usize>();
    if value_count > PGC_INDEX_MAX_FILTER_VALUES {
        return Err(Error::InvalidInput(format!(
            "PGC index conditions exceeded the {PGC_INDEX_MAX_FILTER_VALUES}-value limit"
        )));
    }
    if result.order.len() > PGC_INDEX_MAX_ORDERS {
        return Err(Error::InvalidInput(format!(
            "PGC index conditions exceeded the {PGC_INDEX_MAX_ORDERS}-order limit"
        )));
    }
    let filters = result
        .filter
        .into_iter()
        .map(|filter| PgcIndexFilter {
            field: filter.field,
            name: filter.name,
            values: filter
                .values
                .into_iter()
                .map(|value| PgcIndexFilterValue {
                    keyword: value.keyword,
                    name: value.name,
                })
                .collect(),
        })
        .collect();
    let orders = result
        .order
        .into_iter()
        .map(|order| PgcIndexOrderOption {
            field: order.field,
            name: order.name,
            sort: order.sort,
        })
        .collect();
    Ok(PgcIndexConditions {
        kind,
        filters,
        orders,
    })
}

async fn send_bounded(
    bounded_client: &BiliClient,
    client: &BiliClient,
    url: Url,
) -> Result<Vec<u8>> {
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
            "PGC index API endpoint redirected; redirect was not followed".to_owned(),
        ));
    }
    let mut response = response
        .error_for_status()
        .map_err(BiliClient::http_error_without_url)?;
    if response
        .content_length()
        .is_some_and(|length| length > PGC_INDEX_RESPONSE_MAX_BYTES as u64)
    {
        return Err(response_too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(BiliClient::http_error_without_url)?
    {
        if body.len().saturating_add(chunk.len()) > PGC_INDEX_RESPONSE_MAX_BYTES {
            return Err(response_too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn append_common_query(url: &mut Url, kind: PgcIndexKind) {
    url.query_pairs_mut()
        .append_pair("season_type", &kind.season_type().to_string())
        .append_pair("type", "1");
}

fn validate_query(query: &PgcIndexQuery) -> Result<()> {
    if query.page == 0 {
        return Err(Error::InvalidInput(
            "PGC index page must be greater than zero".to_owned(),
        ));
    }
    if query.page_size == 0 || query.page_size > PGC_INDEX_MAX_PAGE_SIZE {
        return Err(Error::InvalidInput(format!(
            "PGC index page size must be between 1 and {PGC_INDEX_MAX_PAGE_SIZE}"
        )));
    }
    if !query.order.supports(query.kind) {
        return Err(Error::Unsupported(
            "PGC index order is not supported for the selected kind".to_owned(),
        ));
    }
    if query.order == PgcIndexOrder::Score && query.sort != PgcIndexSort::Descending {
        return Err(Error::Unsupported(
            "PGC index score order only supports descending sort".to_owned(),
        ));
    }
    for (field, value) in [
        ("area", query.filters.area.as_deref()),
        ("style_id", query.filters.style_id.as_deref()),
        ("release_date", query.filters.release_date.as_deref()),
        ("season_status", query.filters.season_status.as_deref()),
    ] {
        if let Some(value) = value
            && (value.is_empty()
                || value.len() > PGC_INDEX_MAX_FILTER_VALUE_BYTES
                || value.chars().any(char::is_control))
        {
            return Err(Error::InvalidInput(format!(
                "PGC index filter {field} must be non-empty, at most {PGC_INDEX_MAX_FILTER_VALUE_BYTES} bytes, and contain no control characters"
            )));
        }
    }
    Ok(())
}

fn parse_binary_flag(value: u8, field: &'static str) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::InvalidInput(format!(
            "PGC index returned an invalid {field} value"
        ))),
    }
}

fn response_too_large() -> Error {
    Error::InvalidInput(format!(
        "bounded PGC index API response exceeded the {PGC_INDEX_RESPONSE_MAX_BYTES}-byte limit"
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        PGC_INDEX_MAX_FILTER_GROUPS, PGC_INDEX_MAX_FILTER_VALUES, PGC_INDEX_MAX_ORDERS,
        PGC_INDEX_RESPONSE_MAX_BYTES, PgcIndexFilterSelection, PgcIndexKind, PgcIndexOrder,
        PgcIndexQuery, PgcIndexSort, pgc_index, pgc_index_conditions,
    };
    use crate::{BiliClient, ClientConfig, Credentials, EndpointConfig, Error};
    use httpmock::prelude::*;
    use serde_json::json;

    fn client(server: &MockServer) -> BiliClient {
        BiliClient::new(
            ClientConfig::default()
                .with_endpoints(EndpointConfig::default().with_pgc_base(server.base_url())),
        )
    }

    fn client_with_credentials(server: &MockServer) -> BiliClient {
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

    fn item(season_id: u64, season_type: u8) -> serde_json::Value {
        json!({
            "season_id": season_id,
            "media_id": 28_338_128,
            "first_ep": {"cover": "https://example.test/ep.jpg", "ep_id": 604_275},
            "title": "Example title",
            "subTitle": "Example subtitle",
            "cover": "https://example.test/cover.jpg",
            "index_show": "Released",
            "is_finish": 1,
            "order": "2.1亿次播放",
            "score": "9.1",
            "season_status": 13,
            "season_type": season_type
        })
    }

    #[tokio::test]
    async fn movie_index_encodes_opaque_filters_and_reports_actual_page() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/season/index/result")
                .query_param("type", "1")
                .query_param("season_type", "2")
                .query_param("order", "6")
                .query_param("sort", "1")
                .query_param("page", "1")
                .query_param("pagesize", "2")
                .query_param("release_date", "2022年, 2023年")
                .query_param_missing("access_key")
                .header_missing("cookie");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {
                    "has_next": 1,
                    "list": [item(41_937, 2), item(41_938, 2)],
                    "num": 2,
                    "size": 2,
                    "total": 780
                }
            }));
        });
        let query = PgcIndexQuery::new(PgcIndexKind::Movie)
            .with_order(PgcIndexOrder::ReleaseDate)
            .with_sort(PgcIndexSort::Ascending)
            .with_page(1)
            .with_page_size(2)
            .with_filters(PgcIndexFilterSelection {
                release_date: Some("2022年, 2023年".to_owned()),
                ..PgcIndexFilterSelection::default()
            });

        let page = pgc_index(&client(&server), query).await?;
        assert_eq!(page.page, 2);
        assert_eq!(page.page_size, 2);
        assert_eq!(page.next_page, Some(3));
        assert_eq!(page.total, 780);
        assert_eq!(page.items[0].season_id, 41_937);
        assert_eq!(page.items[0].first_episode_id, Some(604_275));
        assert_eq!(page.items[0].subtitle.as_deref(), Some("Example subtitle"));
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn tv_index_accepts_tv_orders_and_returns_empty_optional_ids_as_none()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/season/index/result")
                .query_param("type", "1")
                .query_param("season_type", "5")
                .query_param("order", "1")
                .query_param("sort", "0")
                .query_param("page", "1")
                .query_param("pagesize", "20");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {
                    "has_next": 0,
                    "list": [{
                        "season_id": 9,
                        "media_id": 0,
                        "first_ep": {"ep_id": 0},
                        "title": "TV",
                        "is_finish": 0,
                        "season_type": 5
                    }],
                    "num": 1,
                    "size": 20,
                    "total": 1
                }
            }));
        });

        let page = pgc_index(
            &client(&server),
            PgcIndexQuery::new(PgcIndexKind::Tv).with_order(PgcIndexOrder::Danmaku),
        )
        .await?;
        assert_eq!(page.next_page, None);
        assert_eq!(page.items[0].media_id, None);
        assert_eq!(page.items[0].first_episode_id, None);
        assert_eq!(page.items[0].finished, Some(false));
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn conditions_preserve_service_fields_values_and_order() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/season/index/condition")
                .query_param("type", "1")
                .query_param("season_type", "2")
                .query_param_missing("access_key")
                .header_missing("cookie");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {
                    "filter": [
                        {"field": "area", "name": "地区", "values": [
                            {"keyword": "-1", "name": "全部"},
                            {"keyword": "1", "name": "中国大陆"}
                        ]},
                        {"field": "release_date", "name": "上映时间", "values": []}
                    ],
                    "order": [{"field": "2", "name": "播放数量", "sort": "0,1"}]
                }
            }));
        });

        let conditions = pgc_index_conditions(&client(&server), PgcIndexKind::Movie).await?;
        assert_eq!(conditions.filters[0].field, "area");
        assert_eq!(conditions.filters[0].values[1].keyword, "1");
        assert_eq!(conditions.filters[1].values.len(), 0);
        assert_eq!(conditions.orders[0].sort, "0,1");
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn invalid_query_and_filter_combinations_fail_before_request() {
        let server = MockServer::start();
        let any_request = server.mock(|when, then| {
            when.method(GET);
            then.status(200).json_body(json!({}));
        });
        let client = client(&server);
        let invalid = [
            PgcIndexQuery::new(PgcIndexKind::Movie).with_page(0),
            PgcIndexQuery::new(PgcIndexKind::Movie).with_page_size(21),
            PgcIndexQuery::new(PgcIndexKind::Tv).with_order(PgcIndexOrder::ReleaseDate),
            PgcIndexQuery::new(PgcIndexKind::Movie)
                .with_sort(PgcIndexSort::Ascending)
                .with_order(PgcIndexOrder::Score),
            PgcIndexQuery::new(PgcIndexKind::Movie).with_filters(PgcIndexFilterSelection {
                area: Some("bad\nvalue".to_owned()),
                ..PgcIndexFilterSelection::default()
            }),
        ];
        for query in invalid {
            assert!(matches!(
                pgc_index(&client, query).await,
                Err(Error::InvalidInput(_) | Error::Unsupported(_))
            ));
        }
        any_request.assert_calls(0);
    }

    #[tokio::test]
    async fn configured_credentials_are_not_sent() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/season/index/result")
                .query_param_missing("access_key")
                .header_missing("cookie")
                .header_missing("authorization");
            then.status(200).json_body(json!({
                "code": 0,
                "data": {"has_next": 0, "list": [], "num": 1, "size": 20, "total": 0}
            }));
        });

        pgc_index(
            &client_with_credentials(&server),
            PgcIndexQuery::new(PgcIndexKind::Movie),
        )
        .await?;
        mock.assert_calls(1);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_invalid_envelopes_and_page_metadata() {
        let cases = [
            (
                "API error envelope",
                json!({"code": -400, "message": "invalid request"}),
            ),
            ("missing data", json!({"code": 0})),
            (
                "zero page number",
                json!({"code": 0, "data": {"has_next": 0, "list": [], "num": 0, "size": 1, "total": 0}}),
            ),
            (
                "zero page size",
                json!({"code": 0, "data": {"has_next": 0, "list": [], "num": 1, "size": 0, "total": 0}}),
            ),
            (
                "page size exceeds request",
                json!({"code": 0, "data": {"has_next": 0, "list": [], "num": 1, "size": 2, "total": 0}}),
            ),
            (
                "rows exceed actual size",
                json!({"code": 0, "data": {"has_next": 0, "list": [item(1, 2), item(2, 2)], "num": 1, "size": 1, "total": 2}}),
            ),
            (
                "invalid has_next",
                json!({"code": 0, "data": {"has_next": 2, "list": [], "num": 1, "size": 1, "total": 0}}),
            ),
            (
                "next page overflow",
                json!({"code": 0, "data": {"has_next": 1, "list": [], "num": 4_294_967_295_u64, "size": 1, "total": 0}}),
            ),
        ];

        for (case_name, body) in cases {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(GET).path("/pgc/season/index/result");
                then.status(200).json_body(body);
            });
            let query = PgcIndexQuery::new(PgcIndexKind::Movie).with_page_size(1);
            assert!(
                pgc_index(&client(&server), query).await.is_err(),
                "case should fail: {case_name}"
            );
            mock.assert_calls(1);
        }
    }

    #[tokio::test]
    async fn rejects_condition_group_value_and_order_caps_without_truncating() {
        let cases = [
            (
                "too many groups",
                json!({
                    "filter": (0..=PGC_INDEX_MAX_FILTER_GROUPS).map(|index| json!({"field": format!("field{index}"), "name": "filter", "values": []})).collect::<Vec<_>>(),
                    "order": []
                }),
            ),
            (
                "too many aggregate values",
                json!({
                    "filter": [{"field": "area", "name": "area", "values": (0..=PGC_INDEX_MAX_FILTER_VALUES).map(|index| json!({"keyword": index.to_string(), "name": "value"})).collect::<Vec<_>>()}],
                    "order": []
                }),
            ),
            (
                "too many orders",
                json!({
                    "filter": [],
                    "order": (0..=PGC_INDEX_MAX_ORDERS).map(|index| json!({"field": index.to_string(), "name": "order", "sort": "0,1"})).collect::<Vec<_>>()
                }),
            ),
        ];

        for (case_name, data) in cases {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(GET).path("/pgc/season/index/condition");
                then.status(200).json_body(json!({"code": 0, "data": data}));
            });
            assert!(
                matches!(
                    pgc_index_conditions(&client(&server), PgcIndexKind::Tv).await,
                    Err(Error::InvalidInput(_))
                ),
                "case should fail: {case_name}"
            );
            mock.assert_calls(1);
        }
    }

    #[tokio::test]
    async fn rejects_oversized_response_body() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/pgc/season/index/result");
            then.status(200)
                .body(" ".repeat(PGC_INDEX_RESPONSE_MAX_BYTES + 1));
        });

        assert!(matches!(
            pgc_index(
                &client(&server),
                PgcIndexQuery::new(PgcIndexKind::Movie),
            )
            .await,
            Err(Error::InvalidInput(message)) if message.contains("response exceeded")
        ));
        mock.assert_calls(1);
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let destination = MockServer::start();
        let downstream = destination.mock(|when, then| {
            when.method(GET);
            then.status(200).json_body(json!({
                "code": 0,
                "data": {"has_next": 0, "list": [], "num": 1, "size": 20, "total": 0}
            }));
        });
        let origin = MockServer::start();
        let redirect = origin.mock(|when, then| {
            when.method(GET).path("/pgc/season/index/result");
            then.status(302).header(
                "location",
                format!("{}/pgc/season/index/result", destination.base_url()),
            );
        });

        assert!(matches!(
            pgc_index(
                &client(&origin),
                PgcIndexQuery::new(PgcIndexKind::Movie),
            )
            .await,
            Err(Error::InvalidInput(message)) if message.contains("redirected")
        ));
        redirect.assert_calls(1);
        downstream.assert_calls(0);
    }
}

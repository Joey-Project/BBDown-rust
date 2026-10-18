use super::{
    ApiData, BiliClient, NavData, current_unix_timestamp, wbi_mixin_key, wbi_signed_query,
};
use crate::bv;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const VIDEO_SEARCH_MAX_ITEMS: usize = 20;
const VIDEO_SEARCH_RESPONSE_MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VideoSearchQuery {
    pub keyword: String,
    pub page: u32,
    pub order: VideoSearchOrder,
}

impl VideoSearchQuery {
    #[must_use]
    pub fn new(keyword: impl Into<String>) -> Self {
        Self {
            keyword: keyword.into(),
            page: 1,
            order: VideoSearchOrder::TotalRank,
        }
    }

    #[must_use]
    pub fn with_page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }

    #[must_use]
    pub fn with_order(mut self, order: VideoSearchOrder) -> Self {
        self.order = order;
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let keyword = self.keyword.trim();
        if keyword.is_empty() || keyword.len() > 200 || keyword.chars().any(char::is_control) {
            return Err(Error::InvalidInput(
                "video search keyword must contain 1 to 200 non-control bytes".to_owned(),
            ));
        }
        if filter_wbi_keyword(keyword).is_empty() {
            return Err(Error::InvalidInput(
                "video search keyword is empty after WBI normalization".to_owned(),
            ));
        }
        if self.page == 0 {
            return Err(Error::InvalidInput(
                "video search page must be greater than zero".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum VideoSearchOrder {
    #[default]
    #[serde(rename = "totalrank")]
    TotalRank,
    #[serde(rename = "click")]
    Click,
    #[serde(rename = "pubdate")]
    PubDate,
    #[serde(rename = "dm")]
    Danmaku,
    #[serde(rename = "stow")]
    Favorites,
    #[serde(rename = "scores")]
    Comments,
}

impl VideoSearchOrder {
    pub(crate) const fn api_value(self) -> &'static str {
        match self {
            Self::TotalRank => "totalrank",
            Self::Click => "click",
            Self::PubDate => "pubdate",
            Self::Danmaku => "dm",
            Self::Favorites => "stow",
            Self::Comments => "scores",
        }
    }

    pub(crate) fn parse_api_value(value: &str) -> Result<Self> {
        match value {
            "" | "totalrank" => Ok(Self::TotalRank),
            "click" => Ok(Self::Click),
            "pubdate" => Ok(Self::PubDate),
            "dm" => Ok(Self::Danmaku),
            "stow" => Ok(Self::Favorites),
            "scores" => Ok(Self::Comments),
            _ => Err(Error::InvalidInput(format!(
                "unsupported video search order `{value}`"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VideoSearchPage {
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
    pub total_pages: u32,
    pub next_page: Option<u32>,
    pub items: Vec<VideoSearchItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VideoSearchItem {
    pub aid: Option<u64>,
    pub bvid: Option<String>,
    pub cid: Option<u64>,
    pub title: String,
    pub author: Option<String>,
    pub author_mid: Option<u64>,
    pub duration: Option<String>,
    pub pub_time: Option<i64>,
    pub play_count: Option<u64>,
    pub comment_count: Option<u64>,
    pub danmaku_count: Option<u64>,
    pub favorite_count: Option<u64>,
    pub cover_url: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchPageData {
    page: Option<u32>,
    #[serde(rename = "numResults")]
    total: Option<u64>,
    #[serde(rename = "numPages")]
    total_pages: Option<u32>,
    pagesize: Option<u32>,
    result: Option<Vec<SearchVideoRow>>,
}

#[derive(Debug, Deserialize)]
struct SearchVideoRow {
    #[serde(rename = "type")]
    item_type: Option<String>,
    aid: Option<u64>,
    bvid: Option<String>,
    cid: Option<u64>,
    title: Option<String>,
    author: Option<String>,
    mid: Option<u64>,
    duration: Option<String>,
    pubdate: Option<i64>,
    play: Option<u64>,
    review: Option<u64>,
    video_review: Option<u64>,
    favorites: Option<u64>,
    pic: Option<String>,
    description: Option<String>,
}

impl SearchVideoRow {
    fn into_item(self) -> Option<VideoSearchItem> {
        if self.item_type.as_deref() != Some("video") {
            return None;
        }
        let bvid = self
            .bvid
            .map(|value| value.trim().to_owned())
            .filter(|value| value.starts_with("BV") && bv::decode(value).is_ok());
        let decoded_aid = bvid.as_deref().and_then(|value| bv::decode(value).ok());
        let aid = self.aid.filter(|aid| *aid > 0).or(decoded_aid);
        if aid.is_some() && decoded_aid.is_some_and(|decoded| Some(decoded) != aid) {
            return None;
        }
        if aid.is_none() && bvid.is_none() {
            return None;
        }
        let title = self.title.map(|value| clean_highlighted_text(&value))?;
        if title.is_empty() {
            return None;
        }
        Some(VideoSearchItem {
            aid,
            bvid,
            cid: self.cid.filter(|cid| *cid > 0),
            title,
            author: self.author.map(|value| clean_highlighted_text(&value)),
            author_mid: self.mid.filter(|mid| *mid > 0),
            duration: self.duration.filter(|value| !value.trim().is_empty()),
            pub_time: self.pubdate.filter(|value| *value > 0),
            play_count: self.play,
            comment_count: self.review,
            danmaku_count: self.video_review,
            favorite_count: self.favorites,
            cover_url: self.pic.filter(|value| !value.trim().is_empty()),
            description: self.description.map(|value| clean_highlighted_text(&value)),
        })
    }
}

pub(super) async fn search_videos(
    client: &BiliClient,
    query: VideoSearchQuery,
) -> Result<VideoSearchPage> {
    query.validate()?;

    // Clone only the client handles and config to disable redirects for these
    // anonymous, bounded requests without affecting other client operations.
    let mut bounded_client = client.clone();
    bounded_client.http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(BiliClient::http_error_without_url)?;
    let nav_url =
        BiliClient::endpoint_url(&client.config.endpoints.api_base, "/x/web-interface/nav")?;
    let nav: ApiData<NavData> = get_bounded_json_without_cookie(&bounded_client, nav_url).await?;
    if nav.code != 0 && nav.code != -101 {
        return Err(Error::Api {
            code: nav.code,
            message: nav.message,
        });
    }
    let nav_data = nav.data.ok_or(Error::MissingField("data.wbi_img"))?;
    let mixin_key = wbi_mixin_key(
        nav_data
            .wbi_img
            .img_url
            .as_deref()
            .ok_or(Error::MissingField("data.wbi_img.img_url"))?,
        nav_data
            .wbi_img
            .sub_url
            .as_deref()
            .ok_or(Error::MissingField("data.wbi_img.sub_url"))?,
    )?;
    let signed_query = signed_video_search_query(
        &query.keyword,
        query.page,
        query.order,
        &mixin_key,
        &current_unix_timestamp().to_string(),
    )?;
    let mut url = BiliClient::endpoint_url(
        &client.config.endpoints.api_base,
        "/x/web-interface/wbi/search/type",
    )?;
    url.set_query(Some(&signed_query));
    let response: ApiData<SearchPageData> =
        get_bounded_json_without_cookie(&bounded_client, url).await?;
    let data = response.into_data()?;
    let total = data.total.ok_or(Error::MissingField("data.numResults"))?;
    let total_pages = data
        .total_pages
        .ok_or(Error::MissingField("data.numPages"))?;
    let page_size = data.pagesize.ok_or(Error::MissingField("data.pagesize"))?;
    let actual_page = data.page.unwrap_or(query.page);
    if actual_page == 0 {
        return Err(Error::InvalidInput(
            "video search returned an invalid page number".to_owned(),
        ));
    }
    if page_size == 0 || page_size as usize > VIDEO_SEARCH_MAX_ITEMS {
        return Err(Error::InvalidInput(format!(
            "video search page size must be between 1 and {VIDEO_SEARCH_MAX_ITEMS}"
        )));
    }
    let rows = data.result.unwrap_or_default();
    if rows.len() > VIDEO_SEARCH_MAX_ITEMS || rows.len() > page_size as usize {
        return Err(Error::InvalidInput(format!(
            "video search returned more than the {VIDEO_SEARCH_MAX_ITEMS}-item page limit"
        )));
    }
    let mut seen_aids = HashSet::new();
    let mut seen_bvids = HashSet::new();
    let items = rows
        .into_iter()
        .filter_map(SearchVideoRow::into_item)
        .filter(|item| {
            if item.aid.is_some_and(|aid| !seen_aids.insert(aid)) {
                return false;
            }
            if item
                .bvid
                .as_ref()
                .is_some_and(|bvid| !seen_bvids.insert(bvid.clone()))
            {
                return false;
            }
            true
        })
        .collect();
    let next_page = if actual_page < total_pages {
        actual_page.checked_add(1)
    } else {
        None
    };
    Ok(VideoSearchPage {
        page: actual_page,
        page_size,
        total,
        total_pages,
        next_page,
        items,
    })
}

fn filter_wbi_keyword(keyword: &str) -> String {
    keyword
        .chars()
        .filter(|character| !matches!(character, '!' | '\'' | '(' | ')' | '*'))
        .collect()
}

fn signed_video_search_query(
    keyword: &str,
    page: u32,
    order: VideoSearchOrder,
    mixin_key: &str,
    wts: &str,
) -> Result<String> {
    let keyword = filter_wbi_keyword(keyword.trim());
    if keyword.is_empty() {
        return Err(Error::InvalidInput(
            "video search keyword is empty after WBI normalization".to_owned(),
        ));
    }
    let params = vec![
        ("keyword", keyword),
        ("order", order.api_value().to_owned()),
        ("page", page.to_string()),
        ("search_type", "video".to_owned()),
        ("wts", wts.to_owned()),
    ];
    Ok(wbi_signed_query(params, mixin_key))
}

async fn get_bounded_json_without_cookie<T>(client: &BiliClient, url: url::Url) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let response = client
        .http
        .get(url)
        .headers(client.headers(false)?)
        .timeout(client.config.request_timeout)
        .send()
        .await
        .map_err(BiliClient::http_error_without_url)?;
    if response.status().is_redirection() {
        return Err(Error::InvalidInput(
            "video search API endpoint redirected; redirect was not followed".to_owned(),
        ));
    }
    let mut response = response
        .error_for_status()
        .map_err(BiliClient::http_error_without_url)?;
    if response
        .content_length()
        .is_some_and(|content_length| content_length > VIDEO_SEARCH_RESPONSE_MAX_BYTES as u64)
    {
        return Err(Error::InvalidInput(format!(
            "bounded video search API response exceeded the {VIDEO_SEARCH_RESPONSE_MAX_BYTES}-byte limit"
        )));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(BiliClient::http_error_without_url)?
    {
        if body.len().saturating_add(chunk.len()) > VIDEO_SEARCH_RESPONSE_MAX_BYTES {
            return Err(Error::InvalidInput(format!(
                "bounded video search API response exceeded the {VIDEO_SEARCH_RESPONSE_MAX_BYTES}-byte limit"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(Error::from)
}

fn clean_highlighted_text(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut inside_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' if !inside_tag => inside_tag = true,
            '>' if inside_tag => inside_tag = false,
            _ if !inside_tag => text.push(ch),
            _ => {}
        }
    }
    decode_html_entities(&text).trim().to_owned()
}

fn decode_html_entities(raw: &str) -> String {
    let mut decoded = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let entity = &rest[start + 1..];
        let Some(end) = entity.find(';') else {
            decoded.push('&');
            rest = entity;
            continue;
        };
        let name = &entity[..end];
        if let Some(value) = decode_entity(name) {
            decoded.push(value);
        } else {
            decoded.push('&');
            decoded.push_str(name);
            decoded.push(';');
        }
        rest = &entity[end + 1..];
    }
    decoded.push_str(rest);
    decoded
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some('\u{00a0}'),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ => entity
            .strip_prefix('#')
            .and_then(|value| value.parse::<u32>().ok())
            .and_then(char::from_u32),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SearchVideoRow, VideoSearchItem, VideoSearchOrder, VideoSearchQuery,
        clean_highlighted_text, decode_html_entities, search_videos, signed_video_search_query,
    };
    use crate::client::TagPolicy;
    use crate::{BiliClient, ClientConfig, Credentials, EndpointConfig, Error, Input, Selection};
    use httpmock::prelude::*;
    use serde_json::{Value, json};

    #[test]
    fn query_defaults_and_order_wire_codes_are_stable() {
        let query = VideoSearchQuery::new(" Rust ");
        assert_eq!(query.page, 1);
        assert_eq!(query.order, VideoSearchOrder::TotalRank);
        assert_eq!(VideoSearchOrder::Danmaku.api_value(), "dm");
        assert_eq!(VideoSearchOrder::Favorites.api_value(), "stow");
        assert_eq!(VideoSearchOrder::Comments.api_value(), "scores");
    }

    #[test]
    fn query_validation_rejects_empty_control_and_out_of_range_inputs() {
        for query in [
            VideoSearchQuery::new(" "),
            VideoSearchQuery::new("!'()*"),
            VideoSearchQuery::new("bad\nquery"),
            VideoSearchQuery::new("rust").with_page(0),
        ] {
            assert!(matches!(query.validate(), Err(Error::InvalidInput(_))));
        }
        assert!(
            VideoSearchQuery::new("rust")
                .with_page(51)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn search_order_serialization_uses_wire_codes() -> anyhow::Result<()> {
        assert_eq!(
            serde_json::to_string(&VideoSearchOrder::TotalRank)?,
            "\"totalrank\""
        );
        assert_eq!(serde_json::to_string(&VideoSearchOrder::Danmaku)?, "\"dm\"");
        Ok(())
    }

    #[test]
    fn forbidden_wbi_keyword_characters_are_filtered_before_signing() -> anyhow::Result<()> {
        let signed = signed_video_search_query(
            "Rust's async C++ (tutorial)!*",
            2,
            VideoSearchOrder::TotalRank,
            "0123456789abcdef",
            "1700000000",
        )?;
        assert_eq!(
            signed,
            "keyword=Rusts+async+C%2B%2B+tutorial&order=totalrank&page=2&search_type=video&wts=1700000000&w_rid=b596c0401d43384988d6613b47b34dc8"
        );
        assert!(matches!(
            signed_video_search_query(
                "!'()*",
                1,
                VideoSearchOrder::TotalRank,
                "0123456789abcdef",
                "1700000000",
            ),
            Err(Error::InvalidInput(_))
        ));
        Ok(())
    }

    #[test]
    fn title_cleaning_removes_highlight_markup_and_decodes_entities() {
        assert_eq!(
            clean_highlighted_text("a<em class=\"keyword\">Rust</em> &amp; B&#x69;li"),
            "aRust & Bili"
        );
        assert_eq!(
            decode_html_entities("&lt;tag&gt; &unknown;"),
            "<tag> &unknown;"
        );
    }

    #[test]
    fn rows_without_video_identity_or_title_are_filtered() {
        let item = SearchVideoRow {
            item_type: Some("video".to_owned()),
            aid: None,
            bvid: None,
            cid: None,
            title: Some("Missing IDs".to_owned()),
            author: None,
            mid: None,
            duration: None,
            pubdate: None,
            play: None,
            review: None,
            video_review: None,
            favorites: None,
            pic: None,
            description: None,
        };
        assert!(item.into_item().is_none());
    }

    #[tokio::test]
    async fn search_is_one_unicode_page_signed_against_configured_api_base_without_cookie()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let nav = server.mock(|when, then| {
            when.method(GET)
                .path("/prefix/x/web-interface/nav")
                .header_missing("Cookie");
            then.status(200).json_body(nav_response(-101));
        });
        let search = server.mock(|when, then| {
            when.method(GET)
                .path("/prefix/x/web-interface/wbi/search/type")
                .query_param("keyword", "Rust 搜索")
                .query_param("search_type", "video")
                .query_param("page", "2")
                .query_param("order", "stow")
                .query_param_exists("wts")
                .query_param_exists("w_rid")
                .header_missing("Cookie");
            then.status(200).json_body(search_response(
                45,
                3,
                20,
                &[video_row("BV1Q541167Qg", "<em>Rust</em> &amp; Bili")],
            ));
        });
        let client = test_client(&server, true);
        let page = search_videos(
            &client,
            VideoSearchQuery::new("Rust 搜索!'()*")
                .with_page(2)
                .with_order(VideoSearchOrder::Favorites),
        )
        .await?;

        assert_eq!(page.page, 2);
        assert_eq!(page.page_size, 20);
        assert_eq!(page.total, 45);
        assert_eq!(page.total_pages, 3);
        assert_eq!(page.next_page, Some(3));
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].title, "Rust & Bili");
        assert_eq!(page.items[0].aid, Some(455_017_605));
        assert_eq!(nav.calls(), 1);
        assert_eq!(search.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn invalid_query_options_fail_before_any_request() {
        let server = MockServer::start();
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body(nav_response(0));
        });
        let client = test_client(&server, false);
        for query in [
            VideoSearchQuery::new(" "),
            VideoSearchQuery::new("!'()*"),
            VideoSearchQuery::new("bad\u{0007}query"),
            VideoSearchQuery::new("valid").with_page(0),
        ] {
            assert!(matches!(
                search_videos(&client, query).await,
                Err(Error::InvalidInput(_))
            ));
        }
        assert_eq!(nav.calls(), 0);
    }

    #[tokio::test]
    async fn search_uses_the_page_returned_by_the_service_for_continuation() -> anyhow::Result<()> {
        let server = MockServer::start();
        let _nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body(nav_response(0));
        });
        let mut search = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/wbi/search/type")
                .query_param("page", "999");
            then.status(200)
                .json_body(search_response_with_page(4, 5, 5, 20, &[]));
        });
        let client = test_client(&server, false);
        let page = search_videos(&client, VideoSearchQuery::new("clamped").with_page(999)).await?;

        assert_eq!(page.page, 4);
        assert_eq!(page.next_page, Some(5));
        assert_eq!(search.calls(), 1);
        search.delete();

        let zero_page = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/wbi/search/type");
            then.status(200)
                .json_body(search_response_with_page(0, 5, 5, 20, &[]));
        });
        assert!(matches!(
            search_videos(&client, VideoSearchQuery::new("invalid page")).await,
            Err(Error::InvalidInput(message)) if message.contains("invalid page")
        ));
        assert_eq!(zero_page.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn search_deduplicates_filters_non_video_and_handles_empty_pages() -> anyhow::Result<()> {
        let server = MockServer::start();
        let _nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body(nav_response(0));
        });
        let mut search = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/wbi/search/type");
            then.status(200).json_body(search_response(
                5,
                1,
                20,
                &[
                    video_row("BV1Q541167Qg", "first"),
                    video_row("BV1Q541167Qg", "duplicate"),
                    json!({"type":"bangumi","aid":455_017_605,"bvid":"BV1Q541167Qg","title":"pgc"}),
                    json!({"type":"ad","aid":455_017_605,"bvid":"BV1Q541167Qg","title":"ad"}),
                    json!({"type":"video","title":"no id"}),
                ],
            ));
        });
        let client = test_client(&server, false);
        let page = search_videos(&client, VideoSearchQuery::new("rust")).await?;
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].title, "first");
        assert_eq!(page.next_page, None);
        search.delete();

        let empty = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/wbi/search/type");
            then.status(200).json_body(search_response(0, 0, 20, &[]));
        });
        let page = search_videos(&client, VideoSearchQuery::new("no results")).await?;
        assert_eq!(page.items, Vec::<VideoSearchItem>::new());
        assert_eq!(page.total, 0);
        assert_eq!(page.next_page, None);
        assert_eq!(empty.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn search_reports_api_and_required_pagination_failures() {
        let server = MockServer::start();
        let _nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body(nav_response(0));
        });
        let mut api_error = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/wbi/search/type");
            then.status(200)
                .json_body(json!({"code":-502,"message":"search busy"}));
        });
        let client = test_client(&server, false);
        assert!(matches!(
            search_videos(&client, VideoSearchQuery::new("busy")).await,
            Err(Error::Api { code: -502, .. })
        ));
        api_error.delete();

        let missing_page = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/wbi/search/type");
            then.status(200).json_body(json!({
                "code":0,"data":{"numResults":0,"pagesize":20,"result":[]}
            }));
        });
        assert!(matches!(
            search_videos(&client, VideoSearchQuery::new("missing page")).await,
            Err(Error::MissingField("data.numPages"))
        ));
        assert_eq!(missing_page.calls(), 1);
    }

    #[tokio::test]
    async fn anonymous_nav_without_wbi_keys_fails_explicitly() {
        let server = MockServer::start();
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body(json!({
                "code":-101,"message":"not logged in","data":{"wbi_img":{}}
            }));
        });
        let client = test_client(&server, false);
        assert!(matches!(
            search_videos(&client, VideoSearchQuery::new("missing key")).await,
            Err(Error::MissingField("data.wbi_img.img_url"))
        ));
        assert_eq!(nav.calls(), 1);
    }

    #[tokio::test]
    async fn search_does_not_follow_nav_redirect_or_request_search() {
        let server = MockServer::start();
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(302)
                .header("Location", "/x/web-interface/nav-redirect-target")
                .json_body(nav_response(0));
        });
        let redirected = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/nav-redirect-target");
            then.status(200).json_body(nav_response(0));
        });
        let search = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/wbi/search/type");
            then.status(200).json_body(search_response(0, 0, 20, &[]));
        });
        let client = test_client(&server, false);
        assert!(matches!(
            search_videos(&client, VideoSearchQuery::new("redirect")).await,
            Err(Error::InvalidInput(message)) if message.contains("redirected")
        ));
        assert_eq!(nav.calls(), 1);
        assert_eq!(redirected.calls(), 0);
        assert_eq!(search.calls(), 0);
    }

    #[tokio::test]
    async fn generic_search_resolution_requires_selection_before_network() {
        let server = MockServer::start();
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body(nav_response(0));
        });
        let client = test_client(&server, false);
        let result = client
            .resolve(
                Input::VideoSearch {
                    keyword: "rust".to_owned(),
                    page: 1,
                    order: VideoSearchOrder::TotalRank,
                },
                None,
            )
            .await;
        assert!(matches!(
            result,
            Err(Error::SelectionRequired {
                input_kind: "video search"
            })
        ));
        assert_eq!(nav.calls(), 0);
    }

    #[tokio::test]
    async fn search_rejects_oversized_pages_and_body_limits() {
        let server = MockServer::start();
        let _nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body(nav_response(0));
        });
        let rows: Vec<Value> = std::iter::repeat_with(|| video_row("BV1Q541167Qg", "duplicate"))
            .take(21)
            .collect();
        let mut oversized_page = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/wbi/search/type");
            then.status(200)
                .json_body(search_response(21, 2, 21, &rows));
        });
        let client = test_client(&server, false);
        assert!(matches!(
            search_videos(&client, VideoSearchQuery::new("too many")).await,
            Err(Error::InvalidInput(_))
        ));
        oversized_page.delete();

        let oversized_body = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/wbi/search/type");
            then.status(200).json_body(json!({
                "code":0,"data":{"numResults":0,"numPages":0,"pagesize":20,
                "result":[],"extra":"x".repeat(1024 * 1024)}
            }));
        });
        assert!(matches!(
            search_videos(&client, VideoSearchQuery::new("too large")).await,
            Err(Error::InvalidInput(message)) if message.contains("response exceeded")
        ));
        assert_eq!(oversized_body.calls(), 1);
    }

    #[tokio::test]
    async fn selected_search_items_hydrate_only_selected_result_and_use_first_part()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let _nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body(nav_response(-101));
        });
        let _search = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/wbi/search/type");
            then.status(200).json_body(search_response(
                2,
                1,
                20,
                &[
                    video_row("BV1Q541167Qg", "first"),
                    video_row("BV1Q541167Qg", "duplicate"),
                    video_row("BV1xx411c7mD", "second"),
                ],
            ));
        });
        let first = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("bvid", "BV1Q541167Qg");
            then.status(200)
                .json_body(view_response(455_017_605, "BV1Q541167Qg"));
        });
        let second = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("bvid", "BV1xx411c7mD");
            then.status(200)
                .json_body(view_response(170_001, "BV1xx411c7mD"));
        });
        let client = test_client(&server, false);
        let selected = client
            .fetch_video_search_collection(
                VideoSearchQuery::new("rust"),
                Some(Selection::Page(2)),
                TagPolicy::Skip,
            )
            .await?;
        assert_eq!(selected.selected_items.len(), 1);
        assert_eq!(selected.selected_items[0].index, 2);
        assert_eq!(selected.selected_items[0].cid, 101);
        assert_eq!(first.calls(), 0);
        assert_eq!(second.calls(), 1);
        Ok(())
    }

    fn nav_response(code: i64) -> Value {
        json!({"code":code,"message":"nav","data":{"wbi_img":{
            "img_url":"https://i0.hdslb.com/bfs/wbi/0123456789abcdef0123456789abcdef.png",
            "sub_url":"https://i0.hdslb.com/bfs/wbi/fedcba9876543210fedcba9876543210.png"
        }}})
    }

    fn video_row(bvid: &str, title: &str) -> Value {
        json!({"type":"video","bvid":bvid,"title":title,"author":"Alice","mid":42})
    }

    fn search_response(total: u64, total_pages: u32, page_size: u32, rows: &[Value]) -> Value {
        json!({"code":0,"data":{
            "numResults":total,"numPages":total_pages,"pagesize":page_size,"result":rows
        }})
    }

    fn search_response_with_page(
        page: u32,
        total: u64,
        total_pages: u32,
        page_size: u32,
        rows: &[Value],
    ) -> Value {
        let mut response = search_response(total, total_pages, page_size, rows);
        response["data"]["page"] = json!(page);
        response
    }

    fn test_client(server: &MockServer, prefix: bool) -> BiliClient {
        let base = if prefix {
            format!("{}/prefix", server.base_url())
        } else {
            server.base_url()
        };
        let config = ClientConfig::new(
            EndpointConfig::default().with_api_base(base),
            Credentials::default().with_cookie("tracking=local-only"),
        );
        BiliClient::new(config)
    }

    fn view_response(aid: u64, bvid: &str) -> Value {
        json!({"code":0,"data":{
            "aid":aid,"bvid":bvid,"title":"Hydrated video","desc":"description",
            "pages":[
                {"page":1,"cid":101,"part":"P1","duration":12},
                {"page":2,"cid":202,"part":"P2","duration":18}
            ]
        }})
    }
}

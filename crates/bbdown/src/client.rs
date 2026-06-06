use crate::models::{
    EpisodeMetadata, Owner, PageMetadata, ResolvedContent, SeasonMetadata, SeasonResolution, Tag,
    VideoMetadata,
};
use crate::{Credentials, Error, Input, Result, Selection};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue, REFERER, USER_AGENT};
use serde::Deserialize;
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug)]
pub struct EndpointConfig {
    pub api_base: String,
    pub pgc_base: String,
    pub intl_base: String,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            api_base: "https://api.bilibili.com".to_owned(),
            pgc_base: "https://api.bilibili.com".to_owned(),
            intl_base: "https://api.bilibili.tv".to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub endpoints: EndpointConfig,
    pub credentials: Credentials,
    pub user_agent: String,
    pub request_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            endpoints: EndpointConfig::default(),
            credentials: Credentials::default(),
            user_agent: "bbdown-rs/0.1".to_owned(),
            request_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BiliClient {
    http: reqwest::Client,
    config: ClientConfig,
}

impl BiliClient {
    #[must_use]
    pub fn new(config: ClientConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    pub async fn resolve_input(
        &self,
        raw: &str,
        selection: Option<Selection>,
    ) -> Result<ResolvedContent> {
        let input = Input::parse(raw)?;
        self.resolve(input, selection).await
    }

    pub async fn resolve(
        &self,
        input: Input,
        selection: Option<Selection>,
    ) -> Result<ResolvedContent> {
        match input {
            Input::Aid(aid) => self
                .fetch_video_by_aid(aid)
                .await
                .map(ResolvedContent::Video),
            Input::Bvid(bvid) => self
                .fetch_video_by_bvid(&bvid)
                .await
                .map(ResolvedContent::Video),
            Input::Episode(epid) => self
                .fetch_season_by_ep(epid, selection.or(Some(Selection::Current)))
                .await
                .map(ResolvedContent::Season),
            Input::Season(season_id) => {
                let selection = selection.ok_or(Error::SelectionRequired {
                    input_kind: "season",
                })?;
                self.fetch_season_by_season_id(season_id, selection)
                    .await
                    .map(ResolvedContent::Season)
            }
            Input::Media(media_id) => {
                let selection = selection.ok_or(Error::SelectionRequired {
                    input_kind: "media",
                })?;
                self.fetch_season_by_media_id(media_id, selection)
                    .await
                    .map(ResolvedContent::Season)
            }
            Input::IntlEpisode(epid) => self
                .fetch_intl_season_by_ep(epid, selection.or(Some(Selection::Current)))
                .await
                .map(ResolvedContent::Season),
        }
    }

    async fn fetch_video_by_aid(&self, aid: u64) -> Result<VideoMetadata> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/web-interface/view")?;
        url.query_pairs_mut().append_pair("aid", &aid.to_string());
        self.fetch_video(url).await
    }

    async fn fetch_video_by_bvid(&self, bvid: &str) -> Result<VideoMetadata> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/web-interface/view")?;
        url.query_pairs_mut().append_pair("bvid", bvid);
        self.fetch_video(url).await
    }

    async fn fetch_video(&self, url: Url) -> Result<VideoMetadata> {
        let response: ApiData<ViewData> = self.get_json(url).await?;
        let data = response.into_data()?;
        let aid = data.aid.ok_or(Error::MissingField("data.aid"))?;
        let tags = self.fetch_tags(aid).await?;
        let pages = data
            .pages
            .into_iter()
            .map(|page| PageMetadata {
                index: page.page,
                aid,
                cid: page.cid,
                epid: None,
                title: page.part.unwrap_or_else(|| data.title.clone()),
                duration_seconds: page.duration,
            })
            .collect();

        Ok(VideoMetadata {
            aid,
            bvid: data.bvid,
            title: data.title,
            description: data.desc.unwrap_or_default(),
            cover_url: data.pic,
            pub_time: data.pubdate,
            owner: data.owner.map(|owner| Owner {
                mid: owner.mid,
                name: owner.name,
            }),
            tags,
            pages,
        })
    }

    async fn fetch_tags(&self, aid: u64) -> Result<Vec<Tag>> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/tag/archive/tags")?;
        url.query_pairs_mut().append_pair("aid", &aid.to_string());
        let response: ApiData<Vec<TagData>> = self.get_json(url).await?;
        Ok(response
            .into_data()?
            .into_iter()
            .filter_map(|tag| {
                tag.tag_id
                    .zip(tag.tag_name)
                    .map(|(id, name)| Tag { id, name })
            })
            .collect())
    }

    async fn fetch_season_by_ep(
        &self,
        epid: u64,
        selection: Option<Selection>,
    ) -> Result<SeasonResolution> {
        let mut url = Self::endpoint_url(&self.config.endpoints.pgc_base, "/pgc/view/web/season")?;
        url.query_pairs_mut()
            .append_pair("ep_id", &epid.to_string());
        let season = self.fetch_pgc_season(url).await?;
        let selection = selection.unwrap_or(Selection::Current);
        Self::resolve_season_selection(season, Some(&selection), Some(epid), "episode")
    }

    async fn fetch_season_by_season_id(
        &self,
        season_id: u64,
        selection: Selection,
    ) -> Result<SeasonResolution> {
        let mut url = Self::endpoint_url(&self.config.endpoints.pgc_base, "/pgc/view/web/season")?;
        url.query_pairs_mut()
            .append_pair("season_id", &season_id.to_string());
        let season = self.fetch_pgc_season(url).await?;
        Self::resolve_season_selection(season, Some(&selection), None, "season")
    }

    async fn fetch_season_by_media_id(
        &self,
        media_id: u64,
        selection: Selection,
    ) -> Result<SeasonResolution> {
        let mut review_url =
            Self::endpoint_url(&self.config.endpoints.pgc_base, "/pgc/review/user")?;
        review_url
            .query_pairs_mut()
            .append_pair("media_id", &media_id.to_string());
        let review: ApiResult<PgcReviewResult> = self.get_json(review_url).await?;
        let epid = review
            .into_result()?
            .media
            .and_then(|media| media.new_ep)
            .map(|episode| episode.id)
            .ok_or(Error::MissingField("result.media.new_ep.id"))?;
        let mut resolution = self.fetch_season_by_ep(epid, Some(selection)).await?;
        resolution.season.media_id = Some(media_id);
        Ok(resolution)
    }

    async fn fetch_intl_season_by_ep(
        &self,
        epid: u64,
        selection: Option<Selection>,
    ) -> Result<SeasonResolution> {
        let mut url = Self::endpoint_url(
            &self.config.endpoints.intl_base,
            "/intl/gateway/v2/ogv/view/app/season",
        )?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("ep_id", &epid.to_string())
                .append_pair("platform", "android")
                .append_pair("s_locale", "zh_SG")
                .append_pair("mobi_app", "bstar_a");
            if let Some(access_key) = self.config.credentials.access_key.as_deref() {
                query.append_pair("access_key", access_key);
            }
        }
        let response: IntlSeasonRoot = self.get_json(url).await?;
        let result = response.into_result()?;
        let season = season_from_intl(result, Some(epid));
        let selection = selection.unwrap_or(Selection::Current);
        Self::resolve_season_selection(season, Some(&selection), Some(epid), "intl episode")
    }

    async fn fetch_pgc_season(&self, url: Url) -> Result<SeasonMetadata> {
        let response: ApiResult<PgcSeasonResult> = self.get_json(url).await?;
        Ok(season_from_pgc(response.into_result()?))
    }

    fn resolve_season_selection(
        season: SeasonMetadata,
        selection: Option<&Selection>,
        current_epid: Option<u64>,
        input_kind: &'static str,
    ) -> Result<SeasonResolution> {
        let selected_episodes = match selection {
            Some(Selection::All) => season.episodes.clone(),
            Some(Selection::Latest) => season.episodes[..season.main_episode_count]
                .last()
                .cloned()
                .into_iter()
                .collect(),
            Some(Selection::Episode(epid)) => season
                .episodes
                .iter()
                .find(|episode| episode.epid == *epid)
                .cloned()
                .into_iter()
                .collect(),
            Some(Selection::Page(page)) => season
                .episodes
                .iter()
                .find(|episode| episode.index == *page)
                .cloned()
                .into_iter()
                .collect(),
            Some(Selection::Current) | None => {
                let epid = current_epid.ok_or(Error::SelectionRequired { input_kind })?;
                season
                    .episodes
                    .iter()
                    .find(|episode| episode.epid == epid)
                    .cloned()
                    .into_iter()
                    .collect()
            }
        };

        if selected_episodes.is_empty() {
            return Err(Error::MissingField("selected episode"));
        }

        Ok(SeasonResolution {
            season,
            selected_episodes,
        })
    }

    async fn get_json<T>(&self, url: Url) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&self.config.user_agent)
                .unwrap_or_else(|_| HeaderValue::from_static("bbdown-rs/0.1")),
        );
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://www.bilibili.com/"),
        );
        if let Some(cookie) = self.config.credentials.cookie.as_deref()
            && !cookie.is_empty()
        {
            let value = HeaderValue::from_str(cookie)
                .map_err(|_| Error::InvalidInput("invalid cookie header".to_owned()))?;
            headers.insert(COOKIE, value);
        }
        let response = self
            .http
            .get(url)
            .headers(headers)
            .timeout(self.config.request_timeout)
            .send()
            .await
            .map_err(Self::http_error_without_url)?;
        let response = response
            .error_for_status()
            .map_err(Self::http_error_without_url)?;
        response
            .json::<T>()
            .await
            .map_err(Self::http_error_without_url)
    }

    fn http_error_without_url(error: reqwest::Error) -> Error {
        Error::Http(error.without_url())
    }

    fn endpoint_url(base: &str, path: &str) -> Result<Url> {
        let mut url = Url::parse(base)?;
        let base_path = url.path().trim_end_matches('/');
        let suffix = path.trim_start_matches('/');
        let next_path = if base_path.is_empty() {
            format!("/{suffix}")
        } else {
            format!("{base_path}/{suffix}")
        };
        url.set_path(&next_path);
        url.set_query(None);
        url.set_fragment(None);
        Ok(url)
    }
}

fn season_from_pgc(result: PgcSeasonResult) -> SeasonMetadata {
    let PgcSeasonResult {
        season_id,
        media_id,
        title,
        season_title,
        evaluate,
        cover,
        episodes,
        section,
        areas,
        styles,
    } = result;
    let mut episodes = episodes_to_metadata(episodes, 0);
    let main_episode_count = episodes.len();
    let section_episodes = section
        .into_iter()
        .flat_map(|section| section.episodes.into_iter())
        .collect();
    episodes.extend(episodes_to_metadata(section_episodes, main_episode_count));
    SeasonMetadata {
        season_id,
        media_id,
        title: title.or(season_title).unwrap_or_default(),
        description: evaluate.unwrap_or_default(),
        cover_url: cover,
        main_episode_count,
        areas: areas.into_iter().filter_map(|area| area.name).collect(),
        tags: styles.into_iter().filter_map(|style| style.name).collect(),
        episodes,
    }
}

fn season_from_intl(result: IntlSeasonResult, current_epid: Option<u64>) -> SeasonMetadata {
    let IntlSeasonResult {
        season_id,
        media_id,
        title,
        season_title,
        evaluate,
        cover,
        episodes,
        modules,
        areas,
        styles,
    } = result;
    let mut module_episode_groups = modules
        .into_iter()
        .filter_map(|module| module.data)
        .map(|data| data.episodes)
        .filter(|episodes| !episodes.is_empty())
        .collect::<Vec<_>>();
    let module_episodes = current_epid
        .and_then(|epid| {
            module_episode_groups
                .iter()
                .find(|episodes| episodes.iter().any(|episode| episode.id == Some(epid)))
                .cloned()
        })
        .unwrap_or_else(|| module_episode_groups.drain(..).flatten().collect());
    let episodes = if episodes.is_empty() {
        module_episodes
    } else {
        episodes
    };
    let episodes = episodes_to_metadata(episodes, 0);
    let main_episode_count = episodes.len();
    SeasonMetadata {
        season_id,
        media_id,
        title: title.or(season_title).unwrap_or_default(),
        description: evaluate.unwrap_or_default(),
        cover_url: cover,
        main_episode_count,
        areas: areas.into_iter().filter_map(|area| area.name).collect(),
        tags: styles.into_iter().filter_map(|style| style.name).collect(),
        episodes,
    }
}

fn episodes_to_metadata(episodes: Vec<PgcEpisode>, start_index: usize) -> Vec<EpisodeMetadata> {
    let mut output = Vec::new();
    for episode in episodes {
        if let Some(mut episode) = episode_from_pgc(0, episode) {
            let index = start_index
                .checked_add(output.len())
                .and_then(|value| value.checked_add(1));
            if let Some(index) = index.and_then(|value| u32::try_from(value).ok()) {
                episode.index = index;
                output.push(episode);
            }
        }
    }
    output
}

fn episode_from_pgc(index: usize, episode: PgcEpisode) -> Option<EpisodeMetadata> {
    Some(EpisodeMetadata {
        index: u32::try_from(index + 1).ok()?,
        aid: episode.aid?,
        bvid: episode.bvid,
        cid: episode.cid?,
        epid: episode.id?,
        title: episode.title.unwrap_or_default(),
        long_title: episode.long_title,
        pub_time: episode.pub_time,
    })
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
struct ApiResult<T> {
    code: i64,
    #[serde(default)]
    message: String,
    result: Option<T>,
}

impl<T> ApiResult<T> {
    fn into_result(self) -> Result<T> {
        if self.code != 0 {
            return Err(Error::Api {
                code: self.code,
                message: self.message,
            });
        }
        self.result.ok_or(Error::MissingField("result"))
    }
}

#[derive(Debug, Deserialize)]
struct ViewData {
    aid: Option<u64>,
    bvid: Option<String>,
    title: String,
    desc: Option<String>,
    pic: Option<String>,
    pubdate: Option<i64>,
    owner: Option<ViewOwner>,
    #[serde(default)]
    pages: Vec<ViewPage>,
}

#[derive(Debug, Deserialize)]
struct ViewOwner {
    mid: u64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ViewPage {
    page: u32,
    cid: u64,
    part: Option<String>,
    duration: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TagData {
    tag_id: Option<u64>,
    tag_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PgcSeasonResult {
    season_id: Option<u64>,
    media_id: Option<u64>,
    title: Option<String>,
    season_title: Option<String>,
    evaluate: Option<String>,
    cover: Option<String>,
    #[serde(default)]
    episodes: Vec<PgcEpisode>,
    #[serde(default)]
    section: Vec<PgcSection>,
    #[serde(default)]
    areas: Vec<PgcName>,
    #[serde(default)]
    styles: Vec<PgcName>,
}

#[derive(Debug, Deserialize)]
struct IntlSeasonRoot {
    code: i64,
    #[serde(default)]
    message: String,
    result: Option<IntlSeasonResult>,
    data: Option<IntlSeasonResult>,
}

impl IntlSeasonRoot {
    fn into_result(self) -> Result<IntlSeasonResult> {
        if self.code != 0 {
            return Err(Error::Api {
                code: self.code,
                message: self.message,
            });
        }
        self.result
            .or(self.data)
            .ok_or(Error::MissingField("result"))
    }
}

#[derive(Debug, Deserialize)]
struct IntlSeasonResult {
    season_id: Option<u64>,
    media_id: Option<u64>,
    title: Option<String>,
    season_title: Option<String>,
    evaluate: Option<String>,
    cover: Option<String>,
    #[serde(default)]
    episodes: Vec<PgcEpisode>,
    #[serde(default)]
    modules: Vec<IntlModule>,
    #[serde(default)]
    areas: Vec<PgcName>,
    #[serde(default)]
    styles: Vec<PgcName>,
}

#[derive(Debug, Deserialize)]
struct IntlModule {
    data: Option<IntlModuleData>,
}

#[derive(Debug, Deserialize)]
struct IntlModuleData {
    #[serde(default)]
    episodes: Vec<PgcEpisode>,
}

#[derive(Clone, Debug, Deserialize)]
struct PgcEpisode {
    aid: Option<u64>,
    bvid: Option<String>,
    cid: Option<u64>,
    #[serde(alias = "episode_id", alias = "ep_id")]
    id: Option<u64>,
    title: Option<String>,
    long_title: Option<String>,
    pub_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct PgcSection {
    #[serde(default)]
    episodes: Vec<PgcEpisode>,
}

#[derive(Debug, Deserialize)]
struct PgcName {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PgcReviewResult {
    media: Option<PgcReviewMedia>,
}

#[derive(Debug, Deserialize)]
struct PgcReviewMedia {
    new_ep: Option<PgcReviewEpisode>,
}

#[derive(Debug, Deserialize)]
struct PgcReviewEpisode {
    id: u64,
}

#[cfg(test)]
mod tests {
    use super::{BiliClient, ClientConfig, EndpointConfig};
    use crate::{Credentials, Error, ResolvedContent, Selection};
    use httpmock::MockServer;
    use httpmock::prelude::*;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn resolves_video_metadata_with_tags() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "title": "Example video",
                    "desc": "Description",
                    "pic": "https://example.invalid/cover.jpg",
                    "pubdate": 1_700_000_000,
                    "owner": {"mid": 42, "name": "Uploader"},
                    "pages": [{"page": 1, "cid": 9988, "part": "P1", "duration": 123}]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/tag/archive/tags")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": [{"tag_id": 1, "tag_name": "anime"}]
            }));
        });

        let client = test_client(&server);
        let resolved = client.resolve_input("av170001", None).await?;
        match resolved {
            ResolvedContent::Video(video) => {
                assert_eq!(video.title, "Example video");
                assert_eq!(video.tags[0].name, "anime");
                assert_eq!(video.pages[0].cid, 9988);
            }
            ResolvedContent::Season(_) => return Err(anyhow::anyhow!("expected video")),
        }
        Ok(())
    }

    #[tokio::test]
    async fn video_tag_failure_is_not_silent() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "title": "Example video",
                    "pages": [{"page": 1, "cid": 9988, "part": "P1"}]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/tag/archive/tags")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": -101,
                "message": "login required"
            }));
        });

        let client = test_client(&server);
        let Err(error) = client.resolve_input("av170001", None).await else {
            return Err(anyhow::anyhow!("tag API failure should propagate"));
        };
        assert!(matches!(error, Error::Api { code: -101, .. }));
        Ok(())
    }

    #[tokio::test]
    async fn intl_access_key_is_redacted_from_http_errors() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = BiliClient::new(ClientConfig {
            endpoints: EndpointConfig {
                api_base: server.base_url(),
                pgc_base: server.base_url(),
                intl_base: server.base_url(),
            },
            credentials: Credentials {
                cookie: None,
                access_key: Some("TOKEN_SHOULD_REDACT_12345".to_owned()),
            },
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_secs(30),
        });

        let Err(error) = client
            .resolve_input("https://www.bilibili.tv/en/play/34613/341736", None)
            .await
        else {
            return Err(anyhow::anyhow!("HTTP status failure should propagate"));
        };
        let debug = format!("{error:?}");
        assert!(!debug.contains("TOKEN_SHOULD_REDACT_12345"));
        assert!(!debug.contains("access_key"));
        Ok(())
    }

    #[tokio::test]
    async fn resolves_intl_episode_from_module_episodes() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/intl/gateway/v2/ogv/view/app/season")
                .query_param("ep_id", "341736");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 34613,
                    "title": "Intl Season",
                    "modules": [{
                        "data": {
                            "episodes": [
                                {"aid": 7, "cid": 70, "id": 341_736, "title": "1", "long_title": "Start"}
                            ]
                        }
                    }],
                    "areas": [{"name": "Thailand"}],
                    "styles": [{"name": "Anime"}]
                }
            }));
        });

        let client = test_client(&server);
        let resolved = client
            .resolve_input("https://www.bilibili.tv/en/play/34613/341736", None)
            .await?;
        match resolved {
            ResolvedContent::Season(season) => {
                assert_eq!(season.season.title, "Intl Season");
                assert_eq!(season.season.episodes.len(), 1);
                assert_eq!(season.selected_episodes[0].epid, 341_736);
            }
            ResolvedContent::Video(_) => return Err(anyhow::anyhow!("expected season")),
        }
        Ok(())
    }

    #[tokio::test]
    async fn request_timeout_bounds_hung_endpoint() -> anyhow::Result<()> {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || {
            if let Ok((_stream, _address)) = listener.accept() {
                std::thread::sleep(Duration::from_millis(200));
            }
        });

        let client = BiliClient::new(ClientConfig {
            endpoints: EndpointConfig {
                api_base: format!("http://{address}"),
                pgc_base: "http://127.0.0.1:1".to_owned(),
                intl_base: "http://127.0.0.1:1".to_owned(),
            },
            credentials: Credentials::default(),
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_millis(30),
        });

        let started = Instant::now();
        let Err(error) = client.resolve_input("av170001", None).await else {
            return Err(anyhow::anyhow!("hung endpoint should time out"));
        };
        let elapsed = started.elapsed();
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("timeout test server panicked"))?;

        assert!(matches!(error, Error::Http(_)));
        assert!(elapsed < Duration::from_secs(1));
        Ok(())
    }

    #[tokio::test]
    async fn season_links_require_selection() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = test_client(&server);
        let error = client.resolve_input("ss123", None).await.err();
        assert!(matches!(
            error,
            Some(Error::SelectionRequired {
                input_kind: "season"
            })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn resolves_season_latest() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/view/web/season")
                .query_param("season_id", "123");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 123,
                    "media_id": 456,
                    "title": "A Season",
                    "evaluate": "Season desc",
                    "episodes": [
                        {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "title": "1", "long_title": "Start"},
                        {"aid": 11, "bvid": "BV1bb", "cid": 101, "id": 1001, "title": "2", "long_title": "Next"}
                    ],
                    "areas": [{"name": "Japan"}],
                    "styles": [{"name": "Anime"}]
                }
            }));
        });

        let client = test_client(&server);
        let resolved = client
            .resolve_input("ss123", Some(Selection::Latest))
            .await?;
        match resolved {
            ResolvedContent::Season(season) => {
                assert_eq!(season.season.title, "A Season");
                assert_eq!(season.selected_episodes.len(), 1);
                assert_eq!(season.selected_episodes[0].epid, 1001);
            }
            ResolvedContent::Video(_) => return Err(anyhow::anyhow!("expected season")),
        }
        Ok(())
    }

    #[tokio::test]
    async fn resolves_episode_from_section() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/view/web/season")
                .query_param("ep_id", "2000");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 123,
                    "title": "A Season",
                    "evaluate": "Season desc",
                    "episodes": [
                        {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "title": "1", "long_title": "Start"}
                    ],
                    "section": [{
                        "title": "Extras",
                        "episodes": [
                            {"aid": 12, "bvid": "BV1cc", "cid": 102, "id": 2000, "title": "SP", "long_title": "Special"}
                        ]
                    }]
                }
            }));
        });

        let client = test_client(&server);
        let resolved = client.resolve_input("ep2000", None).await?;
        match resolved {
            ResolvedContent::Season(season) => {
                assert_eq!(season.season.episodes.len(), 2);
                assert_eq!(season.selected_episodes[0].epid, 2000);
            }
            ResolvedContent::Video(_) => return Err(anyhow::anyhow!("expected season")),
        }
        Ok(())
    }

    #[tokio::test]
    async fn latest_ignores_section_extras() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/view/web/season")
                .query_param("season_id", "123");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 123,
                    "title": "A Season",
                    "episodes": [
                        {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "title": "1"},
                        {"aid": 11, "bvid": "BV1bb", "cid": 101, "id": 1001, "title": "2"}
                    ],
                    "section": [{
                        "title": "Extras",
                        "episodes": [
                            {"aid": 12, "bvid": "BV1cc", "cid": 102, "id": 2000, "title": "PV"}
                        ]
                    }]
                }
            }));
        });

        let client = test_client(&server);
        let resolved = client
            .resolve_input("ss123", Some(Selection::Latest))
            .await?;
        match resolved {
            ResolvedContent::Season(season) => {
                assert_eq!(season.season.episodes.len(), 3);
                assert_eq!(season.season.main_episode_count, 2);
                assert_eq!(season.selected_episodes[0].epid, 1001);
            }
            ResolvedContent::Video(_) => return Err(anyhow::anyhow!("expected season")),
        }
        Ok(())
    }

    #[tokio::test]
    async fn latest_uses_filtered_main_episode_count() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/view/web/season")
                .query_param("season_id", "123");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 123,
                    "title": "A Season",
                    "episodes": [
                        {"bvid": "BVbad", "cid": 99, "id": 999, "title": "invalid"},
                        {"aid": 11, "bvid": "BV1bb", "cid": 101, "id": 1001, "title": "2"}
                    ],
                    "section": [{
                        "title": "Extras",
                        "episodes": [
                            {"aid": 12, "bvid": "BV1cc", "cid": 102, "id": 2000, "title": "PV"}
                        ]
                    }]
                }
            }));
        });

        let client = test_client(&server);
        let resolved = client
            .resolve_input("ss123", Some(Selection::Latest))
            .await?;
        match resolved {
            ResolvedContent::Season(season) => {
                assert_eq!(season.season.main_episode_count, 1);
                assert_eq!(season.selected_episodes[0].epid, 1001);
            }
            ResolvedContent::Video(_) => return Err(anyhow::anyhow!("expected season")),
        }
        Ok(())
    }

    #[test]
    fn endpoint_url_preserves_path_prefix() -> anyhow::Result<()> {
        let url =
            BiliClient::endpoint_url("http://proxy.example/bili/api", "/x/web-interface/view")?;
        assert_eq!(
            url.as_str(),
            "http://proxy.example/bili/api/x/web-interface/view"
        );
        Ok(())
    }

    fn test_client(server: &MockServer) -> BiliClient {
        BiliClient::new(ClientConfig {
            endpoints: EndpointConfig {
                api_base: server.base_url(),
                pgc_base: server.base_url(),
                intl_base: server.base_url(),
            },
            credentials: Credentials::default(),
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_secs(30),
        })
    }
}

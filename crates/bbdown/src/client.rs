use crate::models::{
    DanmakuTrack, DownloadEntry, DownloadPlan, EpisodeMetadata, FlvSegment, MediaStream, Owner,
    PageMetadata, ResolvedContent, SeasonMetadata, SeasonResolution, StreamDiagnostics,
    StreamQuality, StreamResolverAttempt, StreamResolverOutcome, StreamSet, StreamSource,
    SubtitleFormat, SubtitleTrack, Tag, VideoMetadata,
};
use crate::{Credentials, Error, Input, Result, Selection};
use md5::{Digest, Md5};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue, REFERER, USER_AGENT};
use serde::Deserialize;
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

#[derive(Clone, Debug)]
pub struct EndpointConfig {
    pub api_base: String,
    pub pgc_base: String,
    pub intl_base: String,
    pub comment_base: String,
    pub passport_base: String,
    pub tv_passport_base: String,
    pub tv_passport_poll_base: String,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            api_base: "https://api.bilibili.com".to_owned(),
            pgc_base: "https://api.bilibili.com".to_owned(),
            intl_base: "https://api.bilibili.tv".to_owned(),
            comment_base: "https://comment.bilibili.com".to_owned(),
            passport_base: "https://passport.bilibili.com".to_owned(),
            tv_passport_base: "https://passport.snm0516.aisee.tv".to_owned(),
            tv_passport_poll_base: "https://passport.bilibili.com".to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub endpoints: EndpointConfig,
    pub credentials: Credentials,
    pub restricted_area: RestrictedAreaConfig,
    pub user_agent: String,
    pub request_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            endpoints: EndpointConfig::default(),
            credentials: Credentials::default(),
            restricted_area: RestrictedAreaConfig::default(),
            user_agent: "bbdown-rs/0.1".to_owned(),
            request_timeout: Duration::from_secs(30),
        }
    }
}

impl ClientConfig {
    #[must_use]
    pub fn new(endpoints: EndpointConfig, credentials: Credentials) -> Self {
        Self {
            endpoints,
            credentials,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_restricted_area(mut self, restricted_area: RestrictedAreaConfig) -> Self {
        self.restricted_area = restricted_area;
        self
    }

    #[must_use]
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    #[must_use]
    pub fn with_request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RestrictedAreaConfig {
    pub area_hint: Option<RestrictedArea>,
    pub proxies: Vec<RestrictedAreaProxy>,
}

impl RestrictedAreaConfig {
    #[must_use]
    pub fn ordered_proxies(&self) -> Vec<RestrictedAreaProxy> {
        let mut ordered = Vec::new();
        let mut priorities = self
            .proxies
            .iter()
            .map(|proxy| proxy.order_priority)
            .collect::<Vec<_>>();
        priorities.sort_unstable();
        priorities.dedup();
        for priority in priorities {
            if let Some(area_hint) = self.area_hint {
                self.push_matching(&mut ordered, |proxy| {
                    proxy.order_priority == priority && proxy.area == Some(area_hint)
                });
            }
            self.push_matching(&mut ordered, |proxy| {
                proxy.order_priority == priority && proxy.area.is_none()
            });
            for area in [
                RestrictedArea::Cn,
                RestrictedArea::Th,
                RestrictedArea::Hk,
                RestrictedArea::Tw,
            ] {
                self.push_matching(&mut ordered, |proxy| {
                    proxy.order_priority == priority && proxy.area == Some(area)
                });
            }
            self.push_matching(&mut ordered, |proxy| proxy.order_priority == priority);
        }
        ordered
    }

    fn push_matching(
        &self,
        ordered: &mut Vec<RestrictedAreaProxy>,
        mut predicate: impl FnMut(&RestrictedAreaProxy) -> bool,
    ) {
        for proxy in self.proxies.iter().filter(|proxy| predicate(proxy)) {
            if !ordered.iter().any(|candidate| candidate == proxy) {
                ordered.push(proxy.clone());
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestrictedArea {
    Cn,
    Th,
    Hk,
    Tw,
}

impl RestrictedArea {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Th => "th",
            Self::Hk => "hk",
            Self::Tw => "tw",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestrictedAreaProxyKind {
    PlayUrl,
    BilibiliApi,
}

#[derive(Clone, Eq)]
pub struct RestrictedAreaProxy {
    pub base_url: String,
    pub area: Option<RestrictedArea>,
    pub kind: RestrictedAreaProxyKind,
    order_priority: u8,
}

impl PartialEq for RestrictedAreaProxy {
    fn eq(&self, other: &Self) -> bool {
        self.base_url == other.base_url && self.area == other.area && self.kind == other.kind
    }
}

impl fmt::Debug for RestrictedAreaProxy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RestrictedAreaProxy")
            .field("base_url", &redact_url_string(&self.base_url))
            .field("area", &self.area.map(RestrictedArea::as_str))
            .field("kind", &self.kind)
            .field("order_priority", &self.order_priority)
            .finish()
    }
}

impl RestrictedAreaProxy {
    #[must_use]
    pub fn playurl(base_url: impl Into<String>, area: Option<RestrictedArea>) -> Self {
        Self {
            base_url: base_url.into(),
            area,
            kind: RestrictedAreaProxyKind::PlayUrl,
            order_priority: 0,
        }
    }

    #[must_use]
    pub fn bilibili_api(base_url: impl Into<String>, area: Option<RestrictedArea>) -> Self {
        Self {
            base_url: base_url.into(),
            area,
            kind: RestrictedAreaProxyKind::BilibiliApi,
            order_priority: 0,
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn with_order_priority(mut self, order_priority: u8) -> Self {
        self.order_priority = order_priority;
        self
    }
}

#[derive(Clone, Debug)]
pub struct BiliClient {
    pub(crate) http: reqwest::Client,
    pub(crate) config: ClientConfig,
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
                .fetch_video_by_aid(aid, TagPolicy::Fetch)
                .await
                .map(ResolvedContent::Video),
            Input::Bvid(bvid) => self
                .fetch_video_by_bvid(&bvid, TagPolicy::Fetch)
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

    pub async fn plan_download(
        &self,
        raw: &str,
        selection: Option<Selection>,
    ) -> Result<DownloadPlan> {
        let input = Input::parse(raw)?;
        self.plan(input, selection).await
    }

    pub async fn plan(&self, input: Input, selection: Option<Selection>) -> Result<DownloadPlan> {
        match input {
            Input::Aid(aid) => {
                let video = self.fetch_video_by_aid(aid, TagPolicy::Skip).await?;
                self.plan_video(video, selection).await
            }
            Input::Bvid(bvid) => {
                let video = self.fetch_video_by_bvid(&bvid, TagPolicy::Skip).await?;
                self.plan_video(video, selection).await
            }
            Input::Episode(epid) => {
                let season = self
                    .fetch_season_by_ep(epid, selection.or(Some(Selection::Current)))
                    .await?;
                self.plan_season(season, StreamSource::PgcWeb).await
            }
            Input::Season(season_id) => {
                let selection = selection.ok_or(Error::SelectionRequired {
                    input_kind: "season",
                })?;
                let season = self.fetch_season_by_season_id(season_id, selection).await?;
                self.plan_season(season, StreamSource::PgcWeb).await
            }
            Input::Media(media_id) => {
                let selection = selection.ok_or(Error::SelectionRequired {
                    input_kind: "media",
                })?;
                let season = self.fetch_season_by_media_id(media_id, selection).await?;
                self.plan_season(season, StreamSource::PgcWeb).await
            }
            Input::IntlEpisode(epid) => {
                let season = self
                    .fetch_intl_season_by_ep(epid, selection.or(Some(Selection::Current)))
                    .await?;
                self.plan_season(season, StreamSource::IntlWeb).await
            }
        }
    }

    async fn fetch_video_by_aid(&self, aid: u64, tag_policy: TagPolicy) -> Result<VideoMetadata> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/web-interface/view")?;
        url.query_pairs_mut().append_pair("aid", &aid.to_string());
        self.fetch_video(url, tag_policy).await
    }

    async fn fetch_video_by_bvid(
        &self,
        bvid: &str,
        tag_policy: TagPolicy,
    ) -> Result<VideoMetadata> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/web-interface/view")?;
        url.query_pairs_mut().append_pair("bvid", bvid);
        self.fetch_video(url, tag_policy).await
    }

    async fn fetch_video(&self, url: Url, tag_policy: TagPolicy) -> Result<VideoMetadata> {
        let response: ApiData<ViewData> = self.get_json(url).await?;
        let data = response.into_data()?;
        let aid = data.aid.ok_or(Error::MissingField("data.aid"))?;
        let tags = match tag_policy {
            TagPolicy::Fetch => self.fetch_tags(aid).await?,
            TagPolicy::Skip => Vec::new(),
        };
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
        if let Some(message) = result.access_limit_message() {
            return Err(Error::AccessRestricted(message));
        }
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

    async fn plan_video(
        &self,
        video: VideoMetadata,
        selection: Option<Selection>,
    ) -> Result<DownloadPlan> {
        let pages = Self::select_video_pages(&video, selection.as_ref())?;
        let mut entries = Vec::new();
        for page in pages {
            entries.push(
                self.plan_entry(PlanEntrySeed {
                    index: page.index,
                    aid: page.aid,
                    bvid: video.bvid.clone(),
                    cid: page.cid,
                    epid: None,
                    title: page.title,
                    source: StreamSource::NormalWeb,
                })
                .await?,
            );
        }
        Ok(DownloadPlan {
            title: video.title,
            entries,
        })
    }

    async fn plan_season(
        &self,
        season: SeasonResolution,
        source: StreamSource,
    ) -> Result<DownloadPlan> {
        let mut entries = Vec::new();
        for episode in season.selected_episodes {
            entries.push(
                self.plan_entry(PlanEntrySeed {
                    index: episode.index,
                    aid: episode.aid,
                    bvid: episode.bvid,
                    cid: episode.cid,
                    epid: Some(episode.epid),
                    title: episode_display_title(&episode.title, episode.long_title.as_deref()),
                    source: source.clone(),
                })
                .await?,
            );
        }
        Ok(DownloadPlan {
            title: season.season.title,
            entries,
        })
    }

    async fn plan_entry(&self, seed: PlanEntrySeed) -> Result<DownloadEntry> {
        let resolved_streams = self
            .fetch_stream_set(seed.source.clone(), seed.aid, seed.cid, seed.epid)
            .await?;
        let subtitles = self
            .fetch_subtitles(
                resolved_streams.source.clone(),
                seed.aid,
                seed.cid,
                seed.epid,
            )
            .await
            .unwrap_or_default();
        Ok(DownloadEntry {
            index: seed.index,
            aid: seed.aid,
            bvid: seed.bvid,
            cid: seed.cid,
            epid: seed.epid,
            title: seed.title,
            source: resolved_streams.source,
            streams: resolved_streams.streams,
            diagnostics: resolved_streams.diagnostics,
            subtitles,
            danmaku: DanmakuTrack {
                cid: seed.cid,
                xml_url: Self::endpoint_url(
                    &self.config.endpoints.comment_base,
                    &format!("/{}.xml", seed.cid),
                )?
                .to_string(),
            },
        })
    }

    fn select_video_pages(
        video: &VideoMetadata,
        selection: Option<&Selection>,
    ) -> Result<Vec<PageMetadata>> {
        let pages = match selection {
            Some(Selection::All) => video.pages.clone(),
            Some(Selection::Latest) => video.pages.last().cloned().into_iter().collect(),
            Some(Selection::Page(page)) => video
                .pages
                .iter()
                .find(|candidate| candidate.index == *page)
                .cloned()
                .into_iter()
                .collect(),
            Some(Selection::Current) | None => video.pages.first().cloned().into_iter().collect(),
            Some(Selection::Episode(_)) => {
                return Err(Error::InvalidInput(
                    "episode selection is only valid for PGC inputs".to_owned(),
                ));
            }
        };
        if pages.is_empty() {
            return Err(Error::MissingField("selected page"));
        }
        Ok(pages)
    }

    async fn fetch_stream_set(
        &self,
        source: StreamSource,
        aid: u64,
        cid: u64,
        epid: Option<u64>,
    ) -> Result<ResolvedStreamSet> {
        if source == StreamSource::PgcWeb {
            return self.fetch_pgc_stream_set(aid, cid, epid).await;
        }
        let mut url = match source {
            StreamSource::NormalWeb => {
                Self::endpoint_url(&self.config.endpoints.api_base, "/x/player/playurl")?
            }
            StreamSource::PgcWeb | StreamSource::PgcProxy => unreachable!(),
            StreamSource::IntlWeb => Self::endpoint_url(
                &self.config.endpoints.intl_base,
                "/intl/gateway/v2/ogv/playurl",
            )?,
        };
        {
            let mut query = url.query_pairs_mut();
            match source {
                StreamSource::NormalWeb => {
                    query
                        .append_pair("avid", &aid.to_string())
                        .append_pair("cid", &cid.to_string())
                        .append_pair("qn", "0")
                        .append_pair("fnval", "4048")
                        .append_pair("fnver", "0")
                        .append_pair("fourk", "1")
                        .append_pair("try_look", "1")
                        .append_pair("otype", "json");
                }
                StreamSource::PgcWeb | StreamSource::PgcProxy => unreachable!(),
                StreamSource::IntlWeb => {
                    let epid = epid.ok_or(Error::MissingField("epid"))?;
                    for (key, value) in intl_ogv_playurl_params(
                        epid,
                        cid,
                        self.config.credentials.access_key.as_deref(),
                        current_unix_timestamp(),
                    ) {
                        query.append_pair(key, &value);
                    }
                }
            }
        }
        let streams = self.fetch_playurl_stream_set(url).await?;
        Ok(ResolvedStreamSet::official(source, streams))
    }

    async fn fetch_pgc_stream_set(
        &self,
        aid: u64,
        cid: u64,
        epid: Option<u64>,
    ) -> Result<ResolvedStreamSet> {
        let epid = epid.ok_or(Error::MissingField("epid"))?;
        let official_url = Self::pgc_playurl_url(&self.config.endpoints.pgc_base, aid, cid, epid)?;
        match self.fetch_playurl_stream_set(official_url.clone()).await {
            Ok(streams) => Ok(ResolvedStreamSet::official(StreamSource::PgcWeb, streams)),
            Err(error)
                if self.config.restricted_area.proxies.is_empty()
                    || !is_restricted_area_fallback_error(&error) =>
            {
                Err(error)
            }
            Err(error) => {
                let mut attempts = vec![resolver_attempt(
                    StreamSource::PgcWeb,
                    None,
                    Some(redact_url_for_diagnostics(&official_url)),
                    StreamResolverOutcome::Failed,
                    Some(resolver_error_message(&error)),
                )];
                for proxy in self.config.restricted_area.ordered_proxies() {
                    let request_urls = match self.pgc_proxy_playurl_urls(&proxy, aid, cid, epid) {
                        Ok(urls) => urls,
                        Err(error) => {
                            attempts.push(resolver_attempt(
                                StreamSource::PgcProxy,
                                proxy.area,
                                Some(redact_url_string(&proxy.base_url)),
                                StreamResolverOutcome::Failed,
                                Some(resolver_error_message(&error)),
                            ));
                            continue;
                        }
                    };
                    for request_url in request_urls {
                        match self
                            .fetch_proxy_playurl_stream_set(request_url.clone())
                            .await
                        {
                            Ok(streams) => {
                                attempts.push(resolver_attempt(
                                    StreamSource::PgcProxy,
                                    proxy.area,
                                    Some(redact_url_for_diagnostics(&request_url)),
                                    StreamResolverOutcome::Succeeded,
                                    None,
                                ));
                                return Ok(ResolvedStreamSet {
                                    source: StreamSource::PgcProxy,
                                    streams,
                                    diagnostics: StreamDiagnostics { attempts },
                                });
                            }
                            Err(error) => attempts.push(resolver_attempt(
                                StreamSource::PgcProxy,
                                proxy.area,
                                Some(redact_url_for_diagnostics(&request_url)),
                                StreamResolverOutcome::Failed,
                                Some(resolver_error_message(&error)),
                            )),
                        }
                    }
                }
                Err(Error::AccessRestricted(format!(
                    "restricted-area resolver failed: {}",
                    summarize_resolver_attempts(&attempts)
                )))
            }
        }
    }

    fn pgc_playurl_url(base_url: &str, aid: u64, cid: u64, epid: u64) -> Result<Url> {
        let mut url = Self::endpoint_url(base_url, "/pgc/player/web/v2/playurl")?;
        append_pgc_playurl_params(&mut url, aid, cid, epid, None, None);
        Ok(url)
    }

    fn pgc_proxy_playurl_urls(
        &self,
        proxy: &RestrictedAreaProxy,
        aid: u64,
        cid: u64,
        epid: u64,
    ) -> Result<Vec<Url>> {
        let mut urls = match proxy.kind {
            RestrictedAreaProxyKind::PlayUrl => vec![Url::parse(&proxy.base_url)?],
            RestrictedAreaProxyKind::BilibiliApi => vec![
                Self::endpoint_url_preserving_query(&proxy.base_url, "/pgc/player/web/playurl")?,
                Self::endpoint_url_preserving_query(&proxy.base_url, "/pgc/player/web/v2/playurl")?,
            ],
        };
        for url in &mut urls {
            append_pgc_playurl_params(
                url,
                aid,
                cid,
                epid,
                proxy.area,
                self.config.credentials.access_key.as_deref(),
            );
        }
        Ok(urls)
    }

    async fn fetch_playurl_stream_set(&self, url: Url) -> Result<StreamSet> {
        let response: PlayUrlRoot = self.get_json(url).await?;
        response.into_stream_set()
    }

    async fn fetch_proxy_playurl_stream_set(&self, url: Url) -> Result<StreamSet> {
        let response: PlayUrlRoot = self.get_json_without_cookie(url).await?;
        response.into_stream_set()
    }

    async fn fetch_subtitles(
        &self,
        source: StreamSource,
        aid: u64,
        cid: u64,
        epid: Option<u64>,
    ) -> Result<Vec<SubtitleTrack>> {
        match source {
            StreamSource::NormalWeb | StreamSource::PgcWeb | StreamSource::PgcProxy => {
                let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/player/v2")?;
                url.query_pairs_mut()
                    .append_pair("aid", &aid.to_string())
                    .append_pair("cid", &cid.to_string());
                let response: ApiData<PlayerV2Data> = self.get_json(url).await?;
                Ok(response.into_data()?.into_subtitles())
            }
            StreamSource::IntlWeb => {
                let epid = epid.ok_or(Error::MissingField("epid"))?;
                let mut url = Self::endpoint_url(
                    &self.config.endpoints.intl_base,
                    "/intl/gateway/web/v2/subtitle",
                )?;
                {
                    let mut query = url.query_pairs_mut();
                    query
                        .append_pair("episode_id", &epid.to_string())
                        .append_pair("platform", "web")
                        .append_pair("s_locale", "en_US");
                    if let Some(access_key) = self.config.credentials.access_key.as_deref() {
                        query.append_pair("access_key", access_key);
                    }
                }
                let response: ApiData<IntlSubtitleData> = self.get_json(url).await?;
                Ok(response.into_data()?.into_subtitles())
            }
        }
    }

    async fn get_json<T>(&self, url: Url) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.get_json_with_cookie(url, true).await
    }

    async fn get_json_without_cookie<T>(&self, url: Url) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.get_json_with_cookie(url, false).await
    }

    async fn get_json_with_cookie<T>(&self, url: Url, include_cookie: bool) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let response = self
            .http
            .get(url)
            .headers(self.headers(include_cookie)?)
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

    pub(crate) fn anonymous_headers(&self) -> Result<HeaderMap> {
        self.headers(false)
    }

    pub(crate) fn media_headers(&self) -> Result<HeaderMap> {
        self.headers(false)
    }

    fn headers(&self, include_cookie: bool) -> Result<HeaderMap> {
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
        if include_cookie
            && let Some(cookie) = self.config.credentials.cookie.as_deref()
            && !cookie.is_empty()
        {
            let value = HeaderValue::from_str(cookie)
                .map_err(|_| Error::InvalidInput("invalid cookie header".to_owned()))?;
            headers.insert(COOKIE, value);
        }
        Ok(headers)
    }

    pub(crate) fn http_error_without_url(error: reqwest::Error) -> Error {
        Error::Http(error.without_url())
    }

    pub(crate) fn endpoint_url(base: &str, path: &str) -> Result<Url> {
        let mut url = Url::parse(base)?;
        set_endpoint_path(&mut url, path);
        url.set_query(None);
        url.set_fragment(None);
        Ok(url)
    }

    fn endpoint_url_preserving_query(base: &str, path: &str) -> Result<Url> {
        let mut url = Url::parse(base)?;
        set_endpoint_path(&mut url, path);
        url.set_fragment(None);
        Ok(url)
    }
}

fn set_endpoint_path(url: &mut Url, path: &str) {
    let base_path = url.path().trim_end_matches('/');
    let suffix = path.trim_start_matches('/');
    let next_path = if base_path.is_empty() {
        format!("/{suffix}")
    } else {
        format!("{base_path}/{suffix}")
    };
    url.set_path(&next_path);
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
        areas: areas.into_iter().filter_map(PgcName::into_name).collect(),
        tags: styles.into_iter().filter_map(PgcName::into_name).collect(),
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
        ..
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
                .find(|episodes| episodes.iter().any(|episode| episode.epid() == Some(epid)))
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
        areas: areas.into_iter().filter_map(PgcName::into_name).collect(),
        tags: styles.into_iter().filter_map(PgcName::into_name).collect(),
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
    let epid = episode.epid()?;
    Some(EpisodeMetadata {
        index: u32::try_from(index + 1).ok()?,
        aid: episode.aid?,
        bvid: episode.bvid,
        cid: episode.cid?,
        epid,
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
    status: Option<i64>,
    limit: Option<IntlLimit>,
    #[serde(default)]
    episodes: Vec<PgcEpisode>,
    #[serde(default)]
    modules: Vec<IntlModule>,
    #[serde(default)]
    areas: Vec<PgcName>,
    #[serde(default)]
    styles: Vec<PgcName>,
}

impl IntlSeasonResult {
    fn access_limit_message(&self) -> Option<String> {
        if self.has_episodes() {
            return None;
        }
        let content = self
            .limit
            .as_ref()
            .and_then(|limit| limit.content.as_deref())
            .map(str::trim)
            .filter(|content| !content.is_empty());
        if let Some(content) = content {
            return Some(content.to_owned());
        }
        if self.status == Some(13) {
            return Some("intl content is not available in the current region".to_owned());
        }
        None
    }

    fn has_episodes(&self) -> bool {
        !self.episodes.is_empty()
            || self.modules.iter().any(|module| {
                module
                    .data
                    .as_ref()
                    .is_some_and(|data| !data.episodes.is_empty())
            })
    }
}

#[derive(Debug, Deserialize)]
struct IntlLimit {
    content: Option<String>,
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
    id: Option<u64>,
    ep_id: Option<u64>,
    episode_id: Option<u64>,
    title: Option<String>,
    long_title: Option<String>,
    pub_time: Option<i64>,
}

impl PgcEpisode {
    fn epid(&self) -> Option<u64> {
        self.ep_id.or(self.episode_id).or(self.id)
    }
}

#[derive(Debug, Deserialize)]
struct PgcSection {
    #[serde(default)]
    episodes: Vec<PgcEpisode>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PgcName {
    Object { name: Option<String> },
    Text(String),
}

impl PgcName {
    fn into_name(self) -> Option<String> {
        match self {
            Self::Object { name } => name,
            Self::Text(name) => Some(name),
        }
        .filter(|name| !name.is_empty())
    }
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

#[derive(Clone, Debug)]
struct PlanEntrySeed {
    index: u32,
    aid: u64,
    bvid: Option<String>,
    cid: u64,
    epid: Option<u64>,
    title: String,
    source: StreamSource,
}

#[derive(Clone, Debug)]
struct ResolvedStreamSet {
    source: StreamSource,
    streams: StreamSet,
    diagnostics: StreamDiagnostics,
}

impl ResolvedStreamSet {
    fn official(source: StreamSource, streams: StreamSet) -> Self {
        Self {
            source,
            streams,
            diagnostics: StreamDiagnostics::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TagPolicy {
    Fetch,
    Skip,
}

fn episode_display_title(title: &str, long_title: Option<&str>) -> String {
    match long_title.filter(|value| !value.is_empty()) {
        Some(long_title) if title.is_empty() => long_title.to_owned(),
        Some(long_title) => format!("{title} {long_title}"),
        None => title.to_owned(),
    }
}

fn append_pgc_playurl_params(
    url: &mut Url,
    aid: u64,
    cid: u64,
    epid: u64,
    area: Option<RestrictedArea>,
    access_key: Option<&str>,
) {
    let mut query = url.query_pairs_mut();
    query
        .append_pair("avid", &aid.to_string())
        .append_pair("cid", &cid.to_string())
        .append_pair("ep_id", &epid.to_string())
        .append_pair("module", "bangumi")
        .append_pair("qn", "0")
        .append_pair("fnval", "4048")
        .append_pair("fnver", "0")
        .append_pair("fourk", "1")
        .append_pair("otype", "json");
    if let Some(area) = area {
        query.append_pair("area", area.as_str());
    }
    if let Some(access_key) = access_key.filter(|value| !value.is_empty()) {
        query.append_pair("access_key", access_key);
    }
}

fn is_restricted_area_fallback_error(error: &Error) -> bool {
    match error {
        Error::Api { message, .. } | Error::AccessRestricted(message) => {
            is_restricted_area_message(message)
        }
        _ => false,
    }
}

fn is_restricted_area_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "area restricted",
        "area limit",
        "region restricted",
        "region limit",
        "not available in your region",
        "地区限制",
        "地區限制",
        "区域限制",
        "區域限制",
        "所在地区不可观看",
        "所在地區不可觀看",
        "所在地区无法观看",
        "所在地區無法觀看",
        "所在的地区不可观看",
        "所在的地區不可觀看",
        "所在的地区无法观看",
        "所在的地區無法觀看",
        "地区不可观看",
        "地區不可觀看",
        "地区无法观看",
        "地區無法觀看",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn resolver_attempt(
    source: StreamSource,
    area: Option<RestrictedArea>,
    endpoint: Option<String>,
    outcome: StreamResolverOutcome,
    message: Option<String>,
) -> StreamResolverAttempt {
    StreamResolverAttempt {
        source,
        outcome,
        area: area.map(|area| area.as_str().to_owned()),
        endpoint,
        message,
    }
}

fn resolver_error_message(error: &Error) -> String {
    match error {
        Error::Api { code, message } => {
            format!("API code {code}: {}", sanitize_diagnostic_text(message))
        }
        Error::AccessRestricted(message) => {
            format!("access restricted: {}", sanitize_diagnostic_text(message))
        }
        Error::MissingField(field) => format!("missing field: {field}"),
        Error::Http(error) => format!(
            "HTTP error: {}",
            sanitize_diagnostic_text(&error.to_string())
        ),
        Error::Json(error) => format!(
            "JSON error: {}",
            sanitize_diagnostic_text(&error.to_string())
        ),
        Error::Url(error) => format!(
            "URL error: {}",
            sanitize_diagnostic_text(&error.to_string())
        ),
        Error::InvalidInput(message) => {
            format!("invalid input: {}", sanitize_diagnostic_text(message))
        }
        Error::SelectionRequired { input_kind } => {
            format!("{input_kind} links require an explicit selection")
        }
        Error::Unsupported(message) => {
            format!("unsupported: {}", sanitize_diagnostic_text(message))
        }
        Error::Io(error) => format!(
            "I/O error: {}",
            sanitize_diagnostic_text(&error.to_string())
        ),
        Error::MuxFailed { status } => {
            format!(
                "ffmpeg mux failed with status {}",
                sanitize_diagnostic_text(status)
            )
        }
    }
}

fn summarize_resolver_attempts(attempts: &[StreamResolverAttempt]) -> String {
    attempts
        .iter()
        .map(|attempt| {
            let area = attempt
                .area
                .as_deref()
                .map(|area| format!(" area={area}"))
                .unwrap_or_default();
            let message = attempt
                .message
                .as_deref()
                .map(|message| format!(" ({message})"))
                .unwrap_or_default();
            format!("{:?}{area} {:?}{message}", attempt.source, attempt.outcome)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn redact_url_for_diagnostics(url: &Url) -> String {
    let mut redacted = url.clone();
    let _ = redacted.set_username("");
    let _ = redacted.set_password(None);
    redacted.set_path("");
    redacted.set_query(None);
    redacted.set_fragment(None);
    redacted.to_string().trim_end_matches('/').to_owned()
}

fn redact_url_string(raw: &str) -> String {
    Url::parse(raw).map_or_else(
        |_| redact_unparsed_url_for_diagnostics(raw),
        |url| redact_url_for_diagnostics(&url),
    )
}

fn sanitize_diagnostic_text(raw: &str) -> String {
    let without_urls = redact_urls_in_text(raw);
    let redacted = redact_sensitive_key_values(&without_urls);
    let lower = redacted.to_ascii_lowercase();
    if SENSITIVE_DIAGNOSTIC_KEYS
        .iter()
        .any(|key| lower.contains(key))
    {
        "redacted diagnostic message".to_owned()
    } else {
        redacted
    }
}

const SENSITIVE_DIAGNOSTIC_KEYS: &[&str] = &[
    "access_key",
    "access_token",
    "proxy_token",
    "token",
    "jwt",
    "api_key",
    "x-api-key",
    "authorization",
    "sessdata",
    "bili_jct",
    "cookie",
];

fn redact_urls_in_text(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut index = 0;
    while let Some(relative_start) = find_next_url_start(&raw[index..]) {
        let start = index + relative_start;
        output.push_str(&raw[index..start]);
        let end = raw[start..]
            .find(is_message_url_delimiter)
            .map_or(raw.len(), |relative_end| start + relative_end);
        let token = &raw[start..end];
        let (url_token, trailing) = split_trailing_url_punctuation(token);
        match Url::parse(url_token) {
            Ok(url) => output.push_str(&redact_url_for_diagnostics(&url)),
            Err(_) => output.push_str(&redact_unparsed_url_for_diagnostics(url_token)),
        }
        output.push_str(trailing);
        index = end;
    }
    output.push_str(&raw[index..]);
    output
}

fn find_next_url_start(raw: &str) -> Option<usize> {
    let lower = raw.to_ascii_lowercase();
    match (lower.find("http://"), lower.find("https://")) {
        (Some(http), Some(https)) => Some(http.min(https)),
        (Some(http), None) => Some(http),
        (None, Some(https)) => Some(https),
        (None, None) => None,
    }
}

fn is_message_url_delimiter(character: char) -> bool {
    character.is_whitespace() || matches!(character, '"' | '\'' | '<' | '>' | '(' | ')')
}

fn split_trailing_url_punctuation(token: &str) -> (&str, &str) {
    let trimmed_len = token.trim_end_matches([',', '.', ';']).len();
    token.split_at(trimmed_len)
}

fn redact_sensitive_key_values(raw: &str) -> String {
    SENSITIVE_DIAGNOSTIC_KEYS
        .iter()
        .fold(raw.to_owned(), |value, key| {
            redact_sensitive_key_value(&value, key)
        })
}

fn redact_sensitive_key_value(raw: &str, key: &str) -> String {
    let with_equals = redact_sensitive_key_value_with_separator(raw, key, '=');
    redact_sensitive_key_value_with_separator(&with_equals, key, ':')
}

fn redact_sensitive_key_value_with_separator(raw: &str, key: &str, separator: char) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut index = 0;
    let pattern = format!("{key}{separator}");
    let lower = raw.to_ascii_lowercase();
    while let Some(relative_start) = lower[index..].find(&pattern) {
        let start = index + relative_start;
        output.push_str(&raw[index..start]);
        output.push_str("<redacted>");
        let value_start = skip_ascii_whitespace(raw, start + pattern.len());
        let value_end = raw[value_start..]
            .find(|character| is_sensitive_value_delimiter_for_key(key, character))
            .map_or(raw.len(), |relative_end| value_start + relative_end);
        index = value_end;
    }
    output.push_str(&raw[index..]);
    output
}

fn skip_ascii_whitespace(raw: &str, mut index: usize) -> usize {
    while let Some(character) = raw[index..].chars().next() {
        if !character.is_ascii_whitespace() {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn is_sensitive_value_delimiter_for_key(key: &str, character: char) -> bool {
    if key == "authorization" {
        matches!(
            character,
            '&' | '"' | '\'' | '<' | '>' | '(' | ')' | ',' | ';' | '\r' | '\n'
        )
    } else {
        is_sensitive_value_delimiter(character)
    }
}

fn is_sensitive_value_delimiter(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '&' | '"' | '\'' | '<' | '>' | '(' | ')' | ',' | ';' | '\r' | '\n'
        )
}

fn redact_basic_auth_like_string(raw: &str) -> String {
    let Some(scheme_end) = raw.find("//") else {
        return raw.to_owned();
    };
    let after_scheme = &raw[(scheme_end + 2)..];
    let Some(userinfo_end) = after_scheme.find('@') else {
        return raw.to_owned();
    };
    format!(
        "{}//<redacted>@{}",
        &raw[..scheme_end],
        &after_scheme[(userinfo_end + 1)..]
    )
    .trim_end_matches('/')
    .to_owned()
}

fn redact_unparsed_url_for_diagnostics(raw: &str) -> String {
    let basic_auth_redacted = redact_basic_auth_like_string(raw);
    let Some(scheme_end) = basic_auth_redacted.find("//") else {
        return "<invalid-url>".to_owned();
    };
    let prefix = &basic_auth_redacted[..scheme_end];
    let after_scheme = &basic_auth_redacted[(scheme_end + 2)..];
    let authority_end = after_scheme
        .find(|character: char| character.is_whitespace() || matches!(character, '/' | '?' | '#'))
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if authority.is_empty() {
        "<invalid-url>".to_owned()
    } else {
        format!("{prefix}//{authority}")
            .trim_end_matches('/')
            .to_owned()
    }
}

#[derive(Debug, Deserialize)]
struct PlayUrlRoot {
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<PlayPayload>,
    result: Option<PlayPayload>,
}

impl PlayUrlRoot {
    fn into_stream_set(self) -> Result<StreamSet> {
        if self.code != 0 {
            return Err(Error::Api {
                code: self.code,
                message: self.message,
            });
        }
        let mut payload = self
            .result
            .or(self.data)
            .ok_or(Error::MissingField("playurl result"))?;
        if let Some(video_info) = payload.video_info.take() {
            payload = *video_info;
        }
        if let Some(playurl) = payload.playurl.take() {
            return playurl.into_stream_set(payload.timelength);
        }
        let dash_duration = payload.dash.as_ref().and_then(|dash| dash.duration);
        let duration_seconds = dash_duration.or_else(|| {
            payload
                .timelength
                .and_then(|value| u32::try_from(value / 1000).ok())
        });
        let mut videos = Vec::new();
        let mut audios = Vec::new();
        if let Some(dash) = payload.dash {
            videos.extend(
                dash.video
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(DashTrack::into_media_stream),
            );
            audios.extend(
                dash.audio
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(DashTrack::into_media_stream),
            );
            if let Some(dolby) = dash.dolby {
                audios.extend(
                    dolby
                        .audio
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(DashTrack::into_media_stream),
                );
            }
            if let Some(flac) = dash.flac
                && let Some(audio) = flac.audio
                && let Some(stream) = audio.into_media_stream()
            {
                audios.push(stream);
            }
        }
        videos.extend(
            payload
                .stream_list
                .unwrap_or_default()
                .into_iter()
                .filter_map(IntlStreamItem::into_video_stream),
        );
        audios.extend(
            payload
                .dash_audio
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .filter_map(|(index, resource)| resource.into_media_stream(fallback_id(index))),
        );
        let flv_segments = payload
            .durl
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter_map(|(index, segment)| segment.into_flv_segment(index))
            .collect::<Vec<_>>();
        if videos.is_empty() && audios.is_empty() && flv_segments.is_empty() {
            return Err(Error::MissingField("playurl streams"));
        }
        Ok(StreamSet {
            qualities: stream_qualities(
                &payload.accept_quality,
                &payload.accept_description,
                &payload.support_formats,
                &videos,
            ),
            videos,
            audios,
            flv_segments,
            accept_quality: payload.accept_quality,
            duration_seconds,
        })
    }
}

#[derive(Debug, Deserialize)]
struct PlayPayload {
    video_info: Option<Box<PlayPayload>>,
    playurl: Option<IntlPlayUrlPayload>,
    dash: Option<DashPayload>,
    stream_list: Option<Vec<IntlStreamItem>>,
    dash_audio: Option<Vec<IntlMediaResource>>,
    durl: Option<Vec<DurlSegment>>,
    #[serde(default)]
    accept_quality: Vec<u32>,
    #[serde(default)]
    accept_description: Vec<String>,
    #[serde(default)]
    support_formats: Vec<SupportFormat>,
    timelength: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SupportFormat {
    quality: Option<u32>,
    new_description: Option<String>,
    display_desc: Option<String>,
    description: Option<String>,
}

impl SupportFormat {
    fn label(&self) -> Option<String> {
        first_non_empty([
            self.new_description.clone(),
            self.display_desc.clone(),
            self.description.clone(),
        ])
    }
}

#[derive(Debug, Deserialize)]
struct IntlPlayUrlPayload {
    video: Option<Vec<IntlStreamItem>>,
    audio_resource: Option<Vec<IntlMediaResource>>,
    timelength: Option<u64>,
}

impl IntlPlayUrlPayload {
    fn into_stream_set(self, fallback_timelength: Option<u64>) -> Result<StreamSet> {
        let videos = self
            .video
            .unwrap_or_default()
            .into_iter()
            .filter_map(IntlStreamItem::into_video_stream)
            .collect::<Vec<_>>();
        let audios = self
            .audio_resource
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter_map(|(index, resource)| resource.into_media_stream(fallback_id(index)))
            .collect::<Vec<_>>();
        if videos.is_empty() && audios.is_empty() {
            return Err(Error::MissingField("intl playurl streams"));
        }
        let accept_quality = videos.iter().map(|stream| stream.id).collect::<Vec<_>>();
        let duration_seconds = self
            .timelength
            .or(fallback_timelength)
            .and_then(|value| u32::try_from(value / 1000).ok());
        Ok(StreamSet {
            qualities: stream_qualities(&accept_quality, &[], &[], &videos),
            videos,
            audios,
            flv_segments: Vec::new(),
            accept_quality,
            duration_seconds,
        })
    }
}

#[derive(Debug, Deserialize)]
struct IntlStreamItem {
    stream_info: Option<IntlStreamInfo>,
    video_resource: Option<IntlMediaResource>,
    dash_video: Option<IntlMediaResource>,
}

impl IntlStreamItem {
    fn into_video_stream(self) -> Option<MediaStream> {
        let id = self.stream_info.and_then(|info| info.quality).or_else(|| {
            self.video_resource
                .as_ref()
                .or(self.dash_video.as_ref())
                .and_then(|resource| resource.id)
        })?;
        self.video_resource
            .or(self.dash_video)
            .and_then(|resource| resource.into_media_stream(id))
    }
}

#[derive(Debug, Deserialize)]
struct IntlStreamInfo {
    quality: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct IntlMediaResource {
    id: Option<u32>,
    url: Option<String>,
    base_url: Option<String>,
    #[serde(rename = "baseUrl")]
    base_url_camel: Option<String>,
    backup_url: Option<Vec<String>>,
    #[serde(rename = "backupUrl")]
    backup_url_camel: Option<Vec<String>>,
    codecs: Option<String>,
    bandwidth: Option<u64>,
    width: Option<u32>,
    height: Option<u32>,
    size: Option<u64>,
}

impl IntlMediaResource {
    fn into_media_stream(self, fallback_id: u32) -> Option<MediaStream> {
        let base_url = first_non_empty([self.url, self.base_url, self.base_url_camel])?;
        Some(MediaStream {
            id: self.id.unwrap_or(fallback_id),
            base_url: normalize_media_url(&base_url),
            backup_urls: normalize_media_urls([self.backup_url, self.backup_url_camel]),
            codecs: self.codecs,
            bandwidth: self.bandwidth,
            width: self.width,
            height: self.height,
            frame_rate: None,
            mime_type: None,
            size: self.size,
        })
    }
}

#[derive(Debug, Deserialize)]
struct DashPayload {
    duration: Option<u32>,
    video: Option<Vec<DashTrack>>,
    audio: Option<Vec<DashTrack>>,
    dolby: Option<DolbyPayload>,
    flac: Option<FlacPayload>,
}

#[derive(Debug, Deserialize)]
struct DolbyPayload {
    audio: Option<Vec<DashTrack>>,
}

#[derive(Debug, Deserialize)]
struct FlacPayload {
    audio: Option<DashTrack>,
}

#[derive(Debug, Deserialize)]
struct DashTrack {
    id: Option<u32>,
    base_url: Option<String>,
    #[serde(rename = "baseUrl")]
    base_url_camel: Option<String>,
    backup_url: Option<Vec<String>>,
    #[serde(rename = "backupUrl")]
    backup_url_camel: Option<Vec<String>>,
    codecs: Option<String>,
    bandwidth: Option<u64>,
    width: Option<u32>,
    height: Option<u32>,
    frame_rate: Option<String>,
    #[serde(rename = "frameRate")]
    frame_rate_camel: Option<String>,
    mime_type: Option<String>,
    #[serde(rename = "mimeType")]
    mime_type_camel: Option<String>,
    size: Option<u64>,
}

impl DashTrack {
    fn into_media_stream(self) -> Option<MediaStream> {
        let base_url = first_non_empty([self.base_url, self.base_url_camel])?;
        Some(MediaStream {
            id: self.id?,
            base_url: normalize_media_url(&base_url),
            backup_urls: normalize_media_urls([self.backup_url, self.backup_url_camel]),
            codecs: self.codecs,
            bandwidth: self.bandwidth,
            width: self.width,
            height: self.height,
            frame_rate: first_non_empty([self.frame_rate, self.frame_rate_camel]),
            mime_type: first_non_empty([self.mime_type, self.mime_type_camel]),
            size: self.size,
        })
    }
}

#[derive(Debug, Deserialize)]
struct DurlSegment {
    order: Option<u32>,
    url: Option<String>,
    #[serde(alias = "backupUrl")]
    backup_url: Option<Vec<String>>,
    size: Option<u64>,
    length: Option<u64>,
}

impl DurlSegment {
    fn into_flv_segment(self, index: usize) -> Option<FlvSegment> {
        let url = self.url.filter(|value| !value.is_empty())?;
        Some(FlvSegment {
            order: self
                .order
                .or_else(|| u32::try_from(index + 1).ok())
                .unwrap_or(1),
            url: normalize_media_url(&url),
            backup_urls: normalize_media_urls([self.backup_url]),
            size: self.size,
            length_ms: self.length,
        })
    }
}

#[derive(Debug, Deserialize)]
struct PlayerV2Data {
    subtitle: Option<PlayerSubtitle>,
}

impl PlayerV2Data {
    fn into_subtitles(self) -> Vec<SubtitleTrack> {
        self.subtitle
            .map(PlayerSubtitle::into_subtitles)
            .unwrap_or_default()
    }
}

#[derive(Debug, Deserialize)]
struct PlayerSubtitle {
    #[serde(default)]
    subtitles: Vec<SubtitleData>,
    #[serde(default)]
    list: Vec<SubtitleData>,
}

impl PlayerSubtitle {
    fn into_subtitles(self) -> Vec<SubtitleTrack> {
        self.subtitles
            .into_iter()
            .chain(self.list)
            .filter_map(SubtitleData::into_subtitle)
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct IntlSubtitleData {
    #[serde(default)]
    subtitles: Vec<SubtitleData>,
}

impl IntlSubtitleData {
    fn into_subtitles(self) -> Vec<SubtitleTrack> {
        self.subtitles
            .into_iter()
            .filter_map(SubtitleData::into_subtitle)
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct SubtitleData {
    #[serde(alias = "lang_key", alias = "key")]
    lan: Option<String>,
    #[serde(alias = "lang_doc")]
    lan_doc: Option<String>,
    #[serde(alias = "url")]
    subtitle_url: Option<String>,
}

impl SubtitleData {
    fn into_subtitle(self) -> Option<SubtitleTrack> {
        let url = normalize_media_url(self.subtitle_url?.trim());
        if url.is_empty() {
            return None;
        }
        Some(SubtitleTrack {
            language: self.lan.unwrap_or_else(|| "und".to_owned()),
            language_doc: self.lan_doc,
            format: subtitle_format(&url),
            url,
        })
    }
}

fn normalize_media_url(url: &str) -> String {
    if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.to_owned()
    }
}

fn first_non_empty<const N: usize>(values: [Option<String>; N]) -> Option<String> {
    values.into_iter().flatten().find(|value| !value.is_empty())
}

fn stream_qualities(
    accept_quality: &[u32],
    accept_description: &[String],
    support_formats: &[SupportFormat],
    videos: &[MediaStream],
) -> Vec<StreamQuality> {
    let mut qualities = Vec::new();
    for (index, id) in accept_quality.iter().copied().enumerate() {
        push_stream_quality(
            &mut qualities,
            id,
            support_format_label(id, support_formats).or_else(|| {
                accept_description
                    .get(index)
                    .filter(|value| !value.is_empty())
                    .cloned()
            }),
        );
    }
    for format in support_formats {
        if let Some(id) = format.quality {
            push_stream_quality(&mut qualities, id, format.label());
        }
    }
    for stream in videos {
        push_stream_quality(
            &mut qualities,
            stream.id,
            support_format_label(stream.id, support_formats),
        );
    }
    qualities
}

fn support_format_label(id: u32, support_formats: &[SupportFormat]) -> Option<String> {
    support_formats
        .iter()
        .find(|format| format.quality == Some(id))
        .and_then(SupportFormat::label)
}

fn push_stream_quality(qualities: &mut Vec<StreamQuality>, id: u32, description: Option<String>) {
    if qualities.iter().any(|quality| quality.id == id) {
        return;
    }
    qualities.push(StreamQuality { id, description });
}

fn normalize_media_urls<const N: usize>(url_groups: [Option<Vec<String>>; N]) -> Vec<String> {
    url_groups
        .into_iter()
        .flatten()
        .flatten()
        .filter(|url| !url.is_empty())
        .map(|url| normalize_media_url(&url))
        .collect()
}

fn subtitle_format(url: &str) -> SubtitleFormat {
    let path = Url::parse(url)
        .ok()
        .map_or_else(|| url.to_owned(), |url| url.path().to_owned());
    match std::path::Path::new(&path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if extension.eq_ignore_ascii_case("json") => SubtitleFormat::Json,
        Some(extension) if extension.eq_ignore_ascii_case("ass") => SubtitleFormat::Ass,
        _ => SubtitleFormat::Unknown,
    }
}

fn fallback_id(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn intl_ogv_playurl_params(
    epid: u64,
    cid: u64,
    access_key: Option<&str>,
    timestamp: u64,
) -> Vec<(&'static str, String)> {
    let mut params = Vec::new();
    if let Some(access_key) = access_key.filter(|value| !value.is_empty()) {
        params.push(("access_key", access_key.to_owned()));
    }
    params.extend([
        ("appkey", "7d089525d3611b1c".to_owned()),
        ("area", "th".to_owned()),
        ("build", "1001310".to_owned()),
        ("cid", cid.to_string()),
        ("ep_id", epid.to_string()),
        ("fnval", "4048".to_owned()),
        ("fnver", "0".to_owned()),
        ("force_host", "2".to_owned()),
        ("fourk", "1".to_owned()),
        ("mobi_app", "bstar_a".to_owned()),
        ("platform", "android".to_owned()),
        ("qn", "0".to_owned()),
        ("s_locale", "zh_SG".to_owned()),
        ("ts", timestamp.to_string()),
    ]);
    let sign = sign_ordered_params(&params, "acd495b248ec528c2eed1e862d393126");
    params.push(("sign", sign));
    params
}

pub(crate) fn sign_ordered_params(params: &[(&str, String)], secret: &str) -> String {
    let mut plaintext = String::new();
    for (index, (key, value)) in params.iter().enumerate() {
        if index > 0 {
            plaintext.push('&');
        }
        plaintext.push_str(key);
        plaintext.push('=');
        plaintext.push_str(value);
    }
    plaintext.push_str(secret);
    format!("{:x}", Md5::digest(plaintext.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        BiliClient, ClientConfig, EndpointConfig, PlayUrlRoot, RestrictedArea,
        RestrictedAreaConfig, RestrictedAreaProxy, intl_ogv_playurl_params,
    };
    use crate::{
        Credentials, Error, Input, ResolvedContent, Selection, StreamSource, SubtitleFormat,
    };
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
    async fn plan_video_ignores_tag_failure() -> anyhow::Result<()> {
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
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/player/playurl")
                .query_param("avid", "170001")
                .query_param("cid", "9988")
                .query_param("try_look", "1");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "durl": [{"url": "https://video.example/segment.flv"}]
                }
            }));
        });

        let client = test_client(&server);
        let plan = client.plan(Input::Aid(170_001), None).await?;

        assert_eq!(plan.title, "Example video");
        assert_eq!(
            plan.entries[0].streams.flv_segments[0].url,
            "https://video.example/segment.flv"
        );
        Ok(())
    }

    #[test]
    fn playurl_accepts_nullable_dash_lists_with_flv_fallback() -> anyhow::Result<()> {
        let response: PlayUrlRoot = serde_json::from_value(serde_json::json!({
            "code": 0,
            "data": {
                "dash": {
                    "duration": 9,
                    "video": null,
                    "audio": null
                },
                "durl": [{
                    "url": "https://flv.example/segment.flv",
                    "backupUrl": null
                }]
            }
        }))?;

        let streams = response.into_stream_set()?;

        assert!(streams.videos.is_empty());
        assert!(streams.audios.is_empty());
        assert_eq!(
            streams.flv_segments[0].url,
            "https://flv.example/segment.flv"
        );
        Ok(())
    }

    #[test]
    fn playurl_accepts_null_durl_with_dash_streams() -> anyhow::Result<()> {
        let response: PlayUrlRoot = serde_json::from_value(serde_json::json!({
            "code": 0,
            "data": {
                "dash": {
                    "duration": 9,
                    "video": [{
                        "id": 80,
                        "baseUrl": "https://video.example/80.m4s"
                    }],
                    "audio": [{
                        "id": 30280,
                        "baseUrl": "https://audio.example/30280.m4s"
                    }]
                },
                "durl": null
            }
        }))?;

        let streams = response.into_stream_set()?;

        assert_eq!(streams.videos[0].id, 80);
        assert_eq!(streams.audios[0].id, 30280);
        assert!(streams.flv_segments.is_empty());
        Ok(())
    }

    #[test]
    fn playurl_uses_non_empty_camel_media_fields_when_snake_fields_are_empty() -> anyhow::Result<()>
    {
        let response: PlayUrlRoot = serde_json::from_value(serde_json::json!({
            "code": 0,
            "data": {
                "dash": {
                    "duration": 9,
                    "video": [{
                        "id": 80,
                        "base_url": "",
                        "baseUrl": "//video.example/80.m4s",
                        "backup_url": [],
                        "backupUrl": ["//backup.example/80.m4s"],
                        "frame_rate": "",
                        "frameRate": "30",
                        "mime_type": "",
                        "mimeType": "video/mp4"
                    }],
                    "audio": [{
                        "id": 30280,
                        "base_url": "",
                        "baseUrl": "//audio.example/30280.m4s"
                    }]
                }
            }
        }))?;

        let streams = response.into_stream_set()?;

        assert_eq!(streams.videos[0].base_url, "https://video.example/80.m4s");
        assert_eq!(
            streams.videos[0].backup_urls,
            vec!["https://backup.example/80.m4s"]
        );
        assert_eq!(streams.videos[0].frame_rate.as_deref(), Some("30"));
        assert_eq!(streams.videos[0].mime_type.as_deref(), Some("video/mp4"));
        Ok(())
    }

    #[test]
    fn playurl_accepts_intl_mobile_video_info_shape() -> anyhow::Result<()> {
        let response: PlayUrlRoot = serde_json::from_value(serde_json::json!({
            "code": 0,
            "data": {
                "video_info": {
                    "timelength": 42_000,
                    "stream_list": [{
                        "stream_info": {"quality": 80},
                        "dash_video": {
                            "base_url": "https://intl.example/video.m4s",
                            "bandwidth": 900,
                            "codecs": "avc1",
                            "width": 1280,
                            "height": 720
                        }
                    }],
                    "dash_audio": [{
                        "id": 30280,
                        "base_url": "https://intl.example/audio.m4s",
                        "bandwidth": 128_000,
                        "codecs": "mp4a.40.2"
                    }]
                }
            }
        }))?;

        let streams = response.into_stream_set()?;

        assert_eq!(streams.duration_seconds, Some(42));
        assert_eq!(streams.videos[0].id, 80);
        assert_eq!(streams.audios[0].id, 30280);
        Ok(())
    }

    #[test]
    fn intl_ogv_playurl_params_are_signed_in_helper_order() {
        let params = intl_ogv_playurl_params(341_736, 70, Some("intl-token"), 1_234_567_890);

        assert_eq!(
            params,
            vec![
                ("access_key", "intl-token".to_owned()),
                ("appkey", "7d089525d3611b1c".to_owned()),
                ("area", "th".to_owned()),
                ("build", "1001310".to_owned()),
                ("cid", "70".to_owned()),
                ("ep_id", "341736".to_owned()),
                ("fnval", "4048".to_owned()),
                ("fnver", "0".to_owned()),
                ("force_host", "2".to_owned()),
                ("fourk", "1".to_owned()),
                ("mobi_app", "bstar_a".to_owned()),
                ("platform", "android".to_owned()),
                ("qn", "0".to_owned()),
                ("s_locale", "zh_SG".to_owned()),
                ("ts", "1234567890".to_owned()),
                ("sign", "8b320aef0eac5b957c3cab1f98bb9b6d".to_owned()),
            ]
        );
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn plans_video_download_with_streams_subtitles_and_danmaku() -> anyhow::Result<()> {
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
                "code": 0,
                "data": []
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/player/playurl")
                .query_param("avid", "170001")
                .query_param("cid", "9988")
                .query_param("try_look", "1");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "timelength": 123_000,
                    "accept_quality": [80, 64],
                    "accept_description": ["1080P", "720P"],
                    "support_formats": [
                        {"quality": 80, "new_description": "1080P 高码率"},
                        {"quality": 64, "display_desc": "720P"}
                    ],
                    "dash": {
                        "duration": 123,
                        "video": [{
                            "id": 80,
                            "baseUrl": "//video.example/80.m4s",
                            "base_url": "//video.example/80.m4s",
                            "backupUrl": ["//backup.example/80.m4s"],
                            "backup_url": ["//backup.example/80.m4s"],
                            "codecs": "avc1.640028",
                            "bandwidth": 1000,
                            "width": 1920,
                            "height": 1080,
                            "frameRate": "30",
                            "frame_rate": "30"
                        }],
                        "audio": [{
                            "id": 30280,
                            "baseUrl": "//audio.example/30280.m4s",
                            "base_url": "//audio.example/30280.m4s",
                            "codecs": "mp4a.40.2",
                            "bandwidth": 128_000
                        }],
                        "dolby": {"audio": null}
                    },
                    "durl": [{
                        "url": "//flv.example/segment.flv",
                        "backup_url": ["//flv-backup.example/segment.flv"],
                        "length": 123_000
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/player/v2")
                .query_param("aid", "170001")
                .query_param("cid", "9988");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "subtitle": {
                        "subtitles": [{
                            "lan": "zh-CN",
                            "lan_doc": "中文（简体）",
                            "subtitle_url": "//subtitle.example/zh.json"
                        }]
                    }
                }
            }));
        });

        let client = test_client(&server);
        let plan = client.plan_download("av170001", None).await?;

        assert_eq!(plan.title, "Example video");
        assert_eq!(plan.entries.len(), 1);
        let entry = &plan.entries[0];
        assert_eq!(entry.source, StreamSource::NormalWeb);
        assert_eq!(
            entry.streams.videos[0].base_url,
            "https://video.example/80.m4s"
        );
        assert_eq!(
            entry.streams.videos[0].backup_urls[0],
            "https://backup.example/80.m4s"
        );
        assert_eq!(entry.streams.audios[0].id, 30280);
        assert_eq!(
            entry.streams.audios[0].base_url,
            "https://audio.example/30280.m4s"
        );
        assert_eq!(entry.streams.flv_segments[0].order, 1);
        assert_eq!(
            entry.streams.flv_segments[0].url,
            "https://flv.example/segment.flv"
        );
        assert_eq!(
            entry.streams.flv_segments[0].backup_urls[0],
            "https://flv-backup.example/segment.flv"
        );
        assert_eq!(entry.streams.duration_seconds, Some(123));
        assert_eq!(entry.streams.qualities[0].id, 80);
        assert_eq!(
            entry.streams.qualities[0].description.as_deref(),
            Some("1080P 高码率")
        );
        assert_eq!(entry.streams.qualities[1].id, 64);
        assert_eq!(
            entry.streams.qualities[1].description.as_deref(),
            Some("720P")
        );
        assert_eq!(entry.subtitles[0].url, "https://subtitle.example/zh.json");
        assert_eq!(
            entry.danmaku.xml_url,
            format!("{}/9988.xml", server.base_url())
        );
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
                comment_base: server.base_url(),
                passport_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            credentials: Credentials {
                cookie: None,
                access_key: Some("TOKEN_SHOULD_REDACT_12345".to_owned()),
                tv_access_key: None,
            },
            restricted_area: RestrictedAreaConfig::default(),
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
                                {"aid": 7, "cid": 70, "episode_id": 341_736, "title": "1", "long_title": "Start"}
                            ]
                        }
                    }, {
                        "data": {
                            "episodes": [
                                {"aid": 8, "cid": 80, "id": 341_737, "title": "2", "long_title": "Wrong module"}
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
            .resolve_input(
                "https://www.bilibili.tv/en/play/34613/341736",
                Some(Selection::Latest),
            )
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
    async fn intl_region_limit_returns_access_restricted_error() -> anyhow::Result<()> {
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
                    "status": 13,
                    "limit": {
                        "content": "Sorry, this content is not available in your region"
                    },
                    "modules": []
                }
            }));
        });

        let client = test_client(&server);
        let Err(error) = client
            .resolve_input("https://www.bilibili.tv/en/play/34613/341736", None)
            .await
        else {
            return Err(anyhow::anyhow!("region-limited intl season should fail"));
        };

        match error {
            Error::AccessRestricted(message) => {
                assert!(message.contains("not available in your region"));
            }
            other => {
                return Err(anyhow::anyhow!(
                    "expected access restriction, got {other:?}"
                ));
            }
        }
        Ok(())
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn plans_intl_download_with_access_key_for_subtitles() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/intl/gateway/v2/ogv/view/app/season")
                .query_param("ep_id", "341736")
                .query_param("access_key", "intl-token");
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
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/intl/gateway/v2/ogv/playurl")
                .query_param("ep_id", "341736")
                .query_param("cid", "70")
                .query_param("platform", "android")
                .query_param("mobi_app", "bstar_a")
                .query_param("area", "th")
                .query_param("s_locale", "zh_SG")
                .query_param("access_key", "intl-token")
                .query_param_exists("sign");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "video_info": {
                        "timelength": 42_000,
                        "stream_list": [{
                            "stream_info": {"quality": 80},
                            "dash_video": {
                                "base_url": "https://intl.example/video.m4s",
                                "width": 1280,
                                "height": 720,
                                "bandwidth": 900,
                                "codecs": "avc1"
                            }
                        }],
                        "dash_audio": [{
                            "id": 30280,
                            "base_url": "https://intl.example/audio.m4s",
                            "bandwidth": 128_000,
                            "codecs": "mp4a.40.2"
                        }]
                    }
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/intl/gateway/web/v2/subtitle")
                .query_param("episode_id", "341736")
                .query_param("platform", "web")
                .query_param("s_locale", "en_US")
                .query_param("access_key", "intl-token");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "subtitles": [{
                        "lang_key": "en",
                        "lang_doc": "English",
                        "url": "https://subtitle.example/en.ass"
                    }]
                }
            }));
        });

        let client = BiliClient::new(ClientConfig {
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
                cookie: None,
                access_key: Some("intl-token".to_owned()),
                tv_access_key: None,
            },
            restricted_area: RestrictedAreaConfig::default(),
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_secs(30),
        });
        let plan = client
            .plan_download("https://www.bilibili.tv/en/play/34613/341736", None)
            .await?;

        assert_eq!(plan.entries[0].source, StreamSource::IntlWeb);
        assert_eq!(plan.entries[0].subtitles[0].language, "en");
        assert_eq!(plan.entries[0].subtitles[0].format, SubtitleFormat::Ass);
        Ok(())
    }

    #[tokio::test]
    async fn plans_pgc_download_from_video_info_payload() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/view/web/season")
                .query_param("ep_id", "1000");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 123,
                    "title": "A Season",
                    "episodes": [
                        {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "ep_id": 1000, "title": "1", "long_title": "Start"}
                    ]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/player/web/v2/playurl")
                .query_param("avid", "10")
                .query_param("cid", "100")
                .query_param("ep_id", "1000")
                .query_param("module", "bangumi");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "video_info": {
                        "dash": {
                            "duration": 456,
                            "video": [{
                            "id": 64,
                            "baseUrl": "https://video.example/64.m4s",
                            "base_url": "https://video.example/64.m4s",
                            "backupUrl": [],
                            "backup_url": [],
                            "codecs": "hev1",
                            "bandwidth": 900,
                            "mimeType": "video/mp4",
                            "mime_type": "video/mp4"
                        }],
                            "audio": []
                        }
                    }
                }
            }));
        });

        let client = test_client(&server);
        let plan = client.plan_download("ep1000", None).await?;

        assert_eq!(plan.entries.len(), 1);
        let entry = &plan.entries[0];
        assert_eq!(entry.source, StreamSource::PgcWeb);
        assert_eq!(entry.epid, Some(1000));
        assert_eq!(entry.title, "1 Start");
        assert_eq!(entry.streams.videos[0].id, 64);
        assert_eq!(
            entry.streams.videos[0].mime_type.as_deref(),
            Some("video/mp4")
        );
        Ok(())
    }

    #[test]
    fn restricted_area_proxies_are_ordered_by_hint_then_generic_then_area() {
        let config = RestrictedAreaConfig {
            area_hint: Some(RestrictedArea::Hk),
            proxies: vec![
                RestrictedAreaProxy::playurl("https://generic.example/playurl", None),
                RestrictedAreaProxy::bilibili_api(
                    "https://tw.example/api",
                    Some(RestrictedArea::Tw),
                ),
                RestrictedAreaProxy::bilibili_api(
                    "https://hk.example/api",
                    Some(RestrictedArea::Hk),
                ),
            ],
        };

        let ordered = config.ordered_proxies();
        assert_eq!(ordered[0].area, Some(RestrictedArea::Hk));
        assert_eq!(ordered[1].area, None);
        assert_eq!(ordered[2].area, Some(RestrictedArea::Tw));
    }

    #[test]
    fn restricted_area_message_accepts_common_chinese_unavailable_phrasing() {
        for message in [
            "您所在地区不可观看",
            "所在地区无法观看",
            "您所在地區不可觀看",
            "您所在的地区无法观看",
            "您所在的地區無法觀看",
        ] {
            assert!(super::is_restricted_area_message(message));
        }
    }

    #[tokio::test]
    async fn pgc_streams_fall_back_to_restricted_area_proxy() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/view/web/season")
                .query_param("ep_id", "1000");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 123,
                    "title": "A Season",
                    "episodes": [
                        {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "ep_id": 1000, "title": "1", "long_title": "Start"}
                    ]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/player/web/v2/playurl")
                .query_param("ep_id", "1000");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": -40301,
                "message": "您所在地区不可观看"
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/t/PATH_SECRET/proxy-playurl")
                .query_param("proxy_token", "PROXY_SECRET")
                .query_param("ep_id", "1000")
                .query_param("area", "hk")
                .query_param("access_key", "ACCESS_SECRET")
                .header_missing("cookie");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "video_info": {
                        "dash": {
                            "duration": 456,
                            "video": [{
                                "id": 64,
                                "baseUrl": "https://proxy.example/64.m4s",
                                "base_url": "https://proxy.example/64.m4s"
                            }],
                            "audio": []
                        }
                    }
                }
            }));
        });

        let client = BiliClient::new(ClientConfig {
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
                cookie: Some("SESSDATA=COOKIE_SECRET".to_owned()),
                access_key: Some("ACCESS_SECRET".to_owned()),
                tv_access_key: None,
            },
            restricted_area: RestrictedAreaConfig {
                area_hint: Some(RestrictedArea::Hk),
                proxies: vec![RestrictedAreaProxy::playurl(
                    format!(
                        "{}/t/PATH_SECRET/proxy-playurl?proxy_token=PROXY_SECRET",
                        server.base_url()
                    ),
                    Some(RestrictedArea::Hk),
                )],
            },
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_secs(30),
        });
        let plan = client.plan_download("ep1000", None).await?;

        let entry = &plan.entries[0];
        assert_eq!(entry.source, StreamSource::PgcProxy);
        assert_eq!(
            entry.streams.videos[0].base_url,
            "https://proxy.example/64.m4s"
        );
        assert_eq!(entry.diagnostics.attempts.len(), 2);
        assert_eq!(entry.diagnostics.attempts[0].source, StreamSource::PgcWeb);
        assert_eq!(entry.diagnostics.attempts[1].area.as_deref(), Some("hk"));
        let diagnostics = serde_json::to_string(&entry.diagnostics)?;
        assert!(!diagnostics.contains("ACCESS_SECRET"));
        assert!(!diagnostics.contains("COOKIE_SECRET"));
        assert!(!diagnostics.contains("PATH_SECRET"));
        assert!(!diagnostics.contains("PROXY_SECRET"));
        assert!(!diagnostics.contains("access_key"));
        Ok(())
    }

    #[tokio::test]
    async fn pgc_proxy_invalid_candidate_diagnostics_redact_endpoint() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/view/web/season")
                .query_param("ep_id", "1000");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 123,
                    "title": "A Season",
                    "episodes": [
                        {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "ep_id": 1000, "title": "1"}
                    ]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/player/web/v2/playurl")
                .query_param("ep_id", "1000");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": -40301,
                "message": "area restricted"
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy-playurl")
                .query_param("ep_id", "1000")
                .header_missing("cookie");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "video_info": {
                        "dash": {
                            "video": [{
                                "id": 64,
                                "baseUrl": "https://proxy.example/64.m4s",
                                "base_url": "https://proxy.example/64.m4s"
                            }],
                            "audio": []
                        }
                    }
                }
            }));
        });

        let client = BiliClient::new(ClientConfig {
            endpoints: EndpointConfig {
                api_base: server.base_url(),
                pgc_base: server.base_url(),
                intl_base: server.base_url(),
                comment_base: server.base_url(),
                passport_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            credentials: Credentials::default(),
            restricted_area: RestrictedAreaConfig {
                area_hint: Some(RestrictedArea::Hk),
                proxies: vec![
                    RestrictedAreaProxy::playurl(
                        "https://user:pass@proxy.example:bad/t/PATH_SECRET?proxy_token=PROXY_SECRET",
                        Some(RestrictedArea::Hk),
                    ),
                    RestrictedAreaProxy::playurl(
                        format!("{}/proxy-playurl", server.base_url()),
                        Some(RestrictedArea::Hk),
                    ),
                ],
            },
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_secs(30),
        });

        let plan = client.plan_download("ep1000", None).await?;
        let diagnostics = serde_json::to_string(&plan.entries[0].diagnostics)?;

        assert!(diagnostics.contains("https://proxy.example:bad"));
        for sensitive in ["user:pass", "PATH_SECRET", "PROXY_SECRET", "proxy_token"] {
            assert!(
                !diagnostics.contains(sensitive),
                "diagnostics leaked {sensitive}: {diagnostics}"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn pgc_proxy_fallback_requires_area_restriction() -> anyhow::Result<()> {
        for code in [403_i64, -40301_i64] {
            let server = MockServer::start();
            server.mock(|when, then| {
                when.method(GET)
                    .path("/pgc/view/web/season")
                    .query_param("ep_id", "1000");
                then.status(200).json_body_obj(&serde_json::json!({
                    "code": 0,
                    "result": {
                        "season_id": 123,
                        "title": "A Season",
                        "episodes": [
                            {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "ep_id": 1000, "title": "1"}
                        ]
                    }
                }));
            });
            server.mock(|when, then| {
                when.method(GET)
                    .path("/pgc/player/web/v2/playurl")
                    .query_param("ep_id", "1000");
                then.status(200).json_body_obj(&serde_json::json!({
                    "code": code,
                    "message": "vip required"
                }));
            });
            server.mock(|when, then| {
                when.method(GET).path("/proxy-playurl");
                then.status(200).json_body_obj(&serde_json::json!({
                    "code": 0,
                    "result": {
                        "video_info": {
                            "dash": {
                                "video": [{
                                    "id": 64,
                                    "baseUrl": "https://proxy.example/64.m4s",
                                    "base_url": "https://proxy.example/64.m4s"
                                }],
                                "audio": []
                            }
                        }
                    }
                }));
            });

            let client = BiliClient::new(ClientConfig {
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
                    cookie: None,
                    access_key: Some("ACCESS_SECRET".to_owned()),
                    tv_access_key: None,
                },
                restricted_area: RestrictedAreaConfig {
                    area_hint: Some(RestrictedArea::Hk),
                    proxies: vec![RestrictedAreaProxy::playurl(
                        format!("{}/proxy-playurl", server.base_url()),
                        Some(RestrictedArea::Hk),
                    )],
                },
                user_agent: "test".to_owned(),
                request_timeout: Duration::from_secs(30),
            });

            let Err(error) = client.plan_download("ep1000", None).await else {
                return Err(anyhow::anyhow!("non-area PGC error should not use proxy"));
            };
            assert!(matches!(error, Error::Api { code: error_code, .. } if error_code == code));
        }
        Ok(())
    }

    #[tokio::test]
    async fn pgc_restricted_area_failure_redacts_sensitive_messages() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/view/web/season")
                .query_param("ep_id", "1000");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 123,
                    "title": "A Season",
                    "episodes": [
                        {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "ep_id": 1000, "title": "1"}
                    ]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/player/web/v2/playurl")
                .query_param("ep_id", "1000");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": -40301,
                "message": "area restricted access_key=OFFICIAL_SECRET"
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy-playurl")
                .query_param("ep_id", "1000")
                .query_param("access_key", "ACCESS_SECRET")
                .header_missing("cookie");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": -40301,
                "message": "proxy rejected https://user:pass@proxy.example/api?proxy_token=PROXY_SECRET&access_key=ACCESS_SECRET cookie=SESSDATA=COOKIE_SECRET"
            }));
        });

        let client = BiliClient::new(ClientConfig {
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
                cookie: Some("SESSDATA=COOKIE_SECRET".to_owned()),
                access_key: Some("ACCESS_SECRET".to_owned()),
                tv_access_key: None,
            },
            restricted_area: RestrictedAreaConfig {
                area_hint: Some(RestrictedArea::Hk),
                proxies: vec![RestrictedAreaProxy::playurl(
                    format!("{}/proxy-playurl", server.base_url()),
                    None,
                )],
            },
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_secs(30),
        });

        let Err(error) = client.plan_download("ep1000", None).await else {
            return Err(anyhow::anyhow!("all resolver candidates should fail"));
        };
        let message = error.to_string();
        assert!(message.contains("restricted-area resolver failed"));
        for sensitive in [
            "OFFICIAL_SECRET",
            "ACCESS_SECRET",
            "PROXY_SECRET",
            "COOKIE_SECRET",
            "access_key",
            "proxy_token",
            "cookie",
            "user:pass",
        ] {
            assert!(
                !message.contains(sensitive),
                "message leaked {sensitive}: {message}"
            );
        }
        Ok(())
    }

    #[test]
    fn pgc_bilibili_api_proxy_preserves_base_query_for_web_and_v2_paths() -> anyhow::Result<()> {
        let client = BiliClient::new(ClientConfig {
            credentials: Credentials {
                cookie: None,
                access_key: Some("ACCESS_SECRET".to_owned()),
                tv_access_key: None,
            },
            ..ClientConfig::default()
        });
        let proxy = RestrictedAreaProxy::bilibili_api(
            "https://proxy.example/base?proxy_token=a%3Db",
            Some(RestrictedArea::Hk),
        );

        let urls = client.pgc_proxy_playurl_urls(&proxy, 10, 100, 1000)?;
        let actual = urls.iter().map(url::Url::as_str).collect::<Vec<_>>();
        assert_eq!(
            actual,
            vec![
                "https://proxy.example/base/pgc/player/web/playurl?proxy_token=a%3Db&avid=10&cid=100&ep_id=1000&module=bangumi&qn=0&fnval=4048&fnver=0&fourk=1&otype=json&area=hk&access_key=ACCESS_SECRET",
                "https://proxy.example/base/pgc/player/web/v2/playurl?proxy_token=a%3Db&avid=10&cid=100&ep_id=1000&module=bangumi&qn=0&fnval=4048&fnver=0&fourk=1&otype=json&area=hk&access_key=ACCESS_SECRET"
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn pgc_bilibili_api_proxy_falls_back_to_v2_path() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/view/web/season")
                .query_param("ep_id", "1000");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "season_id": 123,
                    "title": "A Season",
                    "episodes": [
                        {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "ep_id": 1000, "title": "1"}
                    ]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/player/web/v2/playurl")
                .query_param("ep_id", "1000")
                .query_param("module", "bangumi");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": -40301,
                "message": "area restricted"
            }));
        });
        let web_proxy = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/pgc/player/web/playurl")
                .query_param("ep_id", "1000")
                .query_param("area", "hk");
            then.status(404);
        });
        let v2_proxy = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/pgc/player/web/v2/playurl")
                .query_param("ep_id", "1000")
                .query_param("area", "hk");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "result": {
                    "video_info": {
                        "dash": {
                            "duration": 3,
                            "video": [{
                                "id": 80,
                                "baseUrl": "https://proxy.example/video.m4s",
                                "base_url": "https://proxy.example/video.m4s"
                            }],
                            "audio": []
                        }
                    }
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/player/v2")
                .query_param("aid", "10")
                .query_param("cid", "100");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"subtitle": {"subtitles": []}}
            }));
        });

        let client = BiliClient::new(ClientConfig {
            endpoints: EndpointConfig {
                api_base: server.base_url(),
                pgc_base: server.base_url(),
                intl_base: server.base_url(),
                comment_base: server.base_url(),
                passport_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            restricted_area: RestrictedAreaConfig {
                area_hint: Some(RestrictedArea::Hk),
                proxies: vec![RestrictedAreaProxy::bilibili_api(
                    format!("{}/proxy", server.base_url()),
                    Some(RestrictedArea::Hk),
                )],
            },
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_secs(30),
            ..ClientConfig::default()
        });
        let plan = client.plan_download("ep1000", None).await?;

        web_proxy.assert();
        v2_proxy.assert();
        assert_eq!(plan.entries[0].source, StreamSource::PgcProxy);
        assert_eq!(plan.entries[0].streams.videos[0].id, 80);
        assert_eq!(plan.entries[0].diagnostics.attempts.len(), 3);
        Ok(())
    }

    #[test]
    fn resolver_error_message_redacts_sensitive_values() {
        let message = super::resolver_error_message(&Error::Api {
            code: -40301,
            message: "proxy rejected https://user:pass@proxy.example/api?proxy_token=PROXY_SECRET&access_key=ACCESS_SECRET cookie=SESSDATA=COOKIE_SECRET token=TOKEN_SECRET jwt=JWT_SECRET x-api-key: API_KEY_SECRET authorization: Bearer AUTH_SECRET".to_owned(),
        });

        assert!(message.starts_with("API code -40301:"));
        for sensitive in [
            "ACCESS_SECRET",
            "PROXY_SECRET",
            "COOKIE_SECRET",
            "TOKEN_SECRET",
            "JWT_SECRET",
            "API_KEY_SECRET",
            "AUTH_SECRET",
            "proxy_token",
            "access_key",
            "x-api-key",
            "authorization",
            "cookie",
            "user:pass",
        ] {
            assert!(
                !message.contains(sensitive),
                "message leaked {sensitive}: {message}"
            );
        }
    }

    #[test]
    fn resolver_error_message_redacts_mixed_case_urls() {
        let message = super::resolver_error_message(&Error::Api {
            code: -40301,
            message: "proxy rejected HtTpS://user:pass@proxy.example/t/PATH_SECRET?x=1".to_owned(),
        });

        assert!(message.contains("https://proxy.example"));
        for sensitive in ["user:pass", "PATH_SECRET", "?x=1"] {
            assert!(
                !message.contains(sensitive),
                "message leaked {sensitive}: {message}"
            );
        }
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
                comment_base: "http://127.0.0.1:1".to_owned(),
                passport_base: "http://127.0.0.1:1".to_owned(),
                tv_passport_base: "http://127.0.0.1:1".to_owned(),
                tv_passport_poll_base: "http://127.0.0.1:1".to_owned(),
            },
            credentials: Credentials::default(),
            restricted_area: RestrictedAreaConfig::default(),
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
                        {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 9000, "ep_id": 1000, "title": "1", "long_title": "Start"},
                        {"aid": 11, "bvid": "BV1bb", "cid": 101, "id": 9001, "ep_id": 1001, "title": "2", "long_title": "Next"}
                    ],
                    "areas": [{"name": "Japan"}],
                    "styles": ["Anime", "Action"]
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
                assert_eq!(season.season.tags, ["Anime", "Action"]);
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
                            {"aid": 12, "bvid": "BV1cc", "cid": 102, "episode_id": 2000, "title": "SP", "long_title": "Special"}
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
                comment_base: server.base_url(),
                passport_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            credentials: Credentials::default(),
            restricted_area: RestrictedAreaConfig::default(),
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_secs(30),
        })
    }
}

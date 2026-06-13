use crate::app_playurl;
use crate::download::DownloadMode;
use crate::models::{
    DanmakuTrack, DownloadEntry, DownloadPlan, EpisodeMetadata, FlvSegment, MediaStream, Owner,
    PageMetadata, ResolvedContent, SeasonMetadata, SeasonResolution, StreamDiagnostics,
    StreamQuality, StreamResolverAttempt, StreamResolverOutcome, StreamSet, StreamSource,
    SubtitleFormat, SubtitleTrack, Tag, VideoCollectionItem, VideoCollectionKind,
    VideoCollectionMetadata, VideoCollectionResolution, VideoMetadata,
};
use crate::playback::{PlaybackPlan, header_specs_from_map};
use crate::{Credentials, Error, IndexSelection, Input, Result, Selection};
use http_body_util::BodyExt as _;
use md5::{Digest, Md5};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue, REFERER, USER_AGENT};
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

const TV_PLAYURL_APPKEY: &str = "4409e2ce8ffd12b8";
const TV_PLAYURL_APP_SECRET: &str = "59b43e04ad6965f34319062b478f83dd";

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct EndpointConfig {
    pub api_base: String,
    pub pgc_base: String,
    pub intl_base: String,
    pub comment_base: String,
    pub passport_base: String,
    pub tv_api_base: String,
    pub app_grpc_base: String,
    pub app_pgc_grpc_base: String,
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
            tv_api_base: "https://api.snm0516.aisee.tv".to_owned(),
            app_grpc_base: "https://grpc.biliapi.net".to_owned(),
            app_pgc_grpc_base: "https://app.bilibili.com".to_owned(),
            tv_passport_base: "https://passport.snm0516.aisee.tv".to_owned(),
            tv_passport_poll_base: "https://passport.bilibili.com".to_owned(),
        }
    }
}

impl EndpointConfig {
    #[must_use]
    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = api_base.into();
        self
    }

    #[must_use]
    pub fn with_pgc_base(mut self, pgc_base: impl Into<String>) -> Self {
        self.pgc_base = pgc_base.into();
        self
    }

    #[must_use]
    pub fn with_intl_base(mut self, intl_base: impl Into<String>) -> Self {
        self.intl_base = intl_base.into();
        self
    }

    #[must_use]
    pub fn with_comment_base(mut self, comment_base: impl Into<String>) -> Self {
        self.comment_base = comment_base.into();
        self
    }

    #[must_use]
    pub fn with_passport_base(mut self, passport_base: impl Into<String>) -> Self {
        self.passport_base = passport_base.into();
        self
    }

    #[must_use]
    pub fn with_tv_api_base(mut self, tv_api_base: impl Into<String>) -> Self {
        self.tv_api_base = tv_api_base.into();
        self
    }

    #[must_use]
    pub fn with_app_grpc_base(mut self, app_grpc_base: impl Into<String>) -> Self {
        self.app_grpc_base = app_grpc_base.into();
        self
    }

    #[must_use]
    pub fn with_app_pgc_grpc_base(mut self, app_pgc_grpc_base: impl Into<String>) -> Self {
        self.app_pgc_grpc_base = app_pgc_grpc_base.into();
        self
    }

    #[must_use]
    pub fn with_tv_passport_base(mut self, tv_passport_base: impl Into<String>) -> Self {
        self.tv_passport_base = tv_passport_base.into();
        self
    }

    #[must_use]
    pub fn with_tv_passport_poll_base(mut self, tv_passport_poll_base: impl Into<String>) -> Self {
        self.tv_passport_poll_base = tv_passport_poll_base.into();
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlayurlMode {
    #[default]
    Web,
    Tv,
    App,
}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub endpoints: EndpointConfig,
    pub credentials: Credentials,
    pub restricted_area: RestrictedAreaConfig,
    pub playurl_mode: PlayurlMode,
    pub user_agent: String,
    pub request_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            endpoints: EndpointConfig::default(),
            credentials: Credentials::default(),
            restricted_area: RestrictedAreaConfig::default(),
            playurl_mode: PlayurlMode::default(),
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
    pub fn with_endpoints(mut self, endpoints: EndpointConfig) -> Self {
        self.endpoints = endpoints;
        self
    }

    #[must_use]
    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = credentials;
        self
    }

    #[must_use]
    pub fn with_restricted_area(mut self, restricted_area: RestrictedAreaConfig) -> Self {
        self.restricted_area = restricted_area;
        self
    }

    #[must_use]
    pub fn with_playurl_mode(mut self, playurl_mode: PlayurlMode) -> Self {
        self.playurl_mode = playurl_mode;
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

#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RestrictedAreaConfig {
    pub area_hint: Option<RestrictedArea>,
    pub proxies: Vec<RestrictedAreaProxy>,
}

impl RestrictedAreaConfig {
    #[must_use]
    pub fn new(
        area_hint: Option<RestrictedArea>,
        proxies: impl IntoIterator<Item = RestrictedAreaProxy>,
    ) -> Self {
        Self {
            area_hint,
            proxies: proxies.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn with_area_hint(mut self, area_hint: RestrictedArea) -> Self {
        self.area_hint = Some(area_hint);
        self
    }

    #[must_use]
    pub fn with_proxy(mut self, proxy: RestrictedAreaProxy) -> Self {
        self.proxies.push(proxy);
        self
    }

    #[must_use]
    pub fn with_proxies(mut self, proxies: impl IntoIterator<Item = RestrictedAreaProxy>) -> Self {
        self.proxies.extend(proxies);
        self
    }

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
        let input = self.parse_input(raw).await?;
        self.resolve(input, selection).await
    }

    pub async fn resolve(
        &self,
        input: Input,
        selection: Option<Selection>,
    ) -> Result<ResolvedContent> {
        match input {
            Input::Aid(aid) => self.resolve_video_by_aid(aid, selection.as_ref()).await,
            Input::Bvid(bvid) => self.resolve_video_by_bvid(&bvid, selection.as_ref()).await,
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
            Input::CheeseEpisode(epid) => self
                .fetch_pugv_season_by_ep(epid, selection.or(Some(Selection::Current)))
                .await
                .map(ResolvedContent::Season),
            Input::CheeseSeason(season_id) => {
                let selection = selection.ok_or(Error::SelectionRequired {
                    input_kind: "cheese season",
                })?;
                self.fetch_pugv_season_by_season_id(season_id, selection)
                    .await
                    .map(ResolvedContent::Season)
            }
            Input::SpaceVideos(mid) => {
                let fetch_mode = Self::collection_info_fetch_mode(selection.as_ref())?;
                self.fetch_space_video_collection(mid, selection, fetch_mode)
                    .await
                    .map(ResolvedContent::Collection)
            }
            Input::FavoriteList {
                media_id,
                owner_mid,
            } => {
                let fetch_mode = Self::collection_info_fetch_mode(selection.as_ref())?;
                self.fetch_favorite_collection(media_id, owner_mid, selection, fetch_mode)
                    .await
                    .map(ResolvedContent::Collection)
            }
            Input::CollectionList(list_id) => {
                let fetch_mode = Self::collection_info_fetch_mode(selection.as_ref())?;
                self.fetch_medialist_collection(
                    list_id,
                    MediaListKind::Collection,
                    selection,
                    fetch_mode,
                )
                .await
                .map(ResolvedContent::Collection)
            }
            Input::SeriesList(list_id) => {
                let fetch_mode = Self::collection_info_fetch_mode(selection.as_ref())?;
                self.fetch_medialist_collection(
                    list_id,
                    MediaListKind::Series,
                    selection,
                    fetch_mode,
                )
                .await
                .map(ResolvedContent::Collection)
            }
            Input::SpaceCollectionList { list_id, owner_mid } => {
                self.resolve_space_list(owner_mid, list_id, SpaceListKind::Collection, selection)
                    .await
            }
            Input::SpaceSeriesList { list_id, owner_mid } => {
                self.resolve_space_list(owner_mid, list_id, SpaceListKind::Series, selection)
                    .await
            }
            Input::IntlEpisode(epid) => self
                .fetch_intl_season_by_ep(epid, selection.or(Some(Selection::Current)))
                .await
                .map(ResolvedContent::Season),
            Input::ShortLink(raw) => {
                let input = self.resolve_short_link_input(&raw).await?;
                Box::pin(self.resolve(input, selection)).await
            }
        }
    }

    pub async fn plan_download(
        &self,
        raw: &str,
        selection: Option<Selection>,
    ) -> Result<DownloadPlan> {
        let input = self.parse_input(raw).await?;
        self.plan(input, selection).await
    }

    pub async fn plan_download_with_mode(
        &self,
        raw: &str,
        selection: Option<Selection>,
        mode: DownloadMode,
    ) -> Result<DownloadPlan> {
        let input = self.parse_input(raw).await?;
        self.plan_with_download_mode(input, selection, mode).await
    }

    pub async fn plan(&self, input: Input, selection: Option<Selection>) -> Result<DownloadPlan> {
        self.plan_with_mode(input, selection, PlanningMode::Full)
            .await
    }

    pub async fn plan_with_download_mode(
        &self,
        input: Input,
        selection: Option<Selection>,
        mode: DownloadMode,
    ) -> Result<DownloadPlan> {
        self.plan_with_mode(input, selection, PlanningMode::from_download_mode(mode))
            .await
    }

    pub async fn plan_playback(
        &self,
        raw: &str,
        selection: Option<Selection>,
    ) -> Result<PlaybackPlan> {
        let input = self.parse_input(raw).await?;
        self.plan_playback_input(input, selection).await
    }

    pub async fn plan_playback_input(
        &self,
        input: Input,
        selection: Option<Selection>,
    ) -> Result<PlaybackPlan> {
        let plan = self
            .plan_with_mode(input, selection, PlanningMode::StreamsOnly)
            .await?;
        let headers = header_specs_from_map(&self.media_headers()?)?;
        Ok(PlaybackPlan::from_download_plan(&plan, &headers))
    }

    pub(crate) async fn plan_for_download(
        &self,
        input: Input,
        selection: Option<Selection>,
        mode: DownloadMode,
    ) -> Result<DownloadPlan> {
        self.plan_with_download_mode(input, selection, mode).await
    }

    async fn plan_with_mode(
        &self,
        input: Input,
        selection: Option<Selection>,
        planning_mode: PlanningMode,
    ) -> Result<DownloadPlan> {
        match input {
            Input::Aid(aid) => {
                let video = self.fetch_video_by_aid(aid, TagPolicy::Skip).await?;
                self.plan_video(video, selection, planning_mode).await
            }
            Input::Bvid(bvid) => {
                let video = self.fetch_video_by_bvid(&bvid, TagPolicy::Skip).await?;
                self.plan_video(video, selection, planning_mode).await
            }
            Input::Episode(epid) => {
                let season = self
                    .fetch_season_by_ep(epid, selection.or(Some(Selection::Current)))
                    .await?;
                self.plan_season(season, self.pgc_stream_source(), planning_mode)
                    .await
            }
            Input::Season(season_id) => {
                let selection = selection.ok_or(Error::SelectionRequired {
                    input_kind: "season",
                })?;
                let season = self.fetch_season_by_season_id(season_id, selection).await?;
                self.plan_season(season, self.pgc_stream_source(), planning_mode)
                    .await
            }
            Input::Media(media_id) => {
                let selection = selection.ok_or(Error::SelectionRequired {
                    input_kind: "media",
                })?;
                let season = self.fetch_season_by_media_id(media_id, selection).await?;
                self.plan_season(season, self.pgc_stream_source(), planning_mode)
                    .await
            }
            Input::CheeseEpisode(epid) => {
                let season = self
                    .fetch_pugv_season_by_ep(epid, selection.or(Some(Selection::Current)))
                    .await?;
                self.plan_season(season, StreamSource::PugvWeb, planning_mode)
                    .await
            }
            Input::CheeseSeason(season_id) => {
                let selection = selection.ok_or(Error::SelectionRequired {
                    input_kind: "cheese season",
                })?;
                let season = self
                    .fetch_pugv_season_by_season_id(season_id, selection)
                    .await?;
                self.plan_season(season, StreamSource::PugvWeb, planning_mode)
                    .await
            }
            collection_input @ (Input::SpaceVideos(_)
            | Input::FavoriteList { .. }
            | Input::CollectionList(_)
            | Input::SeriesList(_)
            | Input::SpaceCollectionList { .. }
            | Input::SpaceSeriesList { .. }) => {
                self.plan_collection_input(collection_input, selection, planning_mode)
                    .await
            }
            Input::IntlEpisode(epid) => {
                let season = self
                    .fetch_intl_season_by_ep(epid, selection.or(Some(Selection::Current)))
                    .await?;
                self.plan_season(season, StreamSource::IntlWeb, planning_mode)
                    .await
            }
            Input::ShortLink(raw) => {
                let input = self.resolve_short_link_input(&raw).await?;
                Box::pin(self.plan_with_mode(input, selection, planning_mode)).await
            }
        }
    }

    async fn resolve_video_by_aid(
        &self,
        aid: u64,
        selection: Option<&Selection>,
    ) -> Result<ResolvedContent> {
        let video = self.fetch_video_by_aid(aid, TagPolicy::Fetch).await?;
        Ok(ResolvedContent::Video(Self::select_video_metadata(
            video, selection,
        )?))
    }

    async fn resolve_video_by_bvid(
        &self,
        bvid: &str,
        selection: Option<&Selection>,
    ) -> Result<ResolvedContent> {
        let video = self.fetch_video_by_bvid(bvid, TagPolicy::Fetch).await?;
        Ok(ResolvedContent::Video(Self::select_video_metadata(
            video, selection,
        )?))
    }

    async fn resolve_space_list(
        &self,
        owner_mid: u64,
        list_id: u64,
        kind: SpaceListKind,
        selection: Option<Selection>,
    ) -> Result<ResolvedContent> {
        let fetch_mode = Self::collection_info_fetch_mode(selection.as_ref())?;
        self.fetch_space_list_collection(owner_mid, list_id, kind, selection, fetch_mode)
            .await
            .map(ResolvedContent::Collection)
    }

    async fn plan_collection_input(
        &self,
        input: Input,
        selection: Option<Selection>,
        planning_mode: PlanningMode,
    ) -> Result<DownloadPlan> {
        match input {
            Input::SpaceVideos(mid) => {
                let fetch_mode = Self::collection_fetch_mode(selection.as_ref())?;
                let collection = self
                    .fetch_space_video_collection(mid, selection, fetch_mode)
                    .await?;
                self.plan_collection(collection, planning_mode).await
            }
            Input::FavoriteList {
                media_id,
                owner_mid,
            } => {
                let fetch_mode = Self::collection_fetch_mode(selection.as_ref())?;
                let collection = self
                    .fetch_favorite_collection(media_id, owner_mid, selection, fetch_mode)
                    .await?;
                self.plan_collection(collection, planning_mode).await
            }
            Input::CollectionList(list_id) => {
                self.plan_medialist(list_id, MediaListKind::Collection, selection, planning_mode)
                    .await
            }
            Input::SeriesList(list_id) => {
                self.plan_medialist(list_id, MediaListKind::Series, selection, planning_mode)
                    .await
            }
            Input::SpaceCollectionList { list_id, owner_mid } => {
                self.plan_space_list(
                    owner_mid,
                    list_id,
                    SpaceListKind::Collection,
                    selection,
                    planning_mode,
                )
                .await
            }
            Input::SpaceSeriesList { list_id, owner_mid } => {
                self.plan_space_list(
                    owner_mid,
                    list_id,
                    SpaceListKind::Series,
                    selection,
                    planning_mode,
                )
                .await
            }
            _ => unreachable!("non-collection input routed to collection planner"),
        }
    }

    async fn plan_medialist(
        &self,
        list_id: u64,
        kind: MediaListKind,
        selection: Option<Selection>,
        planning_mode: PlanningMode,
    ) -> Result<DownloadPlan> {
        let fetch_mode = Self::collection_fetch_mode(selection.as_ref())?;
        let collection = self
            .fetch_medialist_collection(list_id, kind, selection, fetch_mode)
            .await?;
        self.plan_collection(collection, planning_mode).await
    }

    async fn plan_space_list(
        &self,
        owner_mid: u64,
        list_id: u64,
        kind: SpaceListKind,
        selection: Option<Selection>,
        planning_mode: PlanningMode,
    ) -> Result<DownloadPlan> {
        let fetch_mode = Self::collection_fetch_mode(selection.as_ref())?;
        let collection = self
            .fetch_space_list_collection(owner_mid, list_id, kind, selection, fetch_mode)
            .await?;
        self.plan_collection(collection, planning_mode).await
    }

    async fn parse_input(&self, raw: &str) -> Result<Input> {
        match Input::parse(raw)? {
            Input::ShortLink(short_link) => self.resolve_short_link_input(&short_link).await,
            input => Ok(input),
        }
    }

    async fn resolve_short_link_input(&self, raw: &str) -> Result<Input> {
        let url = Url::parse(raw)?;
        let response = self
            .http
            .get(url)
            .headers(self.anonymous_headers()?)
            .timeout(self.config.request_timeout)
            .send()
            .await
            .map_err(Self::http_error_without_url)?;
        let final_url = response.url().clone();
        if final_url.as_str() == raw {
            return Err(Error::InvalidInput(
                "short link did not redirect to a supported Bilibili URL".to_owned(),
            ));
        }
        match Input::parse(final_url.as_str())? {
            Input::ShortLink(_) => Err(Error::InvalidInput(
                "short link redirected to another short link".to_owned(),
            )),
            input => Ok(input),
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

    async fn fetch_pugv_season_by_ep(
        &self,
        epid: u64,
        selection: Option<Selection>,
    ) -> Result<SeasonResolution> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/pugv/view/web/season")?;
        url.query_pairs_mut()
            .append_pair("ep_id", &epid.to_string());
        let selection = selection.unwrap_or(Selection::Current);
        let season = self.fetch_pugv_season_from_beginning(url).await?;
        Self::resolve_season_selection(season, Some(&selection), Some(epid), "cheese episode")
    }

    async fn fetch_pugv_season_by_season_id(
        &self,
        season_id: u64,
        selection: Selection,
    ) -> Result<SeasonResolution> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/pugv/view/web/season")?;
        url.query_pairs_mut()
            .append_pair("season_id", &season_id.to_string());
        let season = self.fetch_pugv_season(url).await?;
        Self::resolve_season_selection(season, Some(&selection), None, "cheese season")
    }

    async fn fetch_pugv_season(&self, url: Url) -> Result<SeasonMetadata> {
        let mut data = self.fetch_pugv_season_data(url.clone()).await?;
        self.fetch_remaining_pugv_episodes(&mut data).await?;
        Ok(season_from_pugv(data))
    }

    async fn fetch_pugv_season_from_beginning(&self, url: Url) -> Result<SeasonMetadata> {
        let data = self.fetch_pugv_season_data(url.clone()).await?;
        let page_number = data
            .episode_page
            .and_then(|episode_page| episode_page.num)
            .unwrap_or(1);
        if page_number <= 1 {
            let mut data = data;
            self.fetch_remaining_pugv_episodes(&mut data).await?;
            return Ok(season_from_pugv(data));
        }

        let start_url = if let Some(season_id) = data.season_id {
            let mut url =
                Self::endpoint_url(&self.config.endpoints.api_base, "/pugv/view/web/season")?;
            url.query_pairs_mut()
                .append_pair("season_id", &season_id.to_string());
            url
        } else {
            url
        };
        let mut data = self.fetch_pugv_season_data(start_url.clone()).await?;
        let page_number = data
            .episode_page
            .and_then(|episode_page| episode_page.num)
            .unwrap_or(1);
        if page_number > 1 {
            return Err(Error::MissingField("data.episode_page first page"));
        }
        self.fetch_remaining_pugv_episodes(&mut data).await?;
        Ok(season_from_pugv(data))
    }

    async fn fetch_pugv_season_data(&self, url: Url) -> Result<PugvSeasonData> {
        let response: ApiData<PugvSeasonData> = self.get_json(url).await?;
        response.into_data()
    }

    async fn fetch_remaining_pugv_episodes(&self, data: &mut PugvSeasonData) -> Result<()> {
        let Some(mut page) = data.episode_page else {
            return Ok(());
        };
        let season_id = data
            .season_id
            .ok_or(Error::MissingField("data.season_id"))?;
        let page_size = pugv_episode_page_size(&page, data.episodes.len());
        let mut last_page_number = page.num.unwrap_or(1);
        while pugv_page_has_next(&page, page_size, last_page_number) {
            let Some(next_page_number) = last_page_number.checked_add(1) else {
                return Err(Error::MissingField("data.episode_page.num"));
            };
            if let Some(total_pages) = page.total.map(|total| total.div_ceil(page_size))
                && next_page_number > total_pages
            {
                break;
            }
            let next_data = self
                .fetch_pugv_episode_list_page(season_id, next_page_number, page_size)
                .await?;
            let returned_page_number = next_data
                .page
                .as_ref()
                .and_then(|episode_page| episode_page.num)
                .unwrap_or(next_page_number);
            if returned_page_number <= last_page_number {
                return Err(Error::MissingField("data.episode_page pagination cursor"));
            }
            last_page_number = returned_page_number;
            data.episodes.extend(next_data.items);
            let Some(next_page) = next_data.page else {
                break;
            };
            page = next_page;
        }
        Ok(())
    }

    async fn fetch_pugv_episode_list_page(
        &self,
        season_id: u64,
        page_number: u32,
        page_size: u32,
    ) -> Result<PugvEpisodeListData> {
        let mut url =
            Self::endpoint_url(&self.config.endpoints.api_base, "/pugv/view/web/ep/list")?;
        url.query_pairs_mut()
            .append_pair("season_id", &season_id.to_string())
            .append_pair("pn", &page_number.to_string())
            .append_pair("ps", &page_size.to_string());
        let response: ApiData<PugvEpisodeListData> = self.get_json(url).await?;
        response.into_data()
    }

    async fn fetch_favorite_collection(
        &self,
        media_id: Option<u64>,
        owner_mid: Option<u64>,
        selection: Option<Selection>,
        fetch_mode: CollectionFetchMode,
    ) -> Result<VideoCollectionResolution> {
        let media_id = match media_id {
            Some(media_id) => media_id,
            None => {
                self.fetch_default_favorite_id(owner_mid.ok_or(Error::MissingField("space mid"))?)
                    .await?
            }
        };
        let collection = self
            .fetch_favorite_collection_all(media_id, fetch_mode)
            .await?;
        Self::resolve_collection_selection(collection, selection.as_ref())
    }

    async fn fetch_default_favorite_id(&self, owner_mid: u64) -> Result<u64> {
        let mut url = Self::endpoint_url(
            &self.config.endpoints.api_base,
            "/x/v3/fav/folder/created/list-all",
        )?;
        url.query_pairs_mut()
            .append_pair("up_mid", &owner_mid.to_string());
        let response: ApiData<FavoriteFolderListData> = self.get_json(url).await?;
        response
            .into_data()?
            .list
            .into_iter()
            .find_map(|folder| folder.id)
            .ok_or(Error::MissingField("data.list[0].id"))
    }

    async fn fetch_favorite_collection_all(
        &self,
        media_id: u64,
        fetch_mode: CollectionFetchMode,
    ) -> Result<VideoCollectionMetadata> {
        let page_size = 20_u32;
        let mut page_number = 1_u32;
        let first = self
            .fetch_favorite_collection_page(media_id, page_number, page_size)
            .await?;
        let info = first.info;
        let total_count = info.media_count.unwrap_or(first.medias.len());
        let total_pages = total_count.div_ceil(page_size as usize);
        let mut page_medias = first.medias;
        let mut items = Vec::new();
        loop {
            for media in page_medias {
                self.push_favorite_media_items(&mut items, media).await?;
                if fetch_mode.is_satisfied_by(items.len()) {
                    break;
                }
            }
            if fetch_mode.is_satisfied_by(items.len()) || (page_number as usize) >= total_pages {
                break;
            }
            page_number += 1;
            page_medias = self
                .fetch_favorite_collection_page(media_id, page_number, page_size)
                .await?
                .medias;
        }
        renumber_collection_items(&mut items);
        Ok(VideoCollectionMetadata {
            id: Some(media_id),
            kind: VideoCollectionKind::Favorite,
            title: info.title.unwrap_or_else(|| format!("Favorite {media_id}")),
            description: info.intro.unwrap_or_default(),
            cover_url: info.cover,
            pub_time: info.ctime,
            owner: info.upper.and_then(FavoriteUpper::into_owner),
            items,
        })
    }

    async fn fetch_favorite_collection_page(
        &self,
        media_id: u64,
        page_number: u32,
        page_size: u32,
    ) -> Result<FavoriteResourceListData> {
        let mut url =
            Self::endpoint_url(&self.config.endpoints.api_base, "/x/v3/fav/resource/list")?;
        url.query_pairs_mut()
            .append_pair("media_id", &media_id.to_string())
            .append_pair("pn", &page_number.to_string())
            .append_pair("ps", &page_size.to_string())
            .append_pair("order", "mtime")
            .append_pair("type", "0")
            .append_pair("tid", "0")
            .append_pair("platform", "web");
        let response: ApiData<FavoriteResourceListData> = self.get_json(url).await?;
        response.into_data()
    }

    async fn push_favorite_media_items(
        &self,
        items: &mut Vec<VideoCollectionItem>,
        media: FavoriteMedia,
    ) -> Result<()> {
        if media.media_type.is_some_and(|media_type| media_type != 2) {
            return Ok(());
        }
        if media.attr.is_some_and(|attr| attr != 0) {
            return Ok(());
        }
        let aid = media.id.ok_or(Error::MissingField("data.medias[].id"))?;
        let first_cid = media.ugc.as_ref().and_then(|ugc| ugc.first_cid);
        if media.page.unwrap_or(1) > 1 || first_cid.is_none() {
            let video = self.fetch_video_by_aid(aid, TagPolicy::Skip).await?;
            let page_count = media.page.unwrap_or_else(|| {
                u32::try_from(video.pages.len())
                    .ok()
                    .filter(|count| *count > 0)
                    .unwrap_or(1)
            });
            for page in video.pages {
                push_unique_collection_item(
                    items,
                    VideoCollectionItem {
                        index: 0,
                        aid: page.aid,
                        bvid: video.bvid.clone().or_else(|| media.bvid.clone()),
                        cid: page.cid,
                        title: if page_count == 1 {
                            media.title.clone().unwrap_or_else(|| video.title.clone())
                        } else {
                            format_collection_page_title(
                                media.title.as_deref().unwrap_or(&video.title),
                                page.index,
                                &page.title,
                            )
                        },
                        cover_url: video.cover_url.clone().or_else(|| media.cover.clone()),
                        description: media
                            .intro
                            .clone()
                            .unwrap_or_else(|| video.description.clone()),
                        pub_time: media.pubtime.or(video.pub_time),
                        owner: media
                            .upper
                            .clone()
                            .and_then(FavoriteUpper::into_owner)
                            .or(video.owner.clone()),
                        duration_seconds: page.duration_seconds,
                    },
                );
            }
        } else {
            let cid = first_cid.ok_or(Error::MissingField("data.medias[].ugc.first_cid"))?;
            push_unique_collection_item(
                items,
                VideoCollectionItem {
                    index: 0,
                    aid,
                    bvid: media.bvid,
                    cid,
                    title: media.title.unwrap_or_else(|| aid.to_string()),
                    cover_url: media.cover,
                    description: media.intro.unwrap_or_default(),
                    pub_time: media.pubtime,
                    owner: media.upper.and_then(FavoriteUpper::into_owner),
                    duration_seconds: media.duration,
                },
            );
        }
        Ok(())
    }

    async fn fetch_medialist_collection(
        &self,
        list_id: u64,
        kind: MediaListKind,
        selection: Option<Selection>,
        fetch_mode: CollectionFetchMode,
    ) -> Result<VideoCollectionResolution> {
        let collection = self
            .fetch_medialist_collection_all(list_id, kind, fetch_mode)
            .await?;
        Self::resolve_collection_selection(collection, selection.as_ref())
    }

    async fn fetch_medialist_collection_all(
        &self,
        list_id: u64,
        kind: MediaListKind,
        fetch_mode: CollectionFetchMode,
    ) -> Result<VideoCollectionMetadata> {
        let info = self.fetch_medialist_info(list_id, kind).await?;
        let mut items = Vec::new();
        let mut oid = None;
        loop {
            let page = self.fetch_medialist_page(list_id, kind, oid).await?;
            let has_more = page.has_more.unwrap_or(false);
            let mut next_oid = oid;
            let mut selected_item_found = false;
            for media in page.media_list {
                if media.id.is_some() {
                    next_oid = media.id;
                }
                if media.attr.is_some_and(|attr| attr != 0) {
                    continue;
                }
                self.push_medialist_media_items(&mut items, media).await?;
                if fetch_mode.is_satisfied_by(items.len()) {
                    selected_item_found = true;
                    break;
                }
            }
            if selected_item_found || !has_more {
                break;
            }
            if next_oid == oid {
                return Err(Error::MissingField("data.media_list pagination cursor"));
            }
            oid = next_oid;
        }
        renumber_collection_items(&mut items);
        Ok(VideoCollectionMetadata {
            id: Some(list_id),
            kind: kind.into_collection_kind(),
            title: info
                .title
                .unwrap_or_else(|| format!("{} {list_id}", kind.title())),
            description: info.intro.unwrap_or_default(),
            cover_url: info.cover,
            pub_time: info.ctime,
            owner: None,
            items,
        })
    }

    async fn fetch_medialist_info(
        &self,
        list_id: u64,
        kind: MediaListKind,
    ) -> Result<MediaListInfoData> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/v1/medialist/info")?;
        url.query_pairs_mut()
            .append_pair("type", kind.type_id())
            .append_pair("biz_id", &list_id.to_string())
            .append_pair("tid", "0");
        let response: ApiData<MediaListInfoData> = self.get_json(url).await?;
        response.into_data()
    }

    async fn fetch_medialist_page(
        &self,
        list_id: u64,
        kind: MediaListKind,
        oid: Option<u64>,
    ) -> Result<MediaListResourcePageData> {
        let mut url = Self::endpoint_url(
            &self.config.endpoints.api_base,
            "/x/v2/medialist/resource/list",
        )?;
        url.query_pairs_mut()
            .append_pair("type", kind.type_id())
            .append_pair(
                "oid",
                &oid.map_or_else(String::new, |value| value.to_string()),
            )
            .append_pair("otype", "2")
            .append_pair("biz_id", &list_id.to_string())
            .append_pair("with_current", if oid.is_some() { "false" } else { "true" })
            .append_pair("mobi_app", "web")
            .append_pair("ps", "20")
            .append_pair("direction", "false")
            .append_pair("sort_field", "1")
            .append_pair("tid", "0")
            .append_pair("desc", kind.desc_value());
        if kind == MediaListKind::Series {
            url.query_pairs_mut().append_pair("bvid", "");
        }
        let response: ApiData<MediaListResourcePageData> = self.get_json(url).await?;
        response.into_data()
    }

    async fn fetch_space_list_collection(
        &self,
        owner_mid: u64,
        list_id: u64,
        kind: SpaceListKind,
        selection: Option<Selection>,
        fetch_mode: CollectionFetchMode,
    ) -> Result<VideoCollectionResolution> {
        let newest_first = matches!(selection, Some(Selection::Latest));
        let collection = self
            .fetch_space_list_collection_all(owner_mid, list_id, kind, fetch_mode, newest_first)
            .await?;
        Self::resolve_collection_selection(collection, selection.as_ref())
    }

    async fn fetch_space_list_collection_all(
        &self,
        owner_mid: u64,
        list_id: u64,
        kind: SpaceListKind,
        fetch_mode: CollectionFetchMode,
        newest_first: bool,
    ) -> Result<VideoCollectionMetadata> {
        let page_size = 30_u32;
        let mut page_number = 1_u32;
        let first = self
            .fetch_space_archive_page(
                owner_mid,
                list_id,
                kind,
                page_number,
                page_size,
                newest_first,
            )
            .await?;
        let meta = match kind {
            SpaceListKind::Collection => first.meta,
            SpaceListKind::Series => self.fetch_space_series_meta(list_id).await?.meta,
        };
        let total_count = first
            .page
            .total
            .or_else(|| meta.as_ref().and_then(|meta| meta.total))
            .unwrap_or(first.archives.len());
        let total_pages = total_count.div_ceil(page_size as usize);
        let mut archives = first.archives;
        let mut items = Vec::new();
        loop {
            for archive in archives {
                self.push_space_archive_items(&mut items, archive).await?;
                if fetch_mode.is_satisfied_by(items.len()) {
                    break;
                }
            }
            if fetch_mode.is_satisfied_by(items.len()) || (page_number as usize) >= total_pages {
                break;
            }
            page_number += 1;
            let next_data = self
                .fetch_space_archive_page(
                    owner_mid,
                    list_id,
                    kind,
                    page_number,
                    page_size,
                    newest_first,
                )
                .await?;
            archives = next_data.archives;
        }
        renumber_collection_items(&mut items);
        let title = meta
            .as_ref()
            .and_then(SpaceArchiveMeta::title)
            .unwrap_or_else(|| format!("{} {list_id}", kind.title()));
        Ok(VideoCollectionMetadata {
            id: Some(list_id),
            kind: kind.into_collection_kind(),
            title,
            description: meta
                .as_ref()
                .and_then(|meta| meta.description.clone())
                .unwrap_or_default(),
            cover_url: meta.as_ref().and_then(|meta| meta.cover.clone()),
            pub_time: meta.as_ref().and_then(SpaceArchiveMeta::pub_time),
            owner: Some(Owner {
                mid: meta.as_ref().and_then(|meta| meta.mid).unwrap_or(owner_mid),
                name: String::new(),
            }),
            items,
        })
    }

    async fn fetch_space_archive_page(
        &self,
        owner_mid: u64,
        list_id: u64,
        kind: SpaceListKind,
        page_number: u32,
        page_size: u32,
        newest_first: bool,
    ) -> Result<SpaceArchiveListData> {
        let path = match kind {
            SpaceListKind::Collection => "/x/polymer/web-space/seasons_archives_list",
            SpaceListKind::Series => "/x/series/archives",
        };
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, path)?;
        match kind {
            SpaceListKind::Collection => {
                url.query_pairs_mut()
                    .append_pair("mid", &owner_mid.to_string())
                    .append_pair("season_id", &list_id.to_string())
                    .append_pair("sort_reverse", if newest_first { "true" } else { "false" })
                    .append_pair("page_num", &page_number.to_string())
                    .append_pair("page_size", &page_size.to_string());
            }
            SpaceListKind::Series => {
                url.query_pairs_mut()
                    .append_pair("mid", &owner_mid.to_string())
                    .append_pair("current_mid", "0")
                    .append_pair("series_id", &list_id.to_string())
                    .append_pair("only_normal", "true")
                    .append_pair("sort", "desc")
                    .append_pair("pn", &page_number.to_string())
                    .append_pair("ps", &page_size.to_string());
            }
        }
        let response: ApiData<SpaceArchiveListData> = self.get_json(url).await?;
        response.into_data()
    }

    async fn fetch_space_series_meta(&self, list_id: u64) -> Result<SpaceSeriesMetaData> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/series/series")?;
        url.query_pairs_mut()
            .append_pair("series_id", &list_id.to_string());
        let response: ApiData<SpaceSeriesMetaData> = self.get_json(url).await?;
        response.into_data()
    }

    async fn push_space_archive_items(
        &self,
        items: &mut Vec<VideoCollectionItem>,
        archive: SpaceArchive,
    ) -> Result<()> {
        let aid = archive
            .aid
            .ok_or(Error::MissingField("data.archives[].aid"))?;
        let metadata = self.fetch_video_by_aid(aid, TagPolicy::Skip).await?;
        let page_count = u32::try_from(metadata.pages.len())
            .ok()
            .filter(|count| *count > 0)
            .unwrap_or(1);
        for page in metadata.pages {
            push_unique_collection_item(
                items,
                VideoCollectionItem {
                    index: 0,
                    aid: page.aid,
                    bvid: metadata.bvid.clone().or_else(|| archive.bvid.clone()),
                    cid: page.cid,
                    title: if page_count == 1 {
                        archive
                            .title
                            .clone()
                            .unwrap_or_else(|| metadata.title.clone())
                    } else {
                        format_collection_page_title(
                            archive.title.as_deref().unwrap_or(&metadata.title),
                            page.index,
                            &page.title,
                        )
                    },
                    cover_url: metadata.cover_url.clone().or_else(|| archive.pic.clone()),
                    description: metadata.description.clone(),
                    pub_time: metadata.pub_time.or(archive.pubdate).or(archive.ctime),
                    owner: metadata.owner.clone(),
                    duration_seconds: page.duration_seconds.or(archive.duration),
                },
            );
        }
        Ok(())
    }

    async fn push_medialist_media_items(
        &self,
        items: &mut Vec<VideoCollectionItem>,
        media: MediaListMedia,
    ) -> Result<()> {
        let aid = media
            .id
            .ok_or(Error::MissingField("data.media_list[].id"))?;
        if media.pages.is_empty() {
            let metadata = self.fetch_video_by_aid(aid, TagPolicy::Skip).await?;
            let page_count = u32::try_from(metadata.pages.len())
                .ok()
                .filter(|count| *count > 0)
                .unwrap_or(1);
            for page in metadata.pages {
                push_unique_collection_item(
                    items,
                    VideoCollectionItem {
                        index: 0,
                        aid: page.aid,
                        bvid: metadata.bvid.clone().or_else(|| media.bvid.clone()),
                        cid: page.cid,
                        title: if page_count == 1 {
                            media
                                .title
                                .clone()
                                .unwrap_or_else(|| metadata.title.clone())
                        } else {
                            format_collection_page_title(
                                media.title.as_deref().unwrap_or(&metadata.title),
                                page.index,
                                &page.title,
                            )
                        },
                        cover_url: metadata.cover_url.clone().or_else(|| media.cover.clone()),
                        description: media
                            .intro
                            .clone()
                            .unwrap_or_else(|| metadata.description.clone()),
                        pub_time: media.pubtime.or(metadata.pub_time),
                        owner: media
                            .upper
                            .clone()
                            .and_then(FavoriteUpper::into_owner)
                            .or(metadata.owner.clone()),
                        duration_seconds: page.duration_seconds,
                    },
                );
            }
            return Ok(());
        }
        push_medialist_media_pages(items, media, aid)
    }

    async fn fetch_space_video_collection(
        &self,
        mid: u64,
        selection: Option<Selection>,
        fetch_mode: CollectionFetchMode,
    ) -> Result<VideoCollectionResolution> {
        let collection = self
            .fetch_space_video_collection_all(mid, fetch_mode)
            .await?;
        Self::resolve_collection_selection(collection, selection.as_ref())
    }

    async fn fetch_space_video_collection_all(
        &self,
        mid: u64,
        fetch_mode: CollectionFetchMode,
    ) -> Result<VideoCollectionMetadata> {
        let page_size = 50_u32;
        let mut page_number = 1_u32;
        let first = self
            .fetch_space_video_page(mid, page_number, page_size)
            .await?;
        let total_count = first.page.count.unwrap_or(first.videos.len());
        let total_pages = total_count.div_ceil(page_size as usize);
        let mut page_videos = first.videos;
        let mut items = Vec::new();
        loop {
            for video in page_videos {
                let aid = video
                    .aid
                    .ok_or(Error::MissingField("data.list.vlist[].aid"))?;
                let metadata = self.fetch_video_by_aid(aid, TagPolicy::Skip).await?;
                let page_count = u32::try_from(metadata.pages.len())
                    .ok()
                    .filter(|count| *count > 0)
                    .unwrap_or(1);
                for page in metadata.pages {
                    items.push(VideoCollectionItem {
                        index: 0,
                        aid: page.aid,
                        bvid: metadata.bvid.clone().or_else(|| video.bvid.clone()),
                        cid: page.cid,
                        title: if page_count == 1 {
                            metadata.title.clone()
                        } else {
                            format_collection_page_title(&metadata.title, page.index, &page.title)
                        },
                        cover_url: metadata.cover_url.clone().or_else(|| video.pic.clone()),
                        description: metadata.description.clone(),
                        pub_time: metadata.pub_time.or(video.created),
                        owner: metadata.owner.clone(),
                        duration_seconds: page.duration_seconds,
                    });
                    if fetch_mode.is_satisfied_by(items.len()) {
                        break;
                    }
                }
                if fetch_mode.is_satisfied_by(items.len()) {
                    break;
                }
            }
            if fetch_mode.is_satisfied_by(items.len()) || (page_number as usize) >= total_pages {
                break;
            }
            page_number += 1;
            page_videos = self
                .fetch_space_video_page(mid, page_number, page_size)
                .await?
                .videos;
        }
        renumber_collection_items(&mut items);
        Ok(VideoCollectionMetadata {
            id: Some(mid),
            kind: VideoCollectionKind::Space,
            title: format!("Space {mid} videos"),
            description: String::new(),
            cover_url: None,
            pub_time: None,
            owner: Some(Owner {
                mid,
                name: mid.to_string(),
            }),
            items,
        })
    }

    async fn fetch_space_video_page(
        &self,
        mid: u64,
        page_number: u32,
        page_size: u32,
    ) -> Result<SpaceArcSearchData> {
        let mixin_key = self.fetch_wbi_mixin_key().await?;
        let params = vec![
            ("mid", mid.to_string()),
            ("order", "pubdate".to_owned()),
            ("pn", page_number.to_string()),
            ("ps", page_size.to_string()),
            ("tid", "0".to_owned()),
            ("wts", current_unix_timestamp().to_string()),
        ];
        let mut url =
            Self::endpoint_url(&self.config.endpoints.api_base, "/x/space/wbi/arc/search")?;
        url.set_query(Some(&wbi_signed_query(params, &mixin_key)));
        let response: ApiData<SpaceArcSearchRootData> = self.get_json(url).await?;
        let data = response.into_data()?;
        Ok(SpaceArcSearchData {
            videos: data.list.videos,
            page: data.page,
        })
    }

    async fn fetch_wbi_mixin_key(&self) -> Result<String> {
        let url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/web-interface/nav")?;
        let response: ApiData<NavData> = self.get_json(url).await?;
        let data = response.into_data()?;
        wbi_mixin_key(
            data.wbi_img
                .img_url
                .as_deref()
                .ok_or(Error::MissingField("data.wbi_img.img_url"))?,
            data.wbi_img
                .sub_url
                .as_deref()
                .ok_or(Error::MissingField("data.wbi_img.sub_url"))?,
        )
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
            Some(Selection::Indices(indices)) => select_items_by_index(
                &season.episodes,
                indices,
                |episode| episode.index,
                "selected episode",
            )?,
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
        planning_mode: PlanningMode,
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
                    cover_url: video.cover_url.clone(),
                    source: self.normal_stream_source(),
                    planning_mode,
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
        planning_mode: PlanningMode,
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
                    cover_url: season.season.cover_url.clone(),
                    source: source.clone(),
                    planning_mode,
                })
                .await?,
            );
        }
        Ok(DownloadPlan {
            title: season.season.title,
            entries,
        })
    }

    async fn plan_collection(
        &self,
        collection: VideoCollectionResolution,
        planning_mode: PlanningMode,
    ) -> Result<DownloadPlan> {
        let mut entries = Vec::new();
        for item in collection.selected_items {
            entries.push(
                self.plan_entry(PlanEntrySeed {
                    index: item.index,
                    aid: item.aid,
                    bvid: item.bvid,
                    cid: item.cid,
                    epid: None,
                    title: item.title,
                    cover_url: item.cover_url,
                    source: self.normal_stream_source(),
                    planning_mode,
                })
                .await?,
            );
        }
        Ok(DownloadPlan {
            title: collection.collection.title,
            entries,
        })
    }

    async fn plan_entry(&self, seed: PlanEntrySeed) -> Result<DownloadEntry> {
        let (source, streams, diagnostics) = if seed.planning_mode.requires_streams() {
            let resolved_streams = self
                .fetch_stream_set(seed.source.clone(), seed.aid, seed.cid, seed.epid)
                .await?;
            (
                resolved_streams.source,
                resolved_streams.streams,
                resolved_streams.diagnostics,
            )
        } else {
            (
                seed.source.clone(),
                empty_stream_set(),
                StreamDiagnostics::default(),
            )
        };
        let subtitles = if seed.planning_mode.requires_subtitles() {
            let result = self
                .fetch_subtitles(source.clone(), seed.aid, seed.cid, seed.epid)
                .await;
            if matches!(seed.planning_mode, PlanningMode::SubtitlesOnly) {
                result?
            } else {
                result.unwrap_or_default()
            }
        } else {
            Vec::new()
        };
        Ok(DownloadEntry {
            index: seed.index,
            aid: seed.aid,
            bvid: seed.bvid,
            cid: seed.cid,
            epid: seed.epid,
            title: seed.title,
            cover_url: normalize_optional_media_url(seed.cover_url.as_deref()),
            source,
            streams,
            diagnostics,
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
            Some(Selection::Indices(indices)) => {
                select_items_by_index(&video.pages, indices, |page| page.index, "selected page")?
            }
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

    fn select_video_metadata(
        mut video: VideoMetadata,
        selection: Option<&Selection>,
    ) -> Result<VideoMetadata> {
        if selection.is_some() {
            video.pages = Self::select_video_pages(&video, selection)?;
        }
        Ok(video)
    }

    fn resolve_collection_selection(
        collection: VideoCollectionMetadata,
        selection: Option<&Selection>,
    ) -> Result<VideoCollectionResolution> {
        let selected_items = match selection {
            Some(Selection::Latest) => collection.items.first().cloned().into_iter().collect(),
            Some(Selection::Page(page)) => collection
                .items
                .iter()
                .find(|item| item.index == *page)
                .cloned()
                .into_iter()
                .collect(),
            Some(Selection::Indices(indices)) => select_items_by_index(
                &collection.items,
                indices,
                |item| item.index,
                "selected collection item",
            )?,
            Some(Selection::Episode(_)) => {
                return Err(Error::InvalidInput(
                    "episode selection is only valid for PGC inputs".to_owned(),
                ));
            }
            Some(Selection::Current) => {
                return Err(Error::InvalidInput(
                    "current selection is only valid for inputs that identify a single current item"
                        .to_owned(),
                ));
            }
            Some(Selection::All) | None => collection.items.clone(),
        };
        let allow_empty_selection = matches!(selection, Some(Selection::All) | None);
        if selected_items.is_empty() && !allow_empty_selection {
            return Err(Error::MissingField("selected collection item"));
        }
        Ok(VideoCollectionResolution {
            collection,
            selected_items,
        })
    }

    fn collection_fetch_mode(selection: Option<&Selection>) -> Result<CollectionFetchMode> {
        match selection {
            Some(Selection::Current) => Err(Error::InvalidInput(
                "current selection is only valid for inputs that identify a single current item"
                    .to_owned(),
            )),
            Some(Selection::Episode(_)) => Err(Error::InvalidInput(
                "episode selection is only valid for PGC inputs".to_owned(),
            )),
            Some(Selection::Latest) => Ok(CollectionFetchMode::Latest),
            Some(Selection::Page(page)) => Ok(CollectionFetchMode::Page(*page)),
            Some(Selection::Indices(indices)) => Ok(CollectionFetchMode::Page(indices.max_index())),
            Some(Selection::All) | None => Ok(CollectionFetchMode::All),
        }
    }

    fn collection_info_fetch_mode(selection: Option<&Selection>) -> Result<CollectionFetchMode> {
        Self::collection_fetch_mode(selection).map(|_| CollectionFetchMode::All)
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
        if source == StreamSource::PgcTv {
            return self.fetch_pgc_tv_stream_set(aid, cid, epid).await;
        }
        if source == StreamSource::PgcApp {
            return self.fetch_pgc_app_stream_set(aid, cid, epid).await;
        }
        if source == StreamSource::NormalApp {
            return self
                .fetch_app_stream_set(StreamSource::NormalApp, aid, cid, false)
                .await;
        }
        let mut url = match source {
            StreamSource::NormalWeb => {
                Self::endpoint_url(&self.config.endpoints.api_base, "/x/player/playurl")?
            }
            StreamSource::NormalTv => {
                Self::endpoint_url(&self.config.endpoints.tv_api_base, "/x/tv/playurl")?
            }
            StreamSource::NormalApp => unreachable!(),
            StreamSource::PugvWeb => {
                Self::endpoint_url(&self.config.endpoints.api_base, "/pugv/player/web/playurl")?
            }
            StreamSource::PgcWeb
            | StreamSource::PgcTv
            | StreamSource::PgcApp
            | StreamSource::PgcProxy => {
                unreachable!()
            }
            StreamSource::IntlWeb => Self::endpoint_url(
                &self.config.endpoints.intl_base,
                "/intl/gateway/v2/ogv/playurl",
            )?,
        };
        match source {
            StreamSource::NormalWeb => {
                url.query_pairs_mut()
                    .append_pair("avid", &aid.to_string())
                    .append_pair("cid", &cid.to_string())
                    .append_pair("qn", "0")
                    .append_pair("fnval", "4048")
                    .append_pair("fnver", "0")
                    .append_pair("fourk", "1")
                    .append_pair("try_look", "1")
                    .append_pair("otype", "json");
            }
            StreamSource::NormalTv => {
                append_tv_playurl_params(
                    &mut url,
                    aid,
                    cid,
                    None,
                    self.config.credentials.tv_access_key.as_deref(),
                );
            }
            StreamSource::PugvWeb => {
                let epid = epid.ok_or(Error::MissingField("epid"))?;
                url.query_pairs_mut()
                    .append_pair("avid", &aid.to_string())
                    .append_pair("cid", &cid.to_string())
                    .append_pair("ep_id", &epid.to_string())
                    .append_pair("module", "bangumi")
                    .append_pair("qn", "0")
                    .append_pair("fnval", "4048")
                    .append_pair("fnver", "0")
                    .append_pair("fourk", "1")
                    .append_pair("otype", "json");
            }
            StreamSource::NormalApp
            | StreamSource::PgcWeb
            | StreamSource::PgcTv
            | StreamSource::PgcApp
            | StreamSource::PgcProxy => unreachable!(),
            StreamSource::IntlWeb => {
                let epid = epid.ok_or(Error::MissingField("epid"))?;
                let mut query = url.query_pairs_mut();
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
        let streams = match source {
            StreamSource::NormalTv => self.fetch_playurl_stream_set_without_cookie(url).await?,
            _ => self.fetch_playurl_stream_set(url).await?,
        };
        Ok(ResolvedStreamSet::official(source, streams))
    }

    async fn fetch_pgc_app_stream_set(
        &self,
        aid: u64,
        cid: u64,
        epid: Option<u64>,
    ) -> Result<ResolvedStreamSet> {
        let epid = epid.ok_or(Error::MissingField("epid"))?;
        let url = Self::endpoint_url(
            &self.config.endpoints.app_pgc_grpc_base,
            app_playurl::PGC_PLAYURL_PATH,
        )?;
        match self
            .post_app_playurl_stream_set(url.clone(), epid, cid, true)
            .await
        {
            Ok(streams) => Ok(ResolvedStreamSet::official(StreamSource::PgcApp, streams)),
            Err(error) => {
                self.fetch_pgc_proxy_stream_set_after_error(
                    aid,
                    cid,
                    epid,
                    StreamSource::PgcApp,
                    Some(redact_url_for_diagnostics(&url)),
                    error,
                )
                .await
            }
        }
    }

    async fn fetch_app_stream_set(
        &self,
        source: StreamSource,
        content_id: u64,
        cid: u64,
        is_pgc: bool,
    ) -> Result<ResolvedStreamSet> {
        let base = if is_pgc {
            &self.config.endpoints.app_pgc_grpc_base
        } else {
            &self.config.endpoints.app_grpc_base
        };
        let path = if is_pgc {
            app_playurl::PGC_PLAYURL_PATH
        } else {
            app_playurl::NORMAL_PLAYURL_PATH
        };
        let url = Self::endpoint_url(base, path)?;
        let streams = self
            .post_app_playurl_stream_set(url, content_id, cid, is_pgc)
            .await?;
        Ok(ResolvedStreamSet::official(source, streams))
    }

    async fn post_app_playurl_stream_set(
        &self,
        url: Url,
        content_id: u64,
        cid: u64,
        is_pgc: bool,
    ) -> Result<StreamSet> {
        let response = self
            .http
            .post(url)
            .headers(app_playurl::play_view_headers(
                self.app_playurl_access_key(),
            )?)
            .body(app_playurl::play_view_request_body(
                content_id, cid, is_pgc,
            )?)
            .timeout(self.config.request_timeout)
            .send()
            .await
            .map_err(Self::http_error_without_url)?;
        let response = response
            .error_for_status()
            .map_err(Self::http_error_without_url)?;
        let response = Self::collect_app_grpc_response(response).await?;
        decode_app_grpc_stream_set(&response)
    }

    async fn fetch_pgc_tv_stream_set(
        &self,
        aid: u64,
        cid: u64,
        epid: Option<u64>,
    ) -> Result<ResolvedStreamSet> {
        let epid = epid.ok_or(Error::MissingField("epid"))?;
        let mut url = Self::endpoint_url(
            &self.config.endpoints.tv_api_base,
            "/pgc/player/api/playurltv",
        )?;
        append_tv_playurl_params(
            &mut url,
            aid,
            cid,
            Some(epid),
            self.config.credentials.tv_access_key.as_deref(),
        );
        let streams = self.fetch_playurl_stream_set_without_cookie(url).await?;
        Ok(ResolvedStreamSet::official(StreamSource::PgcTv, streams))
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
            Err(error) => {
                self.fetch_pgc_proxy_stream_set_after_error(
                    aid,
                    cid,
                    epid,
                    StreamSource::PgcWeb,
                    Some(redact_url_for_diagnostics(&official_url)),
                    error,
                )
                .await
            }
        }
    }

    async fn fetch_pgc_proxy_stream_set_after_error(
        &self,
        aid: u64,
        cid: u64,
        epid: u64,
        official_source: StreamSource,
        official_endpoint: Option<String>,
        error: Error,
    ) -> Result<ResolvedStreamSet> {
        if self.config.restricted_area.proxies.is_empty()
            || !is_restricted_area_fallback_error(&error)
        {
            return Err(error);
        }
        let proxy_access_key = self.pgc_proxy_playurl_access_key(&official_source);
        let mut attempts = vec![resolver_attempt(
            official_source,
            None,
            official_endpoint,
            StreamResolverOutcome::Failed,
            Some(resolver_error_message(&error)),
        )];
        for proxy in self.config.restricted_area.ordered_proxies() {
            let request_urls =
                match Self::pgc_proxy_playurl_urls(&proxy, aid, cid, epid, proxy_access_key) {
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
                    .fetch_playurl_stream_set_without_cookie(request_url.clone())
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

    fn pgc_playurl_url(base_url: &str, aid: u64, cid: u64, epid: u64) -> Result<Url> {
        let mut url = Self::endpoint_url(base_url, "/pgc/player/web/v2/playurl")?;
        append_pgc_playurl_params(&mut url, aid, cid, epid, None, None);
        Ok(url)
    }

    fn pgc_proxy_playurl_urls(
        proxy: &RestrictedAreaProxy,
        aid: u64,
        cid: u64,
        epid: u64,
        access_key: Option<&str>,
    ) -> Result<Vec<Url>> {
        let mut urls = match proxy.kind {
            RestrictedAreaProxyKind::PlayUrl => vec![Url::parse(&proxy.base_url)?],
            RestrictedAreaProxyKind::BilibiliApi => vec![
                Self::endpoint_url_preserving_query(&proxy.base_url, "/pgc/player/web/playurl")?,
                Self::endpoint_url_preserving_query(&proxy.base_url, "/pgc/player/web/v2/playurl")?,
            ],
        };
        for url in &mut urls {
            append_pgc_playurl_params(url, aid, cid, epid, proxy.area, access_key);
        }
        Ok(urls)
    }

    fn pgc_proxy_playurl_access_key(&self, official_source: &StreamSource) -> Option<&str> {
        if official_source == &StreamSource::PgcApp {
            self.app_playurl_access_key()
        } else {
            self.config.credentials.access_key.as_deref()
        }
    }

    async fn collect_app_grpc_response(response: reqwest::Response) -> Result<AppGrpcResponse> {
        let response: http::Response<reqwest::Body> = response.into();
        let (parts, body) = response.into_parts();
        let collected = body.collect().await.map_err(Self::http_error_without_url)?;
        let trailers = collected.trailers().cloned();
        Ok(AppGrpcResponse {
            headers: parts.headers,
            body: collected.to_bytes().to_vec(),
            trailers,
        })
    }

    async fn fetch_playurl_stream_set(&self, url: Url) -> Result<StreamSet> {
        let response: PlayUrlRoot = self.get_json(url).await?;
        response.into_stream_set()
    }

    async fn fetch_playurl_stream_set_without_cookie(&self, url: Url) -> Result<StreamSet> {
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
            StreamSource::NormalWeb
            | StreamSource::NormalTv
            | StreamSource::NormalApp
            | StreamSource::PgcWeb
            | StreamSource::PgcTv
            | StreamSource::PgcApp
            | StreamSource::PgcProxy
            | StreamSource::PugvWeb => {
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

    fn normal_stream_source(&self) -> StreamSource {
        match self.config.playurl_mode {
            PlayurlMode::Web => StreamSource::NormalWeb,
            PlayurlMode::Tv => StreamSource::NormalTv,
            PlayurlMode::App => StreamSource::NormalApp,
        }
    }

    fn pgc_stream_source(&self) -> StreamSource {
        match self.config.playurl_mode {
            PlayurlMode::Web => StreamSource::PgcWeb,
            PlayurlMode::Tv => StreamSource::PgcTv,
            PlayurlMode::App => StreamSource::PgcApp,
        }
    }

    fn app_playurl_access_key(&self) -> Option<&str> {
        self.config
            .credentials
            .tv_access_key
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.config
                    .credentials
                    .access_key
                    .as_deref()
                    .filter(|value| !value.is_empty())
            })
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

fn deserialize_default_vec<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
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

fn pugv_episode_page_size(page: &PugvEpisodePage, loaded_episode_count: usize) -> u32 {
    page.size
        .or_else(|| u32::try_from(loaded_episode_count).ok())
        .filter(|size| *size > 0)
        .unwrap_or(20)
}

fn pugv_page_has_next(page: &PugvEpisodePage, page_size: u32, page_number: u32) -> bool {
    page.next.unwrap_or_else(|| {
        page.total
            .is_some_and(|total| page_number < total.div_ceil(page_size))
    })
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

fn season_from_pugv(data: PugvSeasonData) -> SeasonMetadata {
    let owner = data.up_info.and_then(FavoriteUpper::into_owner);
    let mut episodes = Vec::new();
    for (fallback_index, episode) in data.episodes.into_iter().enumerate() {
        let Some(epid) = episode.id else {
            continue;
        };
        let Some(aid) = episode.aid else {
            continue;
        };
        let Some(cid) = episode.cid else {
            continue;
        };
        let Some(index) = u32::try_from(fallback_index + 1).ok() else {
            continue;
        };
        episodes.push(EpisodeMetadata {
            index,
            aid,
            bvid: None,
            cid,
            epid,
            title: index.to_string(),
            long_title: episode.title,
            pub_time: episode.release_date,
        });
    }
    let main_episode_count = episodes.len();
    SeasonMetadata {
        season_id: data.season_id,
        media_id: None,
        title: data.title.unwrap_or_default(),
        description: data.subtitle.unwrap_or_default(),
        cover_url: data.cover,
        main_episode_count,
        areas: Vec::new(),
        tags: owner
            .map(|owner| vec![format!("UP: {}", owner.name)])
            .unwrap_or_default(),
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

#[derive(Debug, Deserialize)]
struct PugvSeasonData {
    season_id: Option<u64>,
    title: Option<String>,
    subtitle: Option<String>,
    cover: Option<String>,
    up_info: Option<FavoriteUpper>,
    episode_page: Option<PugvEpisodePage>,
    #[serde(default)]
    episodes: Vec<PugvEpisode>,
}

#[derive(Debug, Deserialize)]
struct PugvEpisodeListData {
    #[serde(default, alias = "episodes")]
    items: Vec<PugvEpisode>,
    page: Option<PugvEpisodePage>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct PugvEpisodePage {
    next: Option<bool>,
    #[serde(alias = "pn", alias = "page_num")]
    num: Option<u32>,
    #[serde(alias = "ps", alias = "page_size")]
    size: Option<u32>,
    #[serde(alias = "count")]
    total: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PugvEpisode {
    id: Option<u64>,
    aid: Option<u64>,
    cid: Option<u64>,
    title: Option<String>,
    release_date: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct FavoriteFolderListData {
    #[serde(default)]
    list: Vec<FavoriteFolder>,
}

#[derive(Debug, Deserialize)]
struct FavoriteFolder {
    id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FavoriteResourceListData {
    info: FavoriteInfo,
    #[serde(default, deserialize_with = "deserialize_default_vec")]
    medias: Vec<FavoriteMedia>,
}

#[derive(Debug, Deserialize)]
struct FavoriteInfo {
    media_count: Option<usize>,
    title: Option<String>,
    intro: Option<String>,
    cover: Option<String>,
    ctime: Option<i64>,
    upper: Option<FavoriteUpper>,
}

#[derive(Clone, Debug, Deserialize)]
struct FavoriteUpper {
    mid: Option<u64>,
    name: Option<String>,
    uname: Option<String>,
}

impl FavoriteUpper {
    fn into_owner(self) -> Option<Owner> {
        Some(Owner {
            mid: self.mid?,
            name: self.name.or(self.uname).unwrap_or_default(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct FavoriteMedia {
    id: Option<u64>,
    #[serde(rename = "type")]
    media_type: Option<u8>,
    bvid: Option<String>,
    title: Option<String>,
    intro: Option<String>,
    cover: Option<String>,
    pubtime: Option<i64>,
    duration: Option<u32>,
    attr: Option<i64>,
    page: Option<u32>,
    upper: Option<FavoriteUpper>,
    ugc: Option<FavoriteUgc>,
}

#[derive(Debug, Deserialize)]
struct FavoriteUgc {
    first_cid: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct MediaListInfoData {
    title: Option<String>,
    intro: Option<String>,
    cover: Option<String>,
    ctime: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct MediaListResourcePageData {
    has_more: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_default_vec")]
    media_list: Vec<MediaListMedia>,
}

#[derive(Debug, Deserialize)]
struct MediaListMedia {
    id: Option<u64>,
    #[serde(alias = "bv_id")]
    bvid: Option<String>,
    title: Option<String>,
    intro: Option<String>,
    cover: Option<String>,
    pubtime: Option<i64>,
    attr: Option<i64>,
    page: Option<u32>,
    upper: Option<FavoriteUpper>,
    #[serde(default)]
    pages: Vec<MediaListPage>,
}

#[derive(Debug, Deserialize)]
struct MediaListPage {
    id: Option<u64>,
    page: Option<u32>,
    title: Option<String>,
    duration: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SpaceArcSearchRootData {
    list: SpaceArcVideoList,
    page: SpaceArcPage,
}

#[derive(Debug)]
struct SpaceArcSearchData {
    videos: Vec<SpaceArcVideo>,
    page: SpaceArcPage,
}

#[derive(Debug, Deserialize)]
struct SpaceArcVideoList {
    #[serde(default, rename = "vlist")]
    videos: Vec<SpaceArcVideo>,
}

#[derive(Debug, Deserialize)]
struct SpaceArcPage {
    count: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SpaceArcVideo {
    aid: Option<u64>,
    bvid: Option<String>,
    pic: Option<String>,
    created: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SpaceArchiveListData {
    #[serde(default)]
    archives: Vec<SpaceArchive>,
    meta: Option<SpaceArchiveMeta>,
    #[serde(default)]
    page: SpaceArchivePage,
}

#[derive(Debug, Deserialize)]
struct SpaceSeriesMetaData {
    meta: Option<SpaceArchiveMeta>,
}

#[derive(Debug, Deserialize)]
struct SpaceArchive {
    aid: Option<u64>,
    bvid: Option<String>,
    title: Option<String>,
    pic: Option<String>,
    ctime: Option<i64>,
    pubdate: Option<i64>,
    duration: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SpaceArchiveMeta {
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    cover: Option<String>,
    mid: Option<u64>,
    ptime: Option<i64>,
    ctime: Option<i64>,
    total: Option<usize>,
}

impl SpaceArchiveMeta {
    fn title(&self) -> Option<String> {
        first_non_empty([self.name.clone(), self.title.clone()])
    }

    fn pub_time(&self) -> Option<i64> {
        self.ptime.or(self.ctime)
    }
}

#[derive(Default, Debug, Deserialize)]
struct SpaceArchivePage {
    total: Option<usize>,
    #[serde(alias = "pn", alias = "page_num")]
    _num: Option<u32>,
    #[serde(alias = "ps", alias = "page_size")]
    _size: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct NavData {
    wbi_img: WbiImage,
}

#[derive(Debug, Deserialize)]
struct WbiImage {
    img_url: Option<String>,
    sub_url: Option<String>,
}

#[derive(Clone, Debug)]
struct PlanEntrySeed {
    index: u32,
    aid: u64,
    bvid: Option<String>,
    cid: u64,
    epid: Option<u64>,
    title: String,
    cover_url: Option<String>,
    source: StreamSource,
    planning_mode: PlanningMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlanningMode {
    Full,
    StreamsOnly,
    SubtitlesOnly,
    MetadataOnly,
}

impl PlanningMode {
    const fn from_download_mode(mode: DownloadMode) -> Self {
        match mode {
            DownloadMode::All => Self::Full,
            DownloadMode::VideoOnly | DownloadMode::AudioOnly => Self::StreamsOnly,
            DownloadMode::SubtitleOnly => Self::SubtitlesOnly,
            DownloadMode::DanmakuOnly | DownloadMode::CoverOnly => Self::MetadataOnly,
        }
    }

    const fn requires_streams(self) -> bool {
        matches!(self, Self::Full | Self::StreamsOnly)
    }

    const fn requires_subtitles(self) -> bool {
        matches!(self, Self::Full | Self::SubtitlesOnly)
    }
}

#[derive(Clone, Debug)]
struct ResolvedStreamSet {
    source: StreamSource,
    streams: StreamSet,
    diagnostics: StreamDiagnostics,
}

struct AppGrpcResponse {
    headers: HeaderMap,
    body: Vec<u8>,
    trailers: Option<HeaderMap>,
}

fn empty_stream_set() -> StreamSet {
    StreamSet {
        videos: Vec::new(),
        audios: Vec::new(),
        flv_segments: Vec::new(),
        accept_quality: Vec::new(),
        qualities: Vec::new(),
        duration_seconds: None,
    }
}

fn select_items_by_index<T: Clone>(
    items: &[T],
    selection: &IndexSelection,
    index_of: impl Fn(&T) -> u32,
    missing_field: &'static str,
) -> Result<Vec<T>> {
    let indexed_items = items
        .iter()
        .map(|item| (index_of(item), item))
        .collect::<Vec<_>>();
    let available_indices = indexed_items
        .iter()
        .map(|(index, _)| *index)
        .collect::<HashSet<_>>();
    for selector in selection.selectors() {
        validate_index_selector_matches(*selector, &available_indices, missing_field)?;
    }

    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for selector in selection.selectors() {
        for (index, item) in &indexed_items {
            if selector.contains(*index) && seen.insert(*index) {
                selected.push((*item).clone());
            }
        }
    }
    Ok(selected)
}

fn validate_index_selector_matches(
    selector: crate::IndexSelector,
    available_indices: &HashSet<u32>,
    missing_field: &'static str,
) -> Result<()> {
    match selector {
        crate::IndexSelector::Index(index) => {
            if available_indices.contains(&index) {
                Ok(())
            } else {
                Err(Error::MissingField(missing_field))
            }
        }
        crate::IndexSelector::Range { start, end } => {
            let requested_count = u64::from(end) - u64::from(start) + 1;
            if requested_count > available_indices.len() as u64 {
                return Err(Error::MissingField(missing_field));
            }
            if (start..=end).all(|index| available_indices.contains(&index)) {
                Ok(())
            } else {
                Err(Error::MissingField(missing_field))
            }
        }
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MediaListKind {
    Collection,
    Series,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpaceListKind {
    Collection,
    Series,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CollectionFetchMode {
    All,
    Latest,
    Page(u32),
}

impl CollectionFetchMode {
    fn is_satisfied_by(self, item_count: usize) -> bool {
        match self {
            Self::All => false,
            Self::Latest => item_count > 0,
            Self::Page(page) => {
                page != 0 && usize::try_from(page).is_ok_and(|target| item_count >= target)
            }
        }
    }
}

impl MediaListKind {
    const fn type_id(self) -> &'static str {
        match self {
            Self::Collection => "8",
            Self::Series => "5",
        }
    }

    const fn desc_value(self) -> &'static str {
        match self {
            Self::Collection => "false",
            Self::Series => "true",
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Collection => "Collection",
            Self::Series => "Series",
        }
    }

    const fn into_collection_kind(self) -> VideoCollectionKind {
        match self {
            Self::Collection => VideoCollectionKind::Collection,
            Self::Series => VideoCollectionKind::Series,
        }
    }
}

impl SpaceListKind {
    const fn title(self) -> &'static str {
        match self {
            Self::Collection => "Collection",
            Self::Series => "Series",
        }
    }

    const fn into_collection_kind(self) -> VideoCollectionKind {
        match self {
            Self::Collection => VideoCollectionKind::Collection,
            Self::Series => VideoCollectionKind::Series,
        }
    }
}

fn episode_display_title(title: &str, long_title: Option<&str>) -> String {
    match long_title.filter(|value| !value.is_empty()) {
        Some(long_title) if title.is_empty() => long_title.to_owned(),
        Some(long_title) => format!("{title} {long_title}"),
        None => title.to_owned(),
    }
}

fn push_medialist_media_pages(
    items: &mut Vec<VideoCollectionItem>,
    media: MediaListMedia,
    aid: u64,
) -> Result<()> {
    let title = media.title.unwrap_or_else(|| aid.to_string());
    let page_count = media.page.unwrap_or_else(|| {
        u32::try_from(media.pages.len())
            .ok()
            .filter(|count| *count > 0)
            .unwrap_or(1)
    });
    for page in media.pages {
        let cid = page
            .id
            .ok_or(Error::MissingField("data.media_list[].pages[].id"))?;
        let page_index = page.page.unwrap_or(1);
        push_unique_collection_item(
            items,
            VideoCollectionItem {
                index: 0,
                aid,
                bvid: media.bvid.clone(),
                cid,
                title: if page_count == 1 {
                    title.clone()
                } else {
                    format_collection_page_title(
                        &title,
                        page_index,
                        page.title.as_deref().unwrap_or_default(),
                    )
                },
                cover_url: media.cover.clone(),
                description: media.intro.clone().unwrap_or_default(),
                pub_time: media.pubtime,
                owner: media.upper.clone().and_then(FavoriteUpper::into_owner),
                duration_seconds: page.duration,
            },
        );
    }
    Ok(())
}

fn push_unique_collection_item(items: &mut Vec<VideoCollectionItem>, item: VideoCollectionItem) {
    if items
        .iter()
        .any(|existing| existing.aid == item.aid && existing.cid == item.cid)
    {
        return;
    }
    items.push(item);
}

fn renumber_collection_items(items: &mut [VideoCollectionItem]) {
    for (index, item) in items.iter_mut().enumerate() {
        if let Ok(next_index) = u32::try_from(index + 1) {
            item.index = next_index;
        }
    }
}

fn format_collection_page_title(video_title: &str, page_index: u32, page_title: &str) -> String {
    if page_index == 1 && (page_title.is_empty() || page_title == video_title) {
        return video_title.to_owned();
    }
    if page_title.is_empty() {
        format!("{video_title}_P{page_index}")
    } else {
        format!("{video_title}_P{page_index}_{page_title}")
    }
}

fn wbi_signed_query(mut params: Vec<(&'static str, String)>, mixin_key: &str) -> String {
    params.sort_by(|left, right| left.0.cmp(right.0));
    let encoded = encode_query(&params);
    let sign = format!(
        "{:x}",
        Md5::digest(format!("{encoded}{mixin_key}").as_bytes())
    );
    params.push(("w_rid", sign));
    encode_query(&params)
}

fn encode_query(params: &[(&'static str, String)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn app_sign(query: &str, secret: &str) -> String {
    format!("{:x}", Md5::digest(format!("{query}{secret}").as_bytes()))
}

fn append_tv_playurl_params(
    url: &mut Url,
    aid: u64,
    cid: u64,
    epid: Option<u64>,
    access_key: Option<&str>,
) {
    let mut params = Vec::new();
    if let Some(access_key) = access_key.filter(|value| !value.is_empty()) {
        params.push(("access_key", access_key.to_owned()));
    }
    params.extend([
        ("appkey", TV_PLAYURL_APPKEY.to_owned()),
        ("build", "106500".to_owned()),
        ("cid", cid.to_string()),
        ("device", "android".to_owned()),
    ]);
    if let Some(epid) = epid {
        params.extend([("ep_id", epid.to_string()), ("expire", "0".to_owned())]);
    }
    params.extend([
        ("fnval", "4048".to_owned()),
        ("fnver", "0".to_owned()),
        ("fourk", "1".to_owned()),
        ("mid", "0".to_owned()),
        ("mobi_app", "android_tv_yst".to_owned()),
        ("object_id", aid.to_string()),
        ("platform", "android".to_owned()),
        ("playurl_type", "1".to_owned()),
        ("qn", "0".to_owned()),
        ("ts", current_unix_timestamp().to_string()),
    ]);
    let sign = app_sign(&encode_query(&params), TV_PLAYURL_APP_SECRET);
    let mut query = url.query_pairs_mut();
    for (key, value) in params {
        query.append_pair(key, &value);
    }
    query.append_pair("sign", &sign);
}

fn wbi_mixin_key(img_url: &str, sub_url: &str) -> Result<String> {
    const MIXIN_KEY_ENC_TAB: [usize; 32] = [
        46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19,
        29, 28, 14, 39, 12, 38, 41, 13,
    ];
    let source = format!("{}{}", wbi_key_part(img_url)?, wbi_key_part(sub_url)?);
    let mut output = String::with_capacity(32);
    for index in MIXIN_KEY_ENC_TAB {
        output.push(char::from(
            *source
                .as_bytes()
                .get(index)
                .ok_or(Error::MissingField("data.wbi_img mixin key"))?,
        ));
    }
    Ok(output)
}

fn wbi_key_part(raw: &str) -> Result<&str> {
    raw.rsplit('/')
        .next()
        .and_then(|file_name| file_name.split('.').next())
        .filter(|part| !part.is_empty())
        .ok_or(Error::MissingField("data.wbi_img key part"))
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

fn app_grpc_error_from_headers(headers: &HeaderMap) -> Option<Error> {
    let status = headers
        .get("grpc-status")
        .and_then(|value| value.to_str().ok())?
        .trim();
    if status.is_empty() || status == "0" {
        return None;
    }
    let code = status.parse::<i64>().unwrap_or(-1);
    let message = headers
        .get("grpc-message")
        .and_then(|value| value.to_str().ok())
        .map(decode_grpc_message)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "APP playurl gRPC request failed".to_owned());
    Some(Error::Api { code, message })
}

fn decode_app_grpc_stream_set(response: &AppGrpcResponse) -> Result<StreamSet> {
    if let Some(error) = app_grpc_error_from_headers(&response.headers) {
        return Err(error);
    }
    if let Some(trailers) = response.trailers.as_ref()
        && let Some(error) = app_grpc_error_from_headers(trailers)
    {
        return Err(error);
    }
    app_playurl::decode_play_view_response(&response.body)
}

fn decode_grpc_message(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            output.push((high << 4) | low);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).unwrap_or_else(|_| raw.to_owned())
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
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
    #[serde(default, deserialize_with = "deserialize_optional_play_payload")]
    data: Option<PlayPayload>,
    #[serde(default, deserialize_with = "deserialize_optional_play_payload")]
    result: Option<PlayPayload>,
    #[serde(flatten)]
    payload: PlayPayload,
}

fn deserialize_optional_play_payload<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<PlayPayload>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(value) = Option::<serde_json::Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Object(_) => serde_json::from_value(value)
            .map(Some)
            .map_err(serde::de::Error::custom),
        _ => Ok(None),
    }
}

impl PlayUrlRoot {
    fn into_stream_set(self) -> Result<StreamSet> {
        if self.code != 0 {
            return Err(Error::Api {
                code: self.code,
                message: self.message,
            });
        }
        let payload = self
            .result
            .or(self.data)
            .or_else(|| self.payload.has_playurl_content().then_some(self.payload))
            .ok_or(Error::MissingField("playurl result"))?;
        payload.into_stream_set()
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

impl PlayPayload {
    fn has_playurl_content(&self) -> bool {
        self.video_info.is_some()
            || self.playurl.is_some()
            || self.dash.is_some()
            || self.stream_list.is_some()
            || self.dash_audio.is_some()
            || self.durl.is_some()
    }

    fn into_stream_set(mut self) -> Result<StreamSet> {
        if let Some(video_info) = self.video_info.take() {
            return video_info.into_stream_set();
        }
        if let Some(playurl) = self.playurl.take() {
            return playurl.into_stream_set(self.timelength);
        }
        let dash_duration = self.dash.as_ref().and_then(|dash| dash.duration);
        let duration_seconds = dash_duration.or_else(|| {
            self.timelength
                .and_then(|value| u32::try_from(value / 1000).ok())
        });
        let mut videos = Vec::new();
        let mut audios = Vec::new();
        if let Some(dash) = self.dash {
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
            self.stream_list
                .unwrap_or_default()
                .into_iter()
                .filter_map(IntlStreamItem::into_video_stream),
        );
        audios.extend(
            self.dash_audio
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .filter_map(|(index, resource)| resource.into_media_stream(fallback_id(index))),
        );
        let flv_segments = self
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
                &self.accept_quality,
                &self.accept_description,
                &self.support_formats,
                &videos,
            ),
            videos,
            audios,
            flv_segments,
            accept_quality: self.accept_quality,
            duration_seconds,
        })
    }
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

fn normalize_optional_media_url(url: Option<&str>) -> Option<String> {
    url.map(str::trim)
        .map(normalize_media_url)
        .filter(|url| !url.is_empty())
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
    for stream in videos {
        push_stream_quality(
            &mut qualities,
            stream.id,
            support_format_label(stream.id, support_formats)
                .or_else(|| accept_quality_label(stream.id, accept_quality, accept_description)),
        );
    }
    qualities
}

fn accept_quality_label(
    id: u32,
    accept_quality: &[u32],
    accept_description: &[String],
) -> Option<String> {
    accept_quality
        .iter()
        .position(|quality| *quality == id)
        .and_then(|index| accept_description.get(index))
        .filter(|value| !value.is_empty())
        .cloned()
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
        BiliClient, ClientConfig, EndpointConfig, MediaListKind, PlayUrlRoot, PlayurlMode,
        RestrictedArea, RestrictedAreaConfig, RestrictedAreaProxy, TV_PLAYURL_APPKEY,
        decode_app_grpc_stream_set, intl_ogv_playurl_params,
    };
    use crate::{
        Credentials, EpisodeMetadata, Error, IndexSelection, IndexSelector, Input, PageMetadata,
        ResolvedContent, SeasonMetadata, Selection, StreamSource, SubtitleFormat,
        VideoCollectionItem, VideoCollectionKind, VideoCollectionMetadata, VideoMetadata,
        app_playurl,
    };
    use http_body_util::BodyExt as _;
    use httpmock::MockServer;
    use httpmock::prelude::*;
    use reqwest::header::{HeaderMap, HeaderValue};
    use std::convert::Infallible;
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
            ResolvedContent::Season(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected video"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn video_info_applies_page_selection() -> anyhow::Result<()> {
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
                    "pages": [
                        {"page": 1, "cid": 9981, "part": "P1"},
                        {"page": 2, "cid": 9982, "part": "P2"},
                        {"page": 3, "cid": 9983, "part": "P3"}
                    ]
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

        let client = test_client(&server);
        let resolved = client
            .resolve_input("av170001", Some("3,1".parse()?))
            .await?;

        match resolved {
            ResolvedContent::Video(video) => {
                assert_eq!(
                    video
                        .pages
                        .iter()
                        .map(|page| page.index)
                        .collect::<Vec<_>>(),
                    [3, 1]
                );
            }
            ResolvedContent::Season(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected video"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn video_info_rejects_missing_page_selection() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_001,
                    "title": "Example video",
                    "pages": [
                        {"page": 1, "cid": 9981, "part": "P1"},
                        {"page": 2, "cid": 9982, "part": "P2"}
                    ]
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

        let client = test_client(&server);
        let Err(error) = client
            .resolve_input("av170001", Some("1,999".parse()?))
            .await
        else {
            return Err(anyhow::anyhow!("missing selected page should fail"));
        };

        assert!(matches!(error, Error::MissingField("selected page")));
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
    fn medialist_desc_matches_bbdown_parity() {
        assert_eq!(MediaListKind::Collection.desc_value(), "false");
        assert_eq!(MediaListKind::Series.desc_value(), "true");
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
    fn playurl_accepts_top_level_bpplayurl_flv_shape() -> anyhow::Result<()> {
        let response: PlayUrlRoot = serde_json::from_value(serde_json::json!({
            "code": 0,
            "result": "suee",
            "timelength": 42_000,
            "durl": [{
                "url": "//flv.example/segment.flv",
                "backupUrl": ["//flv-backup.example/segment.flv"],
                "length": 42_000
            }]
        }))?;

        let streams = response.into_stream_set()?;

        assert!(streams.videos.is_empty());
        assert!(streams.audios.is_empty());
        assert_eq!(streams.duration_seconds, Some(42));
        assert_eq!(
            streams.flv_segments[0].url,
            "https://flv.example/segment.flv"
        );
        assert_eq!(
            streams.flv_segments[0].backup_urls[0],
            "https://flv-backup.example/segment.flv"
        );
        Ok(())
    }

    #[test]
    fn playurl_accepts_top_level_mobile_dash_shape() -> anyhow::Result<()> {
        let response: PlayUrlRoot = serde_json::from_value(serde_json::json!({
            "code": 0,
            "timelength": 123_000,
            "accept_quality": [80],
            "accept_description": ["1080P"],
            "support_formats": [{"quality": 80, "new_description": "1080P 高码率"}],
            "dash": {
                "duration": 123,
                "video": [{
                    "id": 80,
                    "baseUrl": "//video.example/80.m4s",
                    "codecs": "avc1.640028",
                    "width": 1920,
                    "height": 1080
                }],
                "audio": [{
                    "id": 30280,
                    "baseUrl": "//audio.example/30280.m4s",
                    "codecs": "mp4a.40.2"
                }]
            }
        }))?;

        let streams = response.into_stream_set()?;

        assert_eq!(streams.videos[0].id, 80);
        assert_eq!(streams.audios[0].id, 30280);
        assert_eq!(streams.accept_quality, vec![80]);
        assert_eq!(streams.qualities[0].id, 80);
        assert_eq!(
            streams.qualities[0].description.as_deref(),
            Some("1080P 高码率")
        );
        assert_eq!(streams.duration_seconds, Some(123));
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
                    "pic": "//cover.example/video.jpg",
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
            entry.cover_url.as_deref(),
            Some("https://cover.example/video.jpg")
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
        assert_eq!(entry.streams.accept_quality, vec![80, 64]);
        assert_eq!(entry.streams.qualities.len(), 1);
        assert_eq!(entry.streams.qualities[0].id, 80);
        assert_eq!(
            entry.streams.qualities[0].description.as_deref(),
            Some("1080P 高码率")
        );
        assert_eq!(entry.subtitles[0].url, "https://subtitle.example/zh.json");
        assert_eq!(
            entry.danmaku.xml_url,
            format!("{}/9988.xml", server.base_url())
        );
        Ok(())
    }

    #[tokio::test]
    async fn plans_video_download_with_tv_playurl_mode() -> anyhow::Result<()> {
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
                    "title": "TV video",
                    "pages": [{"page": 1, "cid": 9988, "part": "P1"}]
                }
            }));
        });
        let tv_playurl = server.mock(|when, then| {
            when.method(GET)
                .path("/x/tv/playurl")
                .query_param("access_key", "TV_ACCESS")
                .query_param("appkey", TV_PLAYURL_APPKEY)
                .query_param("cid", "9988")
                .query_param("mobi_app", "android_tv_yst")
                .query_param("object_id", "170001")
                .query_param("platform", "android")
                .query_param("playurl_type", "1")
                .query_param("qn", "0")
                .query_param_exists("ts")
                .query_param_exists("sign")
                .header_missing("cookie");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "dash": {
                        "duration": 123,
                        "video": [{
                            "id": 80,
                            "base_url": "https://tv.example/80.m4s",
                            "codecs": "avc1.640028",
                            "bandwidth": 1_000_000,
                            "mime_type": "video/mp4"
                        }],
                        "audio": []
                    }
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
                "data": {"subtitle": {"subtitles": []}}
            }));
        });

        let client = BiliClient::new(
            ClientConfig::default()
                .with_endpoints(
                    EndpointConfig::default()
                        .with_api_base(server.base_url())
                        .with_comment_base(server.base_url())
                        .with_tv_api_base(server.base_url()),
                )
                .with_credentials(
                    Credentials::default()
                        .with_cookie("SESSDATA=WEB_COOKIE")
                        .with_tv_access_key("TV_ACCESS"),
                )
                .with_playurl_mode(PlayurlMode::Tv),
        );
        let plan = client.plan_download("av170001", None).await?;

        tv_playurl.assert();
        let entry = &plan.entries[0];
        assert_eq!(entry.source, StreamSource::NormalTv);
        assert_eq!(
            entry.streams.videos[0].base_url,
            "https://tv.example/80.m4s"
        );
        Ok(())
    }

    #[tokio::test]
    async fn plans_video_download_with_app_playurl_mode() -> anyhow::Result<()> {
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
                    "title": "APP video",
                    "pages": [{"page": 1, "cid": 9988, "part": "P1"}]
                }
            }));
        });
        let app_response =
            app_playurl::test_play_view_response_frame("https://app.example/80.m4s")?;
        let app_playurl = server.mock(|when, then| {
            when.method(POST)
                .path(app_playurl::NORMAL_PLAYURL_PATH)
                .header("content-type", "application/grpc")
                .header("authorization", "identify_v1 TV_ACCESS")
                .header_exists("x-bili-metadata-bin")
                .header_exists("x-bili-device-bin")
                .header_exists("x-bili-network-bin")
                .header_missing("cookie");
            then.status(200).body(app_response.clone());
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/player/v2")
                .query_param("aid", "170001")
                .query_param("cid", "9988");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"subtitle": {"subtitles": []}}
            }));
        });

        let client = BiliClient::new(
            ClientConfig::default()
                .with_endpoints(
                    EndpointConfig::default()
                        .with_api_base(server.base_url())
                        .with_comment_base(server.base_url())
                        .with_app_grpc_base(server.base_url()),
                )
                .with_credentials(
                    Credentials::default()
                        .with_cookie("SESSDATA=WEB_COOKIE")
                        .with_tv_access_key("TV_ACCESS"),
                )
                .with_playurl_mode(PlayurlMode::App),
        );
        let plan = client.plan_download("av170001", None).await?;

        app_playurl.assert();
        let entry = &plan.entries[0];
        assert_eq!(entry.source, StreamSource::NormalApp);
        assert_eq!(
            entry.streams.videos[0].base_url,
            "https://app.example/80.m4s"
        );
        assert_eq!(entry.streams.videos[0].codecs.as_deref(), Some("hev1"));
        assert_eq!(entry.streams.audios[0].codecs.as_deref(), Some("mp4a.40.2"));
        assert_eq!(entry.streams.flv_segments[0].order, 1);
        Ok(())
    }

    #[tokio::test]
    async fn app_grpc_status_error_is_reported() -> anyhow::Result<()> {
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
                    "title": "APP video",
                    "pages": [{"page": 1, "cid": 9988, "part": "P1"}]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(POST)
                .path(app_playurl::NORMAL_PLAYURL_PATH)
                .header("content-type", "application/grpc")
                .header("authorization", "identify_v1 APP_ACCESS")
                .header_missing("cookie");
            then.status(200)
                .header("grpc-status", "7")
                .header("grpc-message", "area%20restricted");
        });

        let client = BiliClient::new(
            ClientConfig::default()
                .with_endpoints(
                    EndpointConfig::default()
                        .with_api_base(server.base_url())
                        .with_app_grpc_base(server.base_url()),
                )
                .with_credentials(Credentials::default().with_access_key("APP_ACCESS"))
                .with_playurl_mode(PlayurlMode::App),
        );
        let error = client
            .plan_download("av170001", None)
            .await
            .err()
            .ok_or_else(|| anyhow::anyhow!("APP gRPC status error should fail"))?;

        assert!(matches!(
            error,
            Error::Api { code: 7, message } if message == "area restricted"
        ));
        Ok(())
    }

    #[tokio::test]
    async fn app_grpc_trailing_status_error_is_reported() -> anyhow::Result<()> {
        let mut trailers = HeaderMap::new();
        trailers.insert("grpc-status", HeaderValue::from_static("7"));
        trailers.insert(
            "grpc-message",
            HeaderValue::from_static("area%20restricted"),
        );
        let body = http_body_util::Empty::<bytes::Bytes>::new()
            .with_trailers(std::future::ready(Some(Ok::<_, Infallible>(trailers))));
        let response = http::Response::builder()
            .status(200)
            .body(reqwest::Body::wrap(body))?;
        let response: reqwest::Response = response.into();
        let response = BiliClient::collect_app_grpc_response(response).await?;
        let error = decode_app_grpc_stream_set(&response)
            .err()
            .ok_or_else(|| anyhow::anyhow!("APP trailing gRPC status error should fail"))?;

        assert!(matches!(
            error,
            Error::Api { code: 7, message } if message == "area restricted"
        ));
        Ok(())
    }

    #[tokio::test]
    async fn plans_cheese_download_with_pugv_source() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/view/web/season")
                .query_param("ep_id", "101");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "season_id": 202,
                    "title": "Course",
                    "subtitle": "Course subtitle",
                    "cover": "https://example.invalid/course.jpg",
                    "up_info": {"mid": 1, "uname": "Teacher"},
                    "episodes": [{
                        "id": 101,
                        "aid": 170_001,
                        "cid": 9988,
                        "index": 1,
                        "title": "Lesson",
                        "duration": 12,
                        "release_date": 1_700_000_000
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/player/web/playurl")
                .query_param("avid", "170001")
                .query_param("cid", "9988")
                .query_param("ep_id", "101");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "dash": {
                        "duration": 12,
                        "video": [{
                            "id": 80,
                            "baseUrl": "https://video.example/course.m4s",
                            "base_url": "https://video.example/course.m4s"
                        }],
                        "audio": [{
                            "id": 30280,
                            "baseUrl": "https://audio.example/course.m4s",
                            "base_url": "https://audio.example/course.m4s"
                        }]
                    }
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
                "data": {"subtitle": {"subtitles": []}}
            }));
        });

        let client = test_client(&server);
        let plan = client.plan_download("cheese/ep101", None).await?;

        assert_eq!(plan.title, "Course");
        assert_eq!(plan.entries[0].source, StreamSource::PugvWeb);
        assert_eq!(plan.entries[0].epid, Some(101));
        assert_eq!(plan.entries[0].streams.videos[0].id, 80);
        Ok(())
    }

    #[tokio::test]
    async fn cheese_episode_current_uses_global_index_from_loaded_order() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/view/web/season")
                .query_param("ep_id", "102");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "season_id": 202,
                    "title": "Course",
                    "episode_page": {"next": false, "num": 2, "size": 1, "total": 2},
                    "episodes": [{
                        "id": 102,
                        "aid": 170_002,
                        "cid": 9989,
                        "index": 1,
                        "page": 2,
                        "title": "Second lesson"
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/view/web/season")
                .query_param("season_id", "202")
                .query_param_missing("pn");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "season_id": 202,
                    "title": "Course",
                    "episode_page": {"next": true, "num": 1, "size": 1, "total": 2},
                    "episodes": [{
                        "id": 101,
                        "aid": 170_001,
                        "cid": 9988,
                        "index": 1,
                        "title": "First lesson"
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/view/web/ep/list")
                .query_param("season_id", "202")
                .query_param("pn", "2")
                .query_param("ps", "1");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "page": {"next": false, "num": 2, "size": 1, "total": 2},
                    "items": [{
                        "id": 102,
                        "aid": 170_002,
                        "cid": 9989,
                        "index": 1,
                        "page": 2,
                        "title": "Second lesson"
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/player/web/playurl")
                .query_param("avid", "170002")
                .query_param("cid", "9989")
                .query_param("ep_id", "102");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "dash": {
                        "duration": 12,
                        "video": [{
                            "id": 80,
                            "baseUrl": "https://video.example/course.m4s",
                            "base_url": "https://video.example/course.m4s"
                        }],
                        "audio": [{
                            "id": 30280,
                            "baseUrl": "https://audio.example/course.m4s",
                            "base_url": "https://audio.example/course.m4s"
                        }]
                    }
                }
            }));
        });
        server_mock_player_v2(&server, 170_002, 9989);

        let plan = test_client(&server)
            .plan_download("cheese/ep102", None)
            .await?;

        assert_eq!(plan.entries[0].index, 2);
        assert_eq!(plan.entries[0].epid, Some(102));
        Ok(())
    }

    #[tokio::test]
    async fn resolves_paginated_cheese_season_latest() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/view/web/season")
                .query_param("season_id", "202")
                .query_param_missing("pn");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "season_id": 202,
                    "title": "Course",
                    "subtitle": "Course subtitle",
                    "cover": "https://example.invalid/course.jpg",
                    "up_info": {"mid": 1, "uname": "Teacher"},
                    "episode_page": {"next": true, "num": 1, "size": 1, "total": 2},
                    "episodes": [{
                        "id": 101,
                        "aid": 170_001,
                        "cid": 9988,
                        "index": 1,
                        "title": "First lesson",
                        "release_date": 1_700_000_000
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/view/web/ep/list")
                .query_param("season_id", "202")
                .query_param("pn", "2")
                .query_param("ps", "1");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "page": {"next": false, "num": 2, "size": 1, "total": 2},
                    "items": [{
                        "id": 102,
                        "aid": 170_002,
                        "cid": 9989,
                        "index": 1,
                        "page": 2,
                        "title": "Second lesson",
                        "release_date": 1_700_000_001
                    }]
                }
            }));
        });

        let resolved = test_client(&server)
            .resolve_input("cheese/ss202", Some(Selection::Latest))
            .await?;
        match resolved {
            ResolvedContent::Season(season) => {
                assert_eq!(season.season.episodes.len(), 2);
                assert_eq!(season.selected_episodes[0].epid, 102);
                assert_eq!(season.selected_episodes[0].index, 2);
                assert_eq!(
                    season.selected_episodes[0].long_title.as_deref(),
                    Some("Second lesson")
                );
            }
            ResolvedContent::Video(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected season"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn pugv_pagination_uses_stable_page_size_when_size_is_missing() -> anyhow::Result<()> {
        let server = MockServer::start();
        for (page_number, episode_id, title, has_next) in [
            (1_u32, 101_u64, "First lesson", true),
            (2, 102, "Second lesson", true),
            (3, 103, "Third lesson", false),
        ] {
            server.mock(|when, then| {
                if page_number == 1 {
                    when.method(GET)
                        .path("/pugv/view/web/season")
                        .query_param("season_id", "202")
                        .query_param_missing("pn");
                    then.status(200).json_body_obj(&serde_json::json!({
                        "code": 0,
                        "data": {
                            "season_id": 202,
                            "title": "Course",
                            "episode_page": {"next": has_next, "num": page_number, "total": 3},
                            "episodes": [{
                                "id": episode_id,
                                "aid": 170_000 + episode_id,
                                "cid": 9_000 + episode_id,
                                "index": 1,
                                "page": page_number,
                                "title": title
                            }]
                        }
                    }));
                } else {
                    when.method(GET)
                        .path("/pugv/view/web/ep/list")
                        .query_param("season_id", "202")
                        .query_param("pn", page_number.to_string())
                        .query_param("ps", "1");
                    then.status(200).json_body_obj(&serde_json::json!({
                        "code": 0,
                        "data": {
                            "page": {"next": has_next, "num": page_number, "total": 3},
                            "items": [{
                                "id": episode_id,
                                "aid": 170_000 + episode_id,
                                "cid": 9_000 + episode_id,
                                "index": page_number,
                                "title": title
                            }]
                        }
                    }));
                }
            });
        }

        let resolved = test_client(&server)
            .resolve_input("cheese/ss202", Some(Selection::Latest))
            .await?;
        match resolved {
            ResolvedContent::Season(season) => {
                assert_eq!(season.season.episodes.len(), 3);
                assert_eq!(season.selected_episodes[0].epid, 103);
                assert_eq!(season.selected_episodes[0].index, 3);
            }
            ResolvedContent::Video(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected season"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn cheese_episode_non_current_selection_refetches_from_first_page() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/view/web/season")
                .query_param("ep_id", "102");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "season_id": 202,
                    "title": "Course",
                    "episode_page": {"next": false, "num": 2, "size": 1, "total": 2},
                    "episodes": [{
                        "id": 102,
                        "aid": 170_002,
                        "cid": 9989,
                        "index": 1,
                        "page": 2,
                        "title": "Second lesson"
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/view/web/season")
                .query_param("season_id", "202")
                .query_param_missing("pn");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "season_id": 202,
                    "title": "Course",
                    "episode_page": {"next": true, "num": 1, "size": 1, "total": 2},
                    "episodes": [{
                        "id": 101,
                        "aid": 170_001,
                        "cid": 9988,
                        "index": 1,
                        "title": "First lesson"
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/pugv/view/web/ep/list")
                .query_param("season_id", "202")
                .query_param("pn", "2")
                .query_param("ps", "1");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "page": {"next": false, "num": 2, "size": 1, "total": 2},
                    "items": [{
                        "id": 102,
                        "aid": 170_002,
                        "cid": 9989,
                        "index": 1,
                        "page": 2,
                        "title": "Second lesson"
                    }]
                }
            }));
        });

        let resolved = test_client(&server)
            .resolve_input("cheese/ep102", Some(Selection::Page(1)))
            .await?;
        match resolved {
            ResolvedContent::Season(season) => {
                assert_eq!(season.season.episodes.len(), 2);
                assert_eq!(season.selected_episodes[0].epid, 101);
            }
            ResolvedContent::Video(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected season"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn resolves_favorite_collection_and_plans_selected_item() -> anyhow::Result<()> {
        let server = MockServer::start();
        mock_favorite_collection(&server);
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/player/playurl")
                .query_param("avid", "170001")
                .query_param("cid", "9988");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "dash": {
                        "duration": 3,
                        "video": [{
                            "id": 80,
                            "baseUrl": "https://video.example/fav.m4s",
                            "base_url": "https://video.example/fav.m4s"
                        }],
                        "audio": [{
                            "id": 30280,
                            "baseUrl": "https://audio.example/fav.m4s",
                            "base_url": "https://audio.example/fav.m4s"
                        }]
                    }
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
                "data": {"subtitle": {"subtitles": []}}
            }));
        });

        let client = test_client(&server);
        let resolved = client.resolve_input("fav456", None).await?;
        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.title, "Favorite");
                assert_eq!(
                    collection.collection.kind,
                    crate::VideoCollectionKind::Favorite
                );
                assert_eq!(collection.selected_items[0].title, "Saved video");
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }

        let plan = client
            .plan_download("fav456", Some(Selection::Page(1)))
            .await?;
        assert_eq!(plan.title, "Favorite");
        assert_eq!(plan.entries[0].title, "Saved video");
        assert_eq!(plan.entries[0].streams.audios[0].id, 30280);
        Ok(())
    }

    #[tokio::test]
    async fn resolves_favorite_item_without_first_cid_from_video_metadata() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v3/fav/resource/list")
                .query_param("media_id", "456")
                .query_param("pn", "1")
                .query_param("ps", "20")
                .query_param("type", "0");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "info": {
                        "media_count": 1,
                        "title": "Favorite",
                        "upper": {"mid": 1, "name": "Tester"}
                    },
                    "medias": [{
                        "id": 170_001,
                        "type": 2,
                        "bvid": "BV1xx411c7mD",
                        "title": "Saved video",
                        "attr": 0,
                        "page": 1
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "title": "Saved video",
                    "owner": {"mid": 1, "name": "Tester"},
                    "pages": [{"page": 1, "cid": 9988, "part": "Saved video"}]
                }
            }));
        });

        let resolved = test_client(&server).resolve_input("fav456", None).await?;
        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.items.len(), 1);
                assert_eq!(collection.selected_items[0].cid, 9988);
                assert_eq!(collection.selected_items[0].title, "Saved video");
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn favorite_collection_accepts_null_media_list_as_empty() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v3/fav/resource/list")
                .query_param("media_id", "456")
                .query_param("pn", "1")
                .query_param("ps", "20")
                .query_param("type", "0");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "info": {
                        "media_count": 0,
                        "title": "Empty Favorite",
                        "upper": {"mid": 1, "name": "Tester"}
                    },
                    "medias": null
                }
            }));
        });

        let resolved = test_client(&server).resolve_input("fav456", None).await?;
        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.title, "Empty Favorite");
                assert!(collection.collection.items.is_empty());
                assert!(collection.selected_items.is_empty());
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn favorite_collection_skips_non_video_entries() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v3/fav/resource/list")
                .query_param("media_id", "456")
                .query_param("pn", "1")
                .query_param("ps", "20")
                .query_param("type", "0");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "info": {
                        "media_count": 2,
                        "title": "Favorite",
                        "upper": {"mid": 1, "name": "Tester"}
                    },
                    "medias": [{
                        "id": 990_001,
                        "type": 12,
                        "title": "Audio entry",
                        "attr": 0
                    }, {
                        "id": 170_001,
                        "type": 2,
                        "bvid": "BV1xx411c7mD",
                        "title": "Saved video",
                        "attr": 0,
                        "page": 1,
                        "ugc": {"first_cid": 9988}
                    }]
                }
            }));
        });

        let resolved = test_client(&server).resolve_input("fav456", None).await?;
        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.items.len(), 1);
                assert_eq!(collection.selected_items[0].aid, 170_001);
                assert_eq!(collection.selected_items[0].title, "Saved video");
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn resolve_collection_selection_keeps_full_items_while_plan_fetches_selected_item()
    -> anyhow::Result<()> {
        let resolve_server = MockServer::start();
        mock_favorite_collection_page(
            &resolve_server,
            1,
            21,
            170_001,
            "BV1xx411c7mD",
            "Newest video",
            9988,
        );
        mock_favorite_collection_page(
            &resolve_server,
            2,
            21,
            170_002,
            "BV1xx411c7mE",
            "Older video",
            9989,
        );

        let resolved = test_client(&resolve_server)
            .resolve_input("fav456", Some(Selection::Latest))
            .await?;
        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.items.len(), 2);
                assert_eq!(collection.collection.items[1].title, "Older video");
                assert_eq!(collection.selected_items.len(), 1);
                assert_eq!(collection.selected_items[0].title, "Newest video");
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }

        let plan_server = MockServer::start();
        mock_favorite_collection_page(
            &plan_server,
            1,
            21,
            170_001,
            "BV1xx411c7mD",
            "Newest video",
            9988,
        );
        server_mock_playurl(&plan_server, 170_001, 9988, "fav");
        server_mock_player_v2(&plan_server, 170_001, 9988);

        let plan = test_client(&plan_server)
            .plan_download("fav456", Some(Selection::Latest))
            .await?;
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].title, "Newest video");
        Ok(())
    }

    #[tokio::test]
    async fn resolves_space_series_with_owner_mid_endpoint() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/series/archives")
                .query_param("mid", "123")
                .query_param("series_id", "456")
                .query_param("pn", "1")
                .query_param("ps", "30");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "archives": [{
                        "aid": 170_001,
                        "bvid": "BV1xx411c7mD",
                        "title": "Series video",
                        "pic": "https://example.invalid/series.jpg",
                        "pubdate": 1_700_000_001,
                        "duration": 3
                    }],
                    "page": {"total": 1}
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/series/series")
                .query_param("series_id", "456");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "meta": {
                        "name": "Uploader series",
                        "description": "Series intro",
                        "cover": "https://example.invalid/series-cover.jpg",
                        "mid": 123,
                        "total": 1
                    }
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "title": "Series video",
                    "desc": "Series description",
                    "owner": {"mid": 123, "name": "Uploader"},
                    "pages": [{"page": 1, "cid": 9988, "part": "Series video", "duration": 3}]
                }
            }));
        });

        let resolved = test_client(&server)
            .resolve_input("https://space.bilibili.com/123/lists/456?type=series", None)
            .await?;

        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.kind, VideoCollectionKind::Series);
                assert_eq!(collection.collection.title, "Uploader series");
                assert_eq!(collection.collection.items.len(), 1);
                assert_eq!(collection.selected_items[0].cid, 9988);
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn space_collection_latest_requests_newest_first() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("mid", "123")
                .query_param("season_id", "456")
                .query_param("sort_reverse", "true")
                .query_param("page_num", "1")
                .query_param("page_size", "30");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "archives": [{
                        "aid": 170_001,
                        "bvid": "BV1xx411c7mD",
                        "title": "Newest collection video",
                        "pic": "https://example.invalid/collection.jpg",
                        "pubdate": 1_700_000_001,
                        "duration": 3
                    }],
                    "meta": {
                        "name": "Uploader collection",
                        "description": "Collection intro",
                        "cover": "https://example.invalid/collection-cover.jpg",
                        "mid": 123,
                        "total": 1
                    },
                    "page": {"total": 1}
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "title": "Newest collection video",
                    "desc": "Collection video description",
                    "owner": {"mid": 123, "name": "Uploader"},
                    "pages": [{"page": 1, "cid": 9988, "part": "Main", "duration": 3}]
                }
            }));
        });

        let resolved = test_client(&server)
            .resolve_input(
                "https://space.bilibili.com/123/lists/456?type=collection",
                Some(Selection::Latest),
            )
            .await?;

        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.kind, VideoCollectionKind::Collection);
                assert_eq!(
                    collection.selected_items[0].title,
                    "Newest collection video"
                );
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn resolves_space_videos_from_wbi_response_shape() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "wbi_img": {
                        "img_url": "https://i0.hdslb.com/bfs/wbi/0123456789abcdef0123456789abcdef.png",
                        "sub_url": "https://i0.hdslb.com/bfs/wbi/fedcba9876543210fedcba9876543210.png"
                    }
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/space/wbi/arc/search")
                .query_param("mid", "123")
                .query_param("pn", "1")
                .query_param("ps", "50");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "list": {
                        "vlist": [{
                            "aid": 170_001,
                            "bvid": "BV1xx411c7mD",
                            "pic": "https://example.invalid/space.jpg",
                            "created": 1_700_000_000
                        }, {
                            "aid": 170_002,
                            "bvid": "BV1xx411c7mE",
                            "pic": "https://example.invalid/unselected.jpg",
                            "created": 1_699_999_999
                        }]
                    },
                    "page": {"count": 2}
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "title": "Space video",
                    "desc": "Space description",
                    "owner": {"mid": 123, "name": "Uploader"},
                    "pages": [{"page": 1, "cid": 9988, "part": "Main", "duration": 3}]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170002");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_002,
                    "bvid": "BV1xx411c7mE",
                    "title": "Unselected space video",
                    "desc": "Unselected description",
                    "owner": {"mid": 123, "name": "Uploader"},
                    "pages": [{"page": 1, "cid": 9989, "part": "Unselected space video", "duration": 3}]
                }
            }));
        });

        let client = test_client(&server);
        let resolved = client
            .resolve_input("mid123", Some(Selection::Page(1)))
            .await?;

        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.kind, VideoCollectionKind::Space);
                assert_eq!(collection.selected_items[0].title, "Space video");
                assert_eq!(collection.selected_items[0].cid, 9988);
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn resolves_medialist_collection_after_filtered_cursor_page() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v1/medialist/info")
                .query_param("type", "8")
                .query_param("biz_id", "456");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "title": "Collection",
                    "intro": "Collection intro",
                    "cover": "https://example.invalid/collection.jpg"
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v2/medialist/resource/list")
                .query_param("biz_id", "456")
                .query_param("oid", "")
                .query_param("with_current", "true")
                .query_param("desc", "false");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "has_more": true,
                    "media_list": [{
                        "id": 170_000,
                        "title": "Filtered video",
                        "attr": 1,
                        "pages": [{"id": 9987, "page": 1, "title": "Filtered"}]
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v2/medialist/resource/list")
                .query_param("biz_id", "456")
                .query_param("oid", "170000")
                .query_param("with_current", "false")
                .query_param("desc", "false");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "has_more": false,
                    "media_list": [{
                        "id": 170_001,
                        "bv_id": "BV1xx411c7mD",
                        "title": "Visible video",
                        "intro": "Visible intro",
                        "cover": "https://example.invalid/visible.jpg",
                        "attr": 0,
                        "pages": [{"id": 9988, "page": 1, "title": "Main", "duration": 3}]
                    }]
                }
            }));
        });

        let client = test_client(&server);
        let resolved = client.resolve_input("collection456", None).await?;

        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.kind, VideoCollectionKind::Collection);
                assert_eq!(collection.collection.items.len(), 1);
                assert_eq!(collection.selected_items[0].title, "Visible video");
                assert_eq!(
                    collection.selected_items[0].bvid.as_deref(),
                    Some("BV1xx411c7mD")
                );
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn resolves_medialist_item_without_pages_from_video_metadata() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v1/medialist/info")
                .query_param("type", "8")
                .query_param("biz_id", "456");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "title": "Collection",
                    "intro": "Collection intro"
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v2/medialist/resource/list")
                .query_param("biz_id", "456")
                .query_param("oid", "")
                .query_param("with_current", "true")
                .query_param("desc", "false");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "has_more": false,
                    "media_list": [{
                        "id": 170_001,
                        "bvid": "BV1xx411c7mD",
                        "title": "Visible video",
                        "intro": "Visible intro",
                        "attr": 0
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "title": "Visible video",
                    "desc": "Video description",
                    "owner": {"mid": 1, "name": "Uploader"},
                    "pages": [{"page": 1, "cid": 9988, "part": "Visible video", "duration": 3}]
                }
            }));
        });

        let resolved = test_client(&server)
            .resolve_input("collection456", None)
            .await?;
        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.items.len(), 1);
                assert_eq!(collection.selected_items[0].cid, 9988);
                assert_eq!(collection.selected_items[0].title, "Visible video");
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn deduplicates_medialist_current_item_cursor_pages() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v1/medialist/info")
                .query_param("type", "8")
                .query_param("biz_id", "456");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "title": "Collection"
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v2/medialist/resource/list")
                .query_param("biz_id", "456")
                .query_param("oid", "")
                .query_param("with_current", "true")
                .query_param("desc", "false");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "has_more": true,
                    "media_list": [{
                        "id": 170_001,
                        "bvid": "BV1xx411c7mD",
                        "title": "First video",
                        "attr": 0,
                        "pages": [{"id": 9988, "page": 1, "title": "Main"}]
                    }]
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v2/medialist/resource/list")
                .query_param("biz_id", "456")
                .query_param("oid", "170001")
                .query_param("with_current", "false")
                .query_param("desc", "false");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "has_more": false,
                    "media_list": [{
                        "id": 170_001,
                        "bvid": "BV1xx411c7mD",
                        "title": "First video",
                        "attr": 0,
                        "pages": [{"id": 9988, "page": 1, "title": "Main"}]
                    }, {
                        "id": 170_002,
                        "bvid": "BV1xx411c7mE",
                        "title": "Second video",
                        "attr": 0,
                        "pages": [{"id": 9989, "page": 1, "title": "Main"}]
                    }]
                }
            }));
        });

        let resolved = test_client(&server)
            .resolve_input("collection456", None)
            .await?;
        match resolved {
            ResolvedContent::Collection(collection) => {
                assert_eq!(collection.collection.items.len(), 2);
                assert_eq!(collection.collection.items[0].aid, 170_001);
                assert_eq!(collection.collection.items[1].aid, 170_002);
                assert_eq!(collection.collection.items[1].index, 2);
            }
            ResolvedContent::Video(_) | ResolvedContent::Season(_) => {
                return Err(anyhow::anyhow!("expected collection"));
            }
        }
        Ok(())
    }

    #[test]
    fn collection_latest_selects_first_parsed_item() -> anyhow::Result<()> {
        let collection = VideoCollectionMetadata {
            id: Some(456),
            kind: VideoCollectionKind::Favorite,
            title: "Favorite".to_owned(),
            description: String::new(),
            cover_url: None,
            pub_time: None,
            owner: None,
            items: vec![
                collection_item(1, "Newest video"),
                collection_item(2, "Older video"),
            ],
        };

        let resolved =
            BiliClient::resolve_collection_selection(collection, Some(&Selection::Latest))?;

        assert_eq!(resolved.selected_items[0].title, "Newest video");
        Ok(())
    }

    #[test]
    fn video_index_selection_preserves_selector_order_and_deduplicates() -> anyhow::Result<()> {
        let video = VideoMetadata {
            aid: 170_001,
            bvid: Some("BV1xx411c7mD".to_owned()),
            title: "Multi page".to_owned(),
            description: String::new(),
            cover_url: None,
            pub_time: None,
            owner: None,
            tags: Vec::new(),
            pages: (1..=4).map(page_metadata).collect(),
        };
        let selection = IndexSelection::new([
            IndexSelector::range(3, 4),
            IndexSelector::index(1),
            IndexSelector::range(2, 3),
        ])?;

        let pages = BiliClient::select_video_pages(&video, Some(&Selection::Indices(selection)))?;

        assert_eq!(
            pages.iter().map(|page| page.index).collect::<Vec<_>>(),
            [3, 4, 1, 2]
        );
        Ok(())
    }

    #[test]
    fn video_index_selection_rejects_partial_hits() -> anyhow::Result<()> {
        let video = VideoMetadata {
            aid: 170_001,
            bvid: Some("BV1xx411c7mD".to_owned()),
            title: "Multi page".to_owned(),
            description: String::new(),
            cover_url: None,
            pub_time: None,
            owner: None,
            tags: Vec::new(),
            pages: (1..=3).map(page_metadata).collect(),
        };
        let selection = IndexSelection::new([IndexSelector::index(1), IndexSelector::index(999)])?;

        let Err(error) =
            BiliClient::select_video_pages(&video, Some(&Selection::Indices(selection)))
        else {
            return Err(anyhow::anyhow!("partial index hits should fail"));
        };

        assert!(matches!(error, Error::MissingField("selected page")));
        Ok(())
    }

    #[test]
    fn season_index_selection_selects_episode_indices() -> anyhow::Result<()> {
        let season = SeasonMetadata {
            season_id: Some(123),
            media_id: Some(456),
            title: "A Season".to_owned(),
            description: String::new(),
            cover_url: None,
            main_episode_count: 4,
            areas: Vec::new(),
            tags: Vec::new(),
            episodes: (1..=4).map(episode_metadata).collect(),
        };
        let selection = IndexSelection::new([
            IndexSelector::index(2),
            IndexSelector::range(4, 4),
            IndexSelector::index(2),
        ])?;

        let resolved = BiliClient::resolve_season_selection(
            season,
            Some(&Selection::Indices(selection)),
            None,
            "season",
        )?;

        assert_eq!(
            resolved
                .selected_episodes
                .iter()
                .map(|episode| episode.index)
                .collect::<Vec<_>>(),
            [2, 4]
        );
        Ok(())
    }

    #[test]
    fn collection_index_selection_selects_list_and_ranges() -> anyhow::Result<()> {
        let collection = VideoCollectionMetadata {
            id: Some(456),
            kind: VideoCollectionKind::Favorite,
            title: "Favorite".to_owned(),
            description: String::new(),
            cover_url: None,
            pub_time: None,
            owner: None,
            items: vec![
                collection_item(1, "Newest video"),
                collection_item(2, "Middle video"),
                collection_item(3, "Older video"),
            ],
        };
        let selection = IndexSelection::new([
            IndexSelector::range(2, 3),
            IndexSelector::index(1),
            IndexSelector::index(2),
        ])?;

        let resolved = BiliClient::resolve_collection_selection(
            collection,
            Some(&Selection::Indices(selection)),
        )?;

        assert_eq!(
            resolved
                .selected_items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["Middle video", "Older video", "Newest video"]
        );
        Ok(())
    }

    #[test]
    fn collection_index_fetch_mode_stops_after_max_requested_index() -> anyhow::Result<()> {
        let selection = IndexSelection::new([IndexSelector::index(1), IndexSelector::range(3, 4)])?;
        let fetch_mode = BiliClient::collection_fetch_mode(Some(&Selection::Indices(selection)))?;

        assert!(!fetch_mode.is_satisfied_by(3));
        assert!(fetch_mode.is_satisfied_by(4));
        Ok(())
    }

    #[test]
    fn collection_all_selection_allows_empty_items() -> anyhow::Result<()> {
        let collection = VideoCollectionMetadata {
            id: Some(456),
            kind: VideoCollectionKind::Favorite,
            title: "Favorite".to_owned(),
            description: String::new(),
            cover_url: None,
            pub_time: None,
            owner: None,
            items: Vec::new(),
        };

        let default_resolved = BiliClient::resolve_collection_selection(collection.clone(), None)?;
        assert!(default_resolved.selected_items.is_empty());
        let all_resolved =
            BiliClient::resolve_collection_selection(collection, Some(&Selection::All))?;
        assert!(all_resolved.selected_items.is_empty());
        Ok(())
    }

    #[test]
    fn collection_current_selection_is_rejected() {
        let collection = VideoCollectionMetadata {
            id: Some(456),
            kind: VideoCollectionKind::Favorite,
            title: "Favorite".to_owned(),
            description: String::new(),
            cover_url: None,
            pub_time: None,
            owner: None,
            items: vec![collection_item(1, "Saved video")],
        };

        let error =
            BiliClient::resolve_collection_selection(collection, Some(&Selection::Current)).err();

        assert!(
            matches!(error, Some(Error::InvalidInput(message)) if message.contains("current selection"))
        );
    }

    #[tokio::test]
    async fn resolves_short_link_input_from_redirect_target() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/short");
            then.status(302).header("location", "/video/av170001");
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "aid": 170_001,
                    "title": "Redirected video",
                    "pages": [{"page": 1, "cid": 9988, "part": "Main"}]
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

        let client = test_client(&server);
        let resolved = client
            .resolve(
                Input::ShortLink(format!("{}/short", server.base_url())),
                None,
            )
            .await?;
        match resolved {
            ResolvedContent::Video(video) => assert_eq!(video.title, "Redirected video"),
            ResolvedContent::Season(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected video"));
            }
        }
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
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            credentials: Credentials {
                cookie: None,
                access_key: Some("TOKEN_SHOULD_REDACT_12345".to_owned()),
                tv_access_key: None,
            },
            restricted_area: RestrictedAreaConfig::default(),
            playurl_mode: PlayurlMode::Web,
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
            ResolvedContent::Video(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected season"));
            }
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
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            credentials: Credentials {
                cookie: None,
                access_key: Some("intl-token".to_owned()),
                tv_access_key: None,
            },
            restricted_area: RestrictedAreaConfig::default(),
            playurl_mode: PlayurlMode::Web,
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

    #[tokio::test]
    async fn plans_pgc_download_with_tv_playurl_mode() -> anyhow::Result<()> {
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
        let tv_playurl = server.mock(|when, then| {
            when.method(GET)
                .path("/pgc/player/api/playurltv")
                .query_param("access_key", "TV_ACCESS")
                .query_param("appkey", TV_PLAYURL_APPKEY)
                .query_param("cid", "100")
                .query_param("ep_id", "1000")
                .query_param("expire", "0")
                .query_param("mobi_app", "android_tv_yst")
                .query_param("object_id", "10")
                .query_param("platform", "android")
                .query_param("playurl_type", "1")
                .query_param_exists("ts")
                .query_param_exists("sign")
                .header_missing("cookie");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "dash": {
                        "duration": 456,
                        "video": [{
                            "id": 64,
                            "base_url": "https://tv.example/pgc-64.m4s",
                            "codecs": "hev1",
                            "bandwidth": 900,
                            "mime_type": "video/mp4"
                        }],
                        "audio": []
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

        let client = BiliClient::new(
            ClientConfig::default()
                .with_endpoints(
                    EndpointConfig::default()
                        .with_api_base(server.base_url())
                        .with_pgc_base(server.base_url())
                        .with_tv_api_base(server.base_url()),
                )
                .with_credentials(
                    Credentials::default()
                        .with_cookie("SESSDATA=WEB_COOKIE")
                        .with_tv_access_key("TV_ACCESS"),
                )
                .with_playurl_mode(PlayurlMode::Tv),
        );
        let plan = client.plan_download("ep1000", None).await?;

        tv_playurl.assert();
        let entry = &plan.entries[0];
        assert_eq!(entry.source, StreamSource::PgcTv);
        assert_eq!(entry.epid, Some(1000));
        assert_eq!(
            entry.streams.videos[0].base_url,
            "https://tv.example/pgc-64.m4s"
        );
        Ok(())
    }

    #[tokio::test]
    async fn plans_pgc_download_with_app_playurl_mode() -> anyhow::Result<()> {
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
        let app_response =
            app_playurl::test_play_view_response_frame("https://app.example/pgc-80.m4s")?;
        let app_playurl = server.mock(|when, then| {
            when.method(POST)
                .path(app_playurl::PGC_PLAYURL_PATH)
                .header("content-type", "application/grpc")
                .header("authorization", "identify_v1 APP_ACCESS")
                .header_exists("x-bili-metadata-bin")
                .header_missing("cookie");
            then.status(200).body(app_response.clone());
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

        let client = BiliClient::new(
            ClientConfig::default()
                .with_endpoints(
                    EndpointConfig::default()
                        .with_api_base(server.base_url())
                        .with_pgc_base(server.base_url())
                        .with_app_pgc_grpc_base(server.base_url()),
                )
                .with_credentials(
                    Credentials::default()
                        .with_cookie("SESSDATA=WEB_COOKIE")
                        .with_access_key("APP_ACCESS"),
                )
                .with_playurl_mode(PlayurlMode::App),
        );
        let plan = client.plan_download("ep1000", None).await?;

        app_playurl.assert();
        let entry = &plan.entries[0];
        assert_eq!(entry.source, StreamSource::PgcApp);
        assert_eq!(entry.epid, Some(1000));
        assert_eq!(
            entry.streams.videos[0].base_url,
            "https://app.example/pgc-80.m4s"
        );
        assert_eq!(entry.streams.videos[0].codecs.as_deref(), Some("hev1"));
        Ok(())
    }

    #[tokio::test]
    async fn pgc_app_streams_fall_back_to_restricted_area_proxy() -> anyhow::Result<()> {
        let server = MockServer::start();
        mock_pgc_episode_metadata(&server);
        let app_playurl = server.mock(|when, then| {
            when.method(POST)
                .path(app_playurl::PGC_PLAYURL_PATH)
                .header("content-type", "application/grpc")
                .header("authorization", "identify_v1 TV_ACCESS")
                .header_missing("cookie");
            then.status(200)
                .header("grpc-status", "7")
                .header("grpc-message", "area%20restricted");
        });
        let proxy_playurl = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy-playurl")
                .query_param("proxy_token", "PROXY_SECRET")
                .query_param("ep_id", "1000")
                .query_param("area", "hk")
                .query_param("access_key", "TV_ACCESS")
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
                                "base_url": "https://proxy.example/64.m4s",
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
        server_mock_player_v2(&server, 10, 100);

        let client = BiliClient::new(
            ClientConfig::default()
                .with_endpoints(
                    EndpointConfig::default()
                        .with_api_base(server.base_url())
                        .with_pgc_base(server.base_url())
                        .with_app_pgc_grpc_base(server.base_url()),
                )
                .with_credentials(Credentials::default().with_tv_access_key("TV_ACCESS"))
                .with_restricted_area(
                    RestrictedAreaConfig::default()
                        .with_area_hint(RestrictedArea::Hk)
                        .with_proxy(RestrictedAreaProxy::playurl(
                            format!(
                                "{}/proxy-playurl?proxy_token=PROXY_SECRET",
                                server.base_url()
                            ),
                            Some(RestrictedArea::Hk),
                        )),
                )
                .with_playurl_mode(PlayurlMode::App),
        );
        let plan = client.plan_download("ep1000", None).await?;

        app_playurl.assert();
        proxy_playurl.assert();
        let entry = &plan.entries[0];
        assert_eq!(entry.source, StreamSource::PgcProxy);
        assert_eq!(entry.diagnostics.attempts.len(), 2);
        assert_eq!(entry.diagnostics.attempts[0].source, StreamSource::PgcApp);
        assert_eq!(entry.diagnostics.attempts[1].source, StreamSource::PgcProxy);
        assert_eq!(entry.diagnostics.attempts[1].area.as_deref(), Some("hk"));
        assert_eq!(
            entry.streams.videos[0].base_url,
            "https://proxy.example/64.m4s"
        );
        Ok(())
    }

    #[test]
    fn endpoint_client_and_restricted_area_builders_configure_embedding_inputs() {
        let endpoints = EndpointConfig::default()
            .with_api_base("https://api.test")
            .with_pgc_base("https://pgc.test")
            .with_intl_base("https://intl.test")
            .with_comment_base("https://comment.test")
            .with_passport_base("https://passport.test")
            .with_tv_api_base("https://tv-api.test")
            .with_app_grpc_base("https://app-grpc.test")
            .with_app_pgc_grpc_base("https://app-pgc-grpc.test")
            .with_tv_passport_base("https://tv-passport.test")
            .with_tv_passport_poll_base("https://tv-poll.test");
        let restricted_area = RestrictedAreaConfig::default()
            .with_area_hint(RestrictedArea::Tw)
            .with_proxy(RestrictedAreaProxy::playurl(
                "https://generic.example/playurl",
                None,
            ))
            .with_proxies([RestrictedAreaProxy::bilibili_api(
                "https://tw.example/api",
                Some(RestrictedArea::Tw),
            )]);
        let credentials = Credentials::default()
            .with_cookie("SESSDATA=redacted")
            .with_access_key("access-key");

        let config = ClientConfig::default()
            .with_endpoints(endpoints)
            .with_credentials(credentials)
            .with_restricted_area(restricted_area)
            .with_playurl_mode(PlayurlMode::App)
            .with_user_agent("embedding-test/1.0")
            .with_request_timeout(Duration::from_secs(7));

        assert_eq!(config.endpoints.api_base, "https://api.test");
        assert_eq!(config.endpoints.tv_api_base, "https://tv-api.test");
        assert_eq!(config.endpoints.app_grpc_base, "https://app-grpc.test");
        assert_eq!(
            config.endpoints.app_pgc_grpc_base,
            "https://app-pgc-grpc.test"
        );
        assert_eq!(
            config.endpoints.tv_passport_poll_base,
            "https://tv-poll.test"
        );
        assert_eq!(config.credentials.access_key.as_deref(), Some("access-key"));
        assert_eq!(config.restricted_area.area_hint, Some(RestrictedArea::Tw));
        assert_eq!(config.restricted_area.proxies.len(), 2);
        assert_eq!(config.playurl_mode, PlayurlMode::App);
        assert_eq!(config.user_agent, "embedding-test/1.0");
        assert_eq!(config.request_timeout, Duration::from_secs(7));
    }

    #[test]
    fn restricted_area_proxies_are_ordered_by_hint_then_generic_then_area() {
        let config = RestrictedAreaConfig::new(
            Some(RestrictedArea::Hk),
            [
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
        );

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
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
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
            playurl_mode: PlayurlMode::Web,
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
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
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
            playurl_mode: PlayurlMode::Web,
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
                    tv_api_base: server.base_url(),
                    app_grpc_base: server.base_url(),
                    app_pgc_grpc_base: server.base_url(),
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
                playurl_mode: PlayurlMode::Web,
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
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
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
            playurl_mode: PlayurlMode::Web,
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

        let urls = BiliClient::pgc_proxy_playurl_urls(
            &proxy,
            10,
            100,
            1000,
            client.config.credentials.access_key.as_deref(),
        )?;
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
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
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
            playurl_mode: PlayurlMode::Web,
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
                tv_api_base: "http://127.0.0.1:1".to_owned(),
                app_grpc_base: "http://127.0.0.1:1".to_owned(),
                app_pgc_grpc_base: "http://127.0.0.1:1".to_owned(),
                tv_passport_base: "http://127.0.0.1:1".to_owned(),
                tv_passport_poll_base: "http://127.0.0.1:1".to_owned(),
            },
            credentials: Credentials::default(),
            restricted_area: RestrictedAreaConfig::default(),
            playurl_mode: PlayurlMode::Web,
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
            ResolvedContent::Video(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected season"));
            }
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
            ResolvedContent::Video(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected season"));
            }
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
            ResolvedContent::Video(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected season"));
            }
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
            ResolvedContent::Video(_) | ResolvedContent::Collection(_) => {
                return Err(anyhow::anyhow!("expected season"));
            }
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
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            credentials: Credentials::default(),
            restricted_area: RestrictedAreaConfig::default(),
            playurl_mode: PlayurlMode::Web,
            user_agent: "test".to_owned(),
            request_timeout: Duration::from_secs(30),
        })
    }

    fn mock_favorite_collection(server: &MockServer) {
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v3/fav/resource/list")
                .query_param("media_id", "456")
                .query_param("pn", "1")
                .query_param("ps", "20")
                .query_param("type", "0");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "info": {
                        "media_count": 1,
                        "title": "Favorite",
                        "intro": "Favorite intro",
                        "cover": "https://example.invalid/favorite.jpg",
                        "ctime": 1_700_000_000,
                        "upper": {"mid": 1, "name": "Tester"}
                    },
                    "medias": [{
                        "id": 170_001,
                        "type": 2,
                        "bvid": "BV1xx411c7mD",
                        "title": "Saved video",
                        "intro": "Saved intro",
                        "cover": "https://example.invalid/saved.jpg",
                        "pubtime": 1_700_000_001,
                        "duration": 3,
                        "attr": 0,
                        "page": 1,
                        "upper": {"mid": 1, "name": "Tester"},
                        "ugc": {"first_cid": 9988}
                    }]
                }
            }));
        });
    }

    fn mock_favorite_collection_page(
        server: &MockServer,
        page_number: u32,
        media_count: usize,
        aid: u64,
        bvid: &str,
        title: &str,
        cid: u64,
    ) {
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/v3/fav/resource/list")
                .query_param("media_id", "456")
                .query_param("pn", page_number.to_string())
                .query_param("ps", "20")
                .query_param("type", "0");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "info": {
                        "media_count": media_count,
                        "title": "Favorite",
                        "intro": "Favorite intro",
                        "cover": "https://example.invalid/favorite.jpg",
                        "ctime": 1_700_000_000,
                        "upper": {"mid": 1, "name": "Tester"}
                    },
                    "medias": [{
                        "id": aid,
                        "type": 2,
                        "bvid": bvid,
                        "title": title,
                        "intro": "Saved intro",
                        "cover": "https://example.invalid/saved.jpg",
                        "pubtime": 1_700_000_001,
                        "duration": 3,
                        "attr": 0,
                        "page": 1,
                        "upper": {"mid": 1, "name": "Tester"},
                        "ugc": {"first_cid": cid}
                    }]
                }
            }));
        });
    }

    fn mock_pgc_episode_metadata(server: &MockServer) {
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
    }

    fn server_mock_playurl(server: &MockServer, aid: u64, cid: u64, label: &str) {
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/player/playurl")
                .query_param("avid", aid.to_string())
                .query_param("cid", cid.to_string());
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "dash": {
                        "duration": 3,
                        "video": [{
                            "id": 80,
                            "baseUrl": format!("https://video.example/{label}.m4s"),
                            "base_url": format!("https://video.example/{label}.m4s")
                        }],
                        "audio": [{
                            "id": 30280,
                            "baseUrl": format!("https://audio.example/{label}.m4s"),
                            "base_url": format!("https://audio.example/{label}.m4s")
                        }]
                    }
                }
            }));
        });
    }

    fn server_mock_player_v2(server: &MockServer, aid: u64, cid: u64) {
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/player/v2")
                .query_param("aid", aid.to_string())
                .query_param("cid", cid.to_string());
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"subtitle": {"subtitles": []}}
            }));
        });
    }

    fn collection_item(index: u32, title: &str) -> VideoCollectionItem {
        VideoCollectionItem {
            index,
            aid: 170_000 + u64::from(index),
            bvid: Some(format!("BV1mock{index}")),
            cid: 9_000 + u64::from(index),
            title: title.to_owned(),
            cover_url: None,
            description: String::new(),
            pub_time: None,
            owner: None,
            duration_seconds: None,
        }
    }

    fn page_metadata(index: u32) -> PageMetadata {
        PageMetadata {
            index,
            aid: 170_001,
            cid: 9_000 + u64::from(index),
            epid: None,
            title: format!("P{index}"),
            duration_seconds: None,
        }
    }

    fn episode_metadata(index: u32) -> EpisodeMetadata {
        EpisodeMetadata {
            index,
            aid: 170_000 + u64::from(index),
            bvid: Some(format!("BV1episode{index}")),
            cid: 9_000 + u64::from(index),
            epid: 1_000 + u64::from(index),
            title: index.to_string(),
            long_title: Some(format!("Episode {index}")),
            pub_time: None,
        }
    }
}

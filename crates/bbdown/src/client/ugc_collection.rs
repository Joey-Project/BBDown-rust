use super::{
    ApiData, BiliClient, SpaceArchive, SpaceArchiveListData, SpaceArchiveMeta, SpaceListKind,
    SpaceSeriesMetaData, UgcCollectionKind, UgcCollectionReference, VideoCollectionItem,
    VideoCollectionMetadata, VideoCollectionResolution, ViewData, ViewUgcSeason,
};
use crate::{Error, IndexSelector, Result, Selection};
use reqwest::redirect::Policy;
use serde_json::Value;
use std::collections::HashSet;
use url::Url;

pub const DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RESPONSE_BYTES: usize = 1024 * 1024;
pub const DEFAULT_UGC_COLLECTION_RESOLVE_MAX_REQUESTS: usize = 10;
pub const DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RAW_ARCHIVES: usize = 100;
pub const DEFAULT_UGC_COLLECTION_RESOLVE_MAX_EXPANDED_PAGES: usize = 100;
pub const DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RESULT_BYTES: usize = 4 * 1024 * 1024;

pub const MAX_UGC_COLLECTION_RESOLVE_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_UGC_COLLECTION_RESOLVE_REQUESTS: usize = 512;
pub const MAX_UGC_COLLECTION_RESOLVE_RAW_ARCHIVES: usize = 4096;
pub const MAX_UGC_COLLECTION_RESOLVE_EXPANDED_PAGES: usize = 4096;
pub const MAX_UGC_COLLECTION_RESOLVE_RESULT_BYTES: usize = 64 * 1024 * 1024;

const SPACE_ARCHIVE_PAGE_SIZE: u32 = 30;

#[derive(Clone, Copy)]
struct CollectionRequest {
    owner_mid: u64,
    list_id: u64,
    kind: SpaceListKind,
    newest_first: bool,
}

struct ArchivePageRequest {
    collection: CollectionRequest,
    page_number: u32,
    remaining_archives: usize,
}

struct CollectionHeader {
    title: String,
    description: String,
    cover_url: Option<String>,
    pub_time: Option<i64>,
    owner_mid: u64,
}

impl CollectionHeader {
    fn new(
        meta: Option<&SpaceArchiveMeta>,
        request: CollectionRequest,
        result_bytes: &mut ResultByteBudget,
    ) -> Result<Self> {
        let title = meta.and_then(space_archive_title);
        let fallback_title_bytes = request
            .kind
            .title()
            .len()
            .checked_add(1)
            .and_then(|length| length.checked_add(decimal_digits(request.list_id)))
            .ok_or_else(|| limit_error("max_result_bytes", result_bytes.max_bytes))?;
        let header_bytes = sum_string_lengths([
            title.map_or(fallback_title_bytes, str::len),
            meta.and_then(|meta| meta.description.as_deref())
                .map_or(0, str::len),
            meta.and_then(|meta| meta.cover.as_deref())
                .map_or(0, str::len),
        ])?;
        result_bytes.reserve(header_bytes)?;

        Ok(Self {
            title: title.map_or_else(
                || format!("{} {}", request.kind.title(), request.list_id),
                str::to_owned,
            ),
            description: meta
                .and_then(|meta| meta.description.as_deref())
                .unwrap_or_default()
                .to_owned(),
            cover_url: meta
                .and_then(|meta| meta.cover.as_deref())
                .map(str::to_owned),
            pub_time: meta.and_then(SpaceArchiveMeta::pub_time),
            owner_mid: meta.and_then(|meta| meta.mid).unwrap_or(request.owner_mid),
        })
    }

    fn into_collection(
        self,
        request: CollectionRequest,
        items: Vec<VideoCollectionItem>,
    ) -> VideoCollectionMetadata {
        VideoCollectionMetadata {
            id: Some(request.list_id),
            kind: request.kind.into_collection_kind(),
            title: self.title,
            description: self.description,
            cover_url: self.cover_url,
            pub_time: self.pub_time,
            owner: Some(super::Owner {
                mid: self.owner_mid,
                name: String::new(),
            }),
            items,
        }
    }
}

/// Resource limits for the bounded UGC collection and series resolver.
///
/// Response bytes apply to each response. Result bytes count UTF-8 string bytes
/// in the returned collection header, collection items, and selected-item copies;
/// fixed struct and allocator overhead is not included. All limits must be
/// between one and their corresponding public hard ceiling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UgcCollectionResolveOptions {
    /// Maximum streamed body size for each API response.
    pub max_response_bytes: usize,
    /// Maximum total requests for one collection-resolution call.
    pub max_requests: usize,
    /// Maximum cumulative archive rows read from archive-page responses.
    pub max_raw_archives: usize,
    /// Maximum cumulative pages read from archive view responses.
    pub max_expanded_pages: usize,
    /// Maximum returned UTF-8 string bytes, including selected-item copies.
    pub max_result_bytes: usize,
}

impl Default for UgcCollectionResolveOptions {
    fn default() -> Self {
        Self {
            max_response_bytes: DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RESPONSE_BYTES,
            max_requests: DEFAULT_UGC_COLLECTION_RESOLVE_MAX_REQUESTS,
            max_raw_archives: DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RAW_ARCHIVES,
            max_expanded_pages: DEFAULT_UGC_COLLECTION_RESOLVE_MAX_EXPANDED_PAGES,
            max_result_bytes: DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RESULT_BYTES,
        }
    }
}

impl UgcCollectionResolveOptions {
    fn validate(&self) -> Result<()> {
        validate_option(
            "max_response_bytes",
            self.max_response_bytes,
            MAX_UGC_COLLECTION_RESOLVE_RESPONSE_BYTES,
        )?;
        validate_option(
            "max_requests",
            self.max_requests,
            MAX_UGC_COLLECTION_RESOLVE_REQUESTS,
        )?;
        validate_option(
            "max_raw_archives",
            self.max_raw_archives,
            MAX_UGC_COLLECTION_RESOLVE_RAW_ARCHIVES,
        )?;
        validate_option(
            "max_expanded_pages",
            self.max_expanded_pages,
            MAX_UGC_COLLECTION_RESOLVE_EXPANDED_PAGES,
        )?;
        validate_option(
            "max_result_bytes",
            self.max_result_bytes,
            MAX_UGC_COLLECTION_RESOLVE_RESULT_BYTES,
        )
    }
}

impl BiliClient {
    /// Queries a video’s UGC membership with bounded response and page parsing.
    ///
    /// Membership is one request. The collection request budget is not used by
    /// this method, while its response, page-array, and result-byte limits apply.
    pub async fn resolve_video_collection_membership_with_options(
        &self,
        bvid: &str,
        options: &UgcCollectionResolveOptions,
    ) -> Result<Option<UgcCollectionReference>> {
        options.validate()?;
        let client = self.with_ugc_collection_limits(options)?;
        let mut requests = RequestBudget::new(1);
        let mut url =
            Self::endpoint_url(&client.config.endpoints.api_base, "/x/web-interface/view")?;
        url.query_pairs_mut().append_pair("bvid", bvid);
        let value = client
            .get_bounded_json_value(url, &mut requests, options)
            .await?;
        check_data_array_limit(
            &value,
            "pages",
            options.max_expanded_pages,
            "max_expanded_pages",
            options.max_expanded_pages,
        )?;
        let response: ApiData<ViewData> = serde_json::from_value(value)?;
        let season = response.into_data()?.ugc_season;
        if let Some(season) = &season {
            let result_bytes = sum_string_lengths([
                season.title.as_deref().map_or(0, str::len),
                season.intro.as_deref().map_or(0, str::len),
                season.cover.as_deref().map_or(0, str::len),
            ])?;
            if result_bytes > options.max_result_bytes {
                return Err(limit_error("max_result_bytes", options.max_result_bytes));
            }
        }
        season.map(ViewUgcSeason::into_reference).transpose()
    }

    /// Resolves a UGC collection or series without silently returning partial data.
    ///
    /// Every request and response array is bounded before typed item expansion.
    /// Results that exceed any limit fail with a stable `Error::InvalidInput`.
    pub async fn resolve_video_collection_with_options(
        &self,
        reference: &UgcCollectionReference,
        selection: Option<Selection>,
        options: &UgcCollectionResolveOptions,
    ) -> Result<VideoCollectionResolution> {
        options.validate()?;
        Self::collection_info_fetch_mode(selection.as_ref())?;
        let kind = match reference.kind {
            UgcCollectionKind::Collection => SpaceListKind::Collection,
            UgcCollectionKind::Series => SpaceListKind::Series,
        };
        let client = self.with_ugc_collection_limits(options)?;
        let mut requests = RequestBudget::new(options.max_requests);
        client
            .fetch_space_list_collection_bounded(
                reference.owner_mid,
                reference.id,
                kind,
                selection.as_ref(),
                options,
                &mut requests,
            )
            .await
    }

    fn with_ugc_collection_limits(&self, options: &UgcCollectionResolveOptions) -> Result<Self> {
        let http = reqwest::Client::builder()
            .redirect(Policy::none())
            .build()
            .map_err(Self::http_error_without_url)?;
        Ok(Self {
            http,
            danmaku_http: self.danmaku_http.clone(),
            config: self.config.clone(),
            json_body_limit: Some(options.max_response_bytes),
            dynamic_device_cookie_cache: self.dynamic_device_cookie_cache.clone(),
        })
    }

    async fn get_bounded_json_value(
        &self,
        url: Url,
        requests: &mut RequestBudget,
        options: &UgcCollectionResolveOptions,
    ) -> Result<Value> {
        requests.reserve()?;
        self.get_json(url)
            .await
            .map_err(|error| map_response_limit_error(error, options.max_response_bytes))
    }

    async fn fetch_space_list_collection_bounded(
        &self,
        owner_mid: u64,
        list_id: u64,
        kind: SpaceListKind,
        selection: Option<&Selection>,
        options: &UgcCollectionResolveOptions,
        requests: &mut RequestBudget,
    ) -> Result<VideoCollectionResolution> {
        let collection_request = CollectionRequest {
            owner_mid,
            list_id,
            kind,
            newest_first: matches!(selection, Some(Selection::Latest)),
        };
        let (archives, meta, total_count) = self
            .fetch_first_archive_page_bounded(collection_request, options, requests)
            .await?;
        let mut result_bytes = ResultByteBudget::new(options.max_result_bytes);
        let header = CollectionHeader::new(meta.as_ref(), collection_request, &mut result_bytes)?;
        let items = self
            .fetch_all_archive_items_bounded(
                collection_request,
                total_count,
                archives,
                options,
                requests,
                &mut result_bytes,
            )
            .await?;
        let collection = header.into_collection(collection_request, items);
        let selected_positions = selected_item_positions(&collection.items, selection)?;
        for position in selected_positions {
            result_bytes.reserve(collection.items[position].string_bytes()?)?;
        }
        BiliClient::resolve_collection_selection(collection, selection)
    }

    async fn fetch_first_archive_page_bounded(
        &self,
        request: CollectionRequest,
        options: &UgcCollectionResolveOptions,
        requests: &mut RequestBudget,
    ) -> Result<(Vec<SpaceArchive>, Option<SpaceArchiveMeta>, usize)> {
        let first = self
            .fetch_space_archive_page_bounded(
                ArchivePageRequest {
                    collection: request,
                    page_number: 1,
                    remaining_archives: options.max_raw_archives,
                },
                options,
                requests,
            )
            .await?;
        let meta = match request.kind {
            SpaceListKind::Collection => first.meta,
            SpaceListKind::Series => {
                self.fetch_space_series_meta_bounded(request.list_id, options, requests)
                    .await?
                    .meta
            }
        };
        let total_count = bounded_archive_total(
            first.page.total,
            meta.as_ref().and_then(|meta| meta.total),
            first.archives.len(),
            options.max_raw_archives,
        )?;
        Ok((first.archives, meta, total_count))
    }

    async fn fetch_all_archive_items_bounded(
        &self,
        request: CollectionRequest,
        total_count: usize,
        mut archives: Vec<SpaceArchive>,
        options: &UgcCollectionResolveOptions,
        requests: &mut RequestBudget,
        result_bytes: &mut ResultByteBudget,
    ) -> Result<Vec<VideoCollectionItem>> {
        let mut raw_archives_seen = archives.len();
        let mut expanded_pages = 0_usize;
        let mut items = Vec::new();
        let total_pages = total_count.div_ceil(SPACE_ARCHIVE_PAGE_SIZE as usize);
        let mut page_number = 1_u32;
        loop {
            for archive in archives {
                self.append_space_archive_items_bounded(
                    &mut items,
                    archive,
                    options,
                    requests,
                    &mut expanded_pages,
                    result_bytes,
                )
                .await?;
            }
            if (page_number as usize) >= total_pages {
                break;
            }
            page_number += 1;
            let next = self
                .fetch_space_archive_page_bounded(
                    ArchivePageRequest {
                        collection: request,
                        page_number,
                        remaining_archives: options.max_raw_archives - raw_archives_seen,
                    },
                    options,
                    requests,
                )
                .await?;
            let _ = bounded_archive_total(
                next.page.total,
                next.meta.as_ref().and_then(|meta| meta.total),
                next.archives.len(),
                options.max_raw_archives,
            )?;
            raw_archives_seen = raw_archives_seen
                .checked_add(next.archives.len())
                .ok_or_else(|| limit_error("max_raw_archives", options.max_raw_archives))?;
            if raw_archives_seen > options.max_raw_archives {
                return Err(limit_error("max_raw_archives", options.max_raw_archives));
            }
            archives = next.archives;
        }
        super::renumber_collection_items(&mut items);
        Ok(items)
    }

    async fn fetch_space_archive_page_bounded(
        &self,
        request: ArchivePageRequest,
        options: &UgcCollectionResolveOptions,
        requests: &mut RequestBudget,
    ) -> Result<SpaceArchiveListData> {
        let collection = request.collection;
        let path = match collection.kind {
            SpaceListKind::Collection => "/x/polymer/web-space/seasons_archives_list",
            SpaceListKind::Series => "/x/series/archives",
        };
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, path)?;
        match collection.kind {
            SpaceListKind::Collection => {
                url.query_pairs_mut()
                    .append_pair("mid", &collection.owner_mid.to_string())
                    .append_pair("season_id", &collection.list_id.to_string())
                    .append_pair(
                        "sort_reverse",
                        if collection.newest_first {
                            "true"
                        } else {
                            "false"
                        },
                    )
                    .append_pair("page_num", &request.page_number.to_string())
                    .append_pair("page_size", &SPACE_ARCHIVE_PAGE_SIZE.to_string());
            }
            SpaceListKind::Series => {
                url.query_pairs_mut()
                    .append_pair("mid", &collection.owner_mid.to_string())
                    .append_pair("current_mid", "0")
                    .append_pair("series_id", &collection.list_id.to_string())
                    .append_pair("only_normal", "true")
                    .append_pair("sort", "desc")
                    .append_pair("pn", &request.page_number.to_string())
                    .append_pair("ps", &SPACE_ARCHIVE_PAGE_SIZE.to_string());
            }
        }
        let value = self.get_bounded_json_value(url, requests, options).await?;
        check_data_array_limit(
            &value,
            "archives",
            request.remaining_archives,
            "max_raw_archives",
            options.max_raw_archives,
        )?;
        let response: ApiData<SpaceArchiveListData> = serde_json::from_value(value)?;
        response.into_data()
    }

    async fn fetch_space_series_meta_bounded(
        &self,
        list_id: u64,
        options: &UgcCollectionResolveOptions,
        requests: &mut RequestBudget,
    ) -> Result<SpaceSeriesMetaData> {
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/series/series")?;
        url.query_pairs_mut()
            .append_pair("series_id", &list_id.to_string());
        let value = self.get_bounded_json_value(url, requests, options).await?;
        let response: ApiData<SpaceSeriesMetaData> = serde_json::from_value(value)?;
        response.into_data()
    }

    async fn append_space_archive_items_bounded(
        &self,
        items: &mut Vec<VideoCollectionItem>,
        archive: SpaceArchive,
        options: &UgcCollectionResolveOptions,
        requests: &mut RequestBudget,
        expanded_pages: &mut usize,
        result_bytes: &mut ResultByteBudget,
    ) -> Result<()> {
        let archive_aid = archive
            .aid
            .ok_or(Error::MissingField("data.archives[].aid"))?;
        let remaining_pages = options.max_expanded_pages - *expanded_pages;
        let mut url = Self::endpoint_url(&self.config.endpoints.api_base, "/x/web-interface/view")?;
        url.query_pairs_mut()
            .append_pair("aid", &archive_aid.to_string());
        let value = self.get_bounded_json_value(url, requests, options).await?;
        check_data_array_limit(
            &value,
            "pages",
            remaining_pages,
            "max_expanded_pages",
            options.max_expanded_pages,
        )?;
        let response: ApiData<ViewData> = serde_json::from_value(value)?;
        let view = response.into_data()?;
        let page_count = view.pages.len();
        if page_count > remaining_pages {
            return Err(limit_error(
                "max_expanded_pages",
                options.max_expanded_pages,
            ));
        }
        *expanded_pages = expanded_pages
            .checked_add(page_count)
            .ok_or_else(|| limit_error("max_expanded_pages", options.max_expanded_pages))?;
        let aid = view.aid.ok_or(Error::MissingField("data.aid"))?;
        let base_title = archive.title.as_deref().unwrap_or(&view.title);
        let description = view.desc.as_deref().unwrap_or_default();
        let bvid = view.bvid.as_deref().or(archive.bvid.as_deref());
        let cover_url = view.pic.as_deref().or(archive.pic.as_deref());

        for page in view.pages {
            if items
                .iter()
                .any(|existing| existing.aid == aid && existing.cid == page.cid)
            {
                continue;
            }
            let page_title = page.part.as_deref().unwrap_or(&view.title);
            let title_bytes = if page_count == 1 {
                base_title.len()
            } else {
                collection_page_title_len(base_title, page.page, page_title)
                    .ok_or_else(|| limit_error("max_result_bytes", options.max_result_bytes))?
            };
            let item_bytes = sum_string_lengths([
                title_bytes,
                bvid.map_or(0, str::len),
                cover_url.map_or(0, str::len),
                description.len(),
                view.owner.as_ref().map_or(0, |owner| owner.name.len()),
            ])?;
            result_bytes.reserve(item_bytes)?;

            let title = if page_count == 1 {
                base_title.to_owned()
            } else {
                super::format_collection_page_title(base_title, page.page, page_title)
            };
            items.push(VideoCollectionItem {
                index: 0,
                aid,
                bvid: bvid.map(str::to_owned),
                cid: page.cid,
                title,
                cover_url: cover_url.map(str::to_owned),
                description: description.to_owned(),
                pub_time: view.pubdate.or(archive.pubdate).or(archive.ctime),
                owner: view.owner.as_ref().map(|owner| super::Owner {
                    mid: owner.mid,
                    name: owner.name.clone(),
                }),
                duration_seconds: page.duration.or(archive.duration),
            });
        }
        Ok(())
    }
}

fn validate_option(name: &str, value: usize, maximum: usize) -> Result<()> {
    if value == 0 || value > maximum {
        return Err(Error::InvalidInput(format!(
            "UGC collection resolution option {name} must be between 1 and {maximum}"
        )));
    }
    Ok(())
}

fn limit_error(name: &str, maximum: usize) -> Error {
    Error::InvalidInput(format!(
        "UGC collection resolution exceeded {name} limit ({maximum})"
    ))
}

fn map_response_limit_error(error: Error, max_response_bytes: usize) -> Error {
    match error {
        Error::InvalidInput(message)
            if message
                == format!(
                    "bounded metadata API response exceeded the {max_response_bytes}-byte limit"
                ) =>
        {
            limit_error("max_response_bytes", max_response_bytes)
        }
        other => other,
    }
}

fn check_data_array_limit(
    value: &Value,
    field: &str,
    remaining: usize,
    limit_name: &str,
    configured_limit: usize,
) -> Result<()> {
    if value
        .get("data")
        .and_then(|data| data.get(field))
        .and_then(Value::as_array)
        .is_some_and(|rows| rows.len() > remaining)
    {
        return Err(limit_error(limit_name, configured_limit));
    }
    Ok(())
}

fn sum_string_lengths(lengths: impl IntoIterator<Item = usize>) -> Result<usize> {
    lengths.into_iter().try_fold(0_usize, |sum, length| {
        sum.checked_add(length)
            .ok_or_else(|| Error::InvalidInput("UGC collection result size overflow".to_owned()))
    })
}

fn bounded_archive_total(
    page_total: Option<usize>,
    meta_total: Option<usize>,
    actual_count: usize,
    maximum: usize,
) -> Result<usize> {
    if page_total.is_some_and(|total| total > maximum)
        || meta_total.is_some_and(|total| total > maximum)
    {
        return Err(limit_error("max_raw_archives", maximum));
    }
    let total = page_total.or(meta_total).unwrap_or(actual_count);
    if total > maximum {
        return Err(limit_error("max_raw_archives", maximum));
    }
    Ok(total)
}

fn decimal_digits(mut value: u64) -> usize {
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

fn space_archive_title(meta: &SpaceArchiveMeta) -> Option<&str> {
    meta.name
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            meta.title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
        })
}

fn collection_page_title_len(base_title: &str, page_index: u32, page_title: &str) -> Option<usize> {
    if page_index == 1 && (page_title.is_empty() || page_title == base_title) {
        return Some(base_title.len());
    }
    let suffix_len = 2_usize
        .checked_add(decimal_digits(u64::from(page_index)))?
        .checked_add(usize::from(!page_title.is_empty()))?
        .checked_add(page_title.len())?;
    base_title.len().checked_add(suffix_len)
}

fn selected_item_positions(
    items: &[VideoCollectionItem],
    selection: Option<&Selection>,
) -> Result<Vec<usize>> {
    match selection {
        None | Some(Selection::All) => Ok((0..items.len()).collect()),
        Some(Selection::Latest) => items
            .first()
            .map(|_| vec![0])
            .ok_or(Error::MissingField("selected collection item")),
        Some(Selection::Page(page)) => items
            .iter()
            .position(|item| item.index == *page)
            .map(|position| vec![position])
            .ok_or(Error::MissingField("selected collection item")),
        Some(Selection::Indices(indices)) => {
            let available = items.iter().map(|item| item.index).collect::<HashSet<_>>();
            let mut positions = Vec::new();
            let mut seen = HashSet::new();
            for selector in indices.selectors() {
                validate_selector_matches(*selector, &available)?;
                for (position, item) in items.iter().enumerate() {
                    if selector.contains(item.index) && seen.insert(item.index) {
                        positions.push(position);
                    }
                }
            }
            Ok(positions)
        }
        Some(Selection::Episode(_)) => Err(Error::InvalidInput(
            "episode selection is only valid for PGC inputs".to_owned(),
        )),
        Some(Selection::Current) => Err(Error::InvalidInput(
            "current selection is only valid for inputs that identify a single current item"
                .to_owned(),
        )),
    }
}

fn validate_selector_matches(selector: IndexSelector, available: &HashSet<u32>) -> Result<()> {
    let matches = match selector {
        IndexSelector::Index(index) => available.contains(&index),
        IndexSelector::Range { start, end } => {
            let requested_count = u64::from(end) - u64::from(start) + 1;
            requested_count <= available.len() as u64
                && (start..=end).all(|index| available.contains(&index))
        }
    };
    if matches {
        Ok(())
    } else {
        Err(Error::MissingField("selected collection item"))
    }
}

struct RequestBudget {
    max_requests: usize,
    used: usize,
}

impl RequestBudget {
    fn new(max_requests: usize) -> Self {
        Self {
            max_requests,
            used: 0,
        }
    }

    fn reserve(&mut self) -> Result<()> {
        if self.used >= self.max_requests {
            return Err(limit_error("max_requests", self.max_requests));
        }
        self.used += 1;
        Ok(())
    }
}

struct ResultByteBudget {
    max_bytes: usize,
    used: usize,
}

impl ResultByteBudget {
    fn new(max_bytes: usize) -> Self {
        Self { max_bytes, used: 0 }
    }

    fn reserve(&mut self, bytes: usize) -> Result<()> {
        let Some(total) = self.used.checked_add(bytes) else {
            return Err(limit_error("max_result_bytes", self.max_bytes));
        };
        if total > self.max_bytes {
            return Err(limit_error("max_result_bytes", self.max_bytes));
        }
        self.used = total;
        Ok(())
    }
}

impl VideoCollectionItem {
    fn string_bytes(&self) -> Result<usize> {
        sum_string_lengths([
            self.bvid.as_deref().map_or(0, str::len),
            self.title.len(),
            self.cover_url.as_deref().map_or(0, str::len),
            self.description.len(),
            self.owner.as_ref().map_or(0, |owner| owner.name.len()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BiliClient, DEFAULT_UGC_COLLECTION_RESOLVE_MAX_EXPANDED_PAGES,
        DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RAW_ARCHIVES,
        DEFAULT_UGC_COLLECTION_RESOLVE_MAX_REQUESTS,
        DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RESPONSE_BYTES,
        DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RESULT_BYTES, MAX_UGC_COLLECTION_RESOLVE_EXPANDED_PAGES,
        MAX_UGC_COLLECTION_RESOLVE_RAW_ARCHIVES, MAX_UGC_COLLECTION_RESOLVE_REQUESTS,
        MAX_UGC_COLLECTION_RESOLVE_RESPONSE_BYTES, MAX_UGC_COLLECTION_RESOLVE_RESULT_BYTES,
        UgcCollectionResolveOptions,
    };
    use crate::{
        ClientConfig, EndpointConfig, Error, Selection, UgcCollectionKind, UgcCollectionReference,
    };
    use httpmock::{Method::GET, MockServer};
    use serde_json::{Value, json};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;
    use std::time::Duration;

    fn test_client(api_base: String) -> BiliClient {
        let endpoints = EndpointConfig::default().with_api_base(api_base);
        BiliClient::new(ClientConfig::default().with_endpoints(endpoints))
    }

    fn collection_reference(kind: UgcCollectionKind) -> UgcCollectionReference {
        UgcCollectionReference {
            id: 456,
            kind,
            owner_mid: 123,
            title: "Reference title".to_owned(),
            description: "Reference description".to_owned(),
            cover_url: None,
            item_count: 1,
        }
    }

    fn assert_limit_error<T: std::fmt::Debug>(result: &crate::Result<T>, name: &str) {
        let is_expected =
            matches!(result, Err(Error::InvalidInput(message)) if message.contains(name));
        assert!(is_expected, "expected {name} limit error, got {result:?}");
    }

    fn archive_page(archives: &Value, total: usize) -> Value {
        json!({
            "code": 0,
            "data": {
                "archives": archives,
                "meta": {"name": "Collection", "total": total},
                "page": {"total": total}
            }
        })
    }

    fn view_body(aid: u64, bvid: &str, title: &str, pages: &Value) -> Value {
        json!({
            "code": 0,
            "data": {
                "aid": aid,
                "bvid": bvid,
                "title": title,
                "desc": "description",
                "owner": {"mid": 123, "name": "Uploader"},
                "pages": pages
            }
        })
    }

    #[test]
    fn options_defaults_and_hard_ceilings_are_public_and_bounded() {
        let options = UgcCollectionResolveOptions::default();
        assert_eq!(options.max_response_bytes, 1024 * 1024);
        assert_eq!(options.max_requests, 10);
        assert_eq!(options.max_raw_archives, 100);
        assert_eq!(options.max_expanded_pages, 100);
        assert_eq!(options.max_result_bytes, 4 * 1024 * 1024);
        assert_eq!(
            DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RESPONSE_BYTES,
            1024 * 1024
        );
        assert_eq!(DEFAULT_UGC_COLLECTION_RESOLVE_MAX_REQUESTS, 10);
        assert_eq!(DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RAW_ARCHIVES, 100);
        assert_eq!(DEFAULT_UGC_COLLECTION_RESOLVE_MAX_EXPANDED_PAGES, 100);
        assert_eq!(
            DEFAULT_UGC_COLLECTION_RESOLVE_MAX_RESULT_BYTES,
            4 * 1024 * 1024
        );
        assert_eq!(MAX_UGC_COLLECTION_RESOLVE_RESPONSE_BYTES, 16 * 1024 * 1024);
        assert_eq!(MAX_UGC_COLLECTION_RESOLVE_REQUESTS, 512);
        assert_eq!(MAX_UGC_COLLECTION_RESOLVE_RAW_ARCHIVES, 4096);
        assert_eq!(MAX_UGC_COLLECTION_RESOLVE_EXPANDED_PAGES, 4096);
        assert_eq!(MAX_UGC_COLLECTION_RESOLVE_RESULT_BYTES, 64 * 1024 * 1024);

        let invalid = UgcCollectionResolveOptions {
            max_requests: MAX_UGC_COLLECTION_RESOLVE_REQUESTS + 1,
            ..options
        };
        assert!(matches!(invalid.validate(), Err(Error::InvalidInput(_))));
    }

    #[tokio::test]
    async fn bounded_membership_preserves_the_reference_fields() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("bvid", "BV1membership1");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {
                    "aid": 601,
                    "bvid": "BV1membership1",
                    "title": "成员视频",
                    "pages": [{"page": 1, "cid": 6011, "part": "ページ"}],
                    "ugc_season": {
                        "id": 456,
                        "title": "音楽合集",
                        "cover": "https://example.test/合集.jpg",
                        "mid": 123,
                        "intro": "説明",
                        "ep_count": 7,
                        "season_type": 1
                    }
                }
            }));
        });

        let reference = test_client(server.base_url())
            .resolve_video_collection_membership_with_options(
                "BV1membership1",
                &UgcCollectionResolveOptions::default(),
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("expected UGC membership"))?;
        assert_eq!(reference.id, 456);
        assert_eq!(reference.title, "音楽合集");
        assert_eq!(reference.description, "説明");
        assert_eq!(reference.item_count, 7);
        Ok(())
    }

    #[tokio::test]
    async fn bounded_requests_do_not_follow_redirects() {
        let server = MockServer::start();
        let redirect = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("bvid", "BV1redirect1");
            then.status(302).header("Location", "/redirect-target");
        });
        let target = server.mock(|when, then| {
            when.method(GET).path("/redirect-target");
            then.status(200)
                .json_body_obj(&json!({"code": 0, "data": {}}));
        });

        let _ = test_client(server.base_url())
            .resolve_video_collection_membership_with_options(
                "BV1redirect1",
                &UgcCollectionResolveOptions::default(),
            )
            .await;
        assert_eq!(redirect.calls(), 1);
        assert_eq!(target.calls(), 0);
    }

    #[tokio::test]
    async fn content_length_over_limit_fails_before_reading_or_parsing_body() -> anyhow::Result<()>
    {
        let (api_base, server) = start_header_only_content_length_response()?;
        let options = UgcCollectionResolveOptions {
            max_response_bytes: 64,
            ..UgcCollectionResolveOptions::default()
        };

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            test_client(api_base)
                .resolve_video_collection_membership_with_options("BV1oversize1", &options),
        )
        .await?;
        assert_limit_error(&result, "max_response_bytes");
        assert!(
            server.join().is_ok(),
            "local content-length response server panicked"
        );
        Ok(())
    }

    fn start_header_only_content_length_response() -> std::io::Result<(String, JoinHandle<()>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 65\r\nConnection: keep-alive\r\n\r\n",
            );
            let mut client_closed = [0_u8; 1];
            let _ = stream.read(&mut client_closed);
        });
        Ok((format!("http://{address}"), handle))
    }

    fn start_chunked_response(body: Vec<u8>) -> std::io::Result<(String, JoinHandle<()>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            );
            let chunk_header = format!("{:X}\r\n", body.len());
            let _ = stream.write_all(chunk_header.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.write_all(b"\r\n0\r\n\r\n");
        });
        Ok((format!("http://{address}"), handle))
    }

    #[tokio::test]
    async fn oversized_chunked_response_fails_before_json_parse() -> anyhow::Result<()> {
        let (api_base, server) = start_chunked_response(vec![b'x'; 65])?;
        let options = UgcCollectionResolveOptions {
            max_response_bytes: 64,
            ..UgcCollectionResolveOptions::default()
        };

        let result = test_client(api_base)
            .resolve_video_collection_membership_with_options("BV1chunked1", &options)
            .await;
        assert_limit_error(&result, "max_response_bytes");
        assert!(
            server.join().is_ok(),
            "local chunked response server panicked"
        );
        Ok(())
    }

    #[tokio::test]
    async fn archive_overreturn_is_rejected_before_view_hydration() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "1");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {
                    "archives": [null, null, null],
                    "meta": {"total": 1},
                    "page": {"total": 1}
                }
            }));
        });
        let view = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "1");
            then.status(200)
                .json_body_obj(&json!({"code": 0, "data": {}}));
        });
        let options = UgcCollectionResolveOptions {
            max_raw_archives: 2,
            ..UgcCollectionResolveOptions::default()
        };

        let result = test_client(server.base_url())
            .resolve_video_collection_with_options(
                &collection_reference(UgcCollectionKind::Collection),
                None,
                &options,
            )
            .await;
        assert_limit_error(&result, "max_raw_archives");
        assert_eq!(view.calls(), 0);
    }

    #[tokio::test]
    async fn huge_declared_total_stops_before_more_archive_pages_or_hydration() {
        let server = MockServer::start();
        let first_page = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "1");
            then.status(200)
                .json_body_obj(&archive_page(&json!([]), 2_000_000));
        });
        let next_page = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "2");
            then.status(200)
                .json_body_obj(&archive_page(&json!([]), 2_000_000));
        });
        let view = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/view");
            then.status(200)
                .json_body_obj(&json!({"code": 0, "data": {}}));
        });

        let result = test_client(server.base_url())
            .resolve_video_collection_with_options(
                &collection_reference(UgcCollectionKind::Collection),
                None,
                &UgcCollectionResolveOptions::default(),
            )
            .await;
        assert_limit_error(&result, "max_raw_archives");
        assert_eq!(first_page.calls(), 1);
        assert_eq!(next_page.calls(), 0);
        assert_eq!(view.calls(), 0);
    }

    #[tokio::test]
    async fn request_budget_stops_exactly_before_the_next_archive_page() {
        let server = MockServer::start();
        let first_page = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "1");
            then.status(200)
                .json_body_obj(&archive_page(&json!([]), 90));
        });
        let second_page = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "2");
            then.status(200)
                .json_body_obj(&archive_page(&json!([]), 90));
        });
        let third_page = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "3");
            then.status(200)
                .json_body_obj(&archive_page(&json!([]), 90));
        });
        let options = UgcCollectionResolveOptions {
            max_requests: 2,
            ..UgcCollectionResolveOptions::default()
        };

        let result = test_client(server.base_url())
            .resolve_video_collection_with_options(
                &collection_reference(UgcCollectionKind::Collection),
                None,
                &options,
            )
            .await;
        assert_limit_error(&result, "max_requests");
        assert_eq!(first_page.calls(), 1);
        assert_eq!(second_page.calls(), 1);
        assert_eq!(third_page.calls(), 0);
    }

    #[tokio::test]
    async fn page_array_limit_precedes_typed_expansion_and_later_hydration() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "1");
            then.status(200).json_body_obj(&archive_page(
                &json!([
                    {"aid": 101, "title": "First"},
                    {"aid": 102, "title": "Second"}
                ]),
                2,
            ));
        });
        let first_view = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "101");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {
                    "aid": 101,
                    "title": "First",
                    "pages": [{"cid": 1}, null]
                }
            }));
        });
        let second_view = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "102");
            then.status(200)
                .json_body_obj(&view_body(102, "BV2", "Second", &json!([])));
        });
        let options = UgcCollectionResolveOptions {
            max_expanded_pages: 1,
            ..UgcCollectionResolveOptions::default()
        };

        let result = test_client(server.base_url())
            .resolve_video_collection_with_options(
                &collection_reference(UgcCollectionKind::Collection),
                None,
                &options,
            )
            .await;
        assert_limit_error(&result, "max_expanded_pages");
        assert_eq!(first_view.calls(), 1);
        assert_eq!(second_view.calls(), 0);
    }

    #[tokio::test]
    async fn result_byte_budget_stops_description_fanout_before_item_clones() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "1");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {
                    "archives": [
                        {"aid": 201, "title": "Long shared description"},
                        {"aid": 202, "title": "Next archive"}
                    ],
                    "meta": {"name": "Unicode 合集", "description": "header"},
                    "page": {"total": 2}
                }
            }));
        });
        let description = "shared description ".repeat(5);
        let view = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "201");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {
                    "aid": 201,
                    "bvid": "BV1long1",
                    "title": "Shared title",
                    "desc": description,
                    "owner": {"mid": 123, "name": "Uploader"},
                    "pages": [
                        {"page": 1, "cid": 2011, "part": "one"},
                        {"page": 2, "cid": 2012, "part": "two"},
                        {"page": 3, "cid": 2013, "part": "three"}
                    ]
                }
            }));
        });
        let next_view = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "202");
            then.status(200)
                .json_body_obj(&view_body(202, "BV2", "Next", &json!([])));
        });
        let options = UgcCollectionResolveOptions {
            max_result_bytes: 240,
            ..UgcCollectionResolveOptions::default()
        };

        let result = test_client(server.base_url())
            .resolve_video_collection_with_options(
                &collection_reference(UgcCollectionKind::Collection),
                None,
                &options,
            )
            .await;
        assert_limit_error(&result, "max_result_bytes");
        assert_eq!(view.calls(), 1);
        assert_eq!(next_view.calls(), 0);
    }

    #[tokio::test]
    async fn collection_header_counts_toward_result_budget_before_hydration() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "1");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {
                    "archives": [{"aid": 701, "title": "Small item"}],
                    "meta": {
                        "name": "Header",
                        "description": "a header description that exceeds the result budget"
                    },
                    "page": {"total": 1}
                }
            }));
        });
        let view = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "701");
            then.status(200)
                .json_body_obj(&view_body(701, "BV701", "Item", &json!([])));
        });
        let options = UgcCollectionResolveOptions {
            max_result_bytes: 8,
            ..UgcCollectionResolveOptions::default()
        };

        let result = test_client(server.base_url())
            .resolve_video_collection_with_options(
                &collection_reference(UgcCollectionKind::Collection),
                None,
                &options,
            )
            .await;
        assert_limit_error(&result, "max_result_bytes");
        assert_eq!(view.calls(), 0);
    }

    #[tokio::test]
    async fn small_collection_preserves_order_dedup_selection_unicode_and_legacy_results()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "1");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {
                    "archives": [
                        {"aid": 301, "title": "第一曲"},
                        {"aid": 302, "title": "第二曲"},
                        {"aid": 301, "title": "duplicate"}
                    ],
                    "meta": {
                        "name": "東方🎹合集",
                        "description": "小さな曲集",
                        "cover": "https://example.test/集合.jpg",
                        "mid": 123,
                        "total": 3
                    },
                    "page": {"total": 3}
                }
            }));
        });
        for (aid, bvid, title, cid) in [
            (301, "BV1first", "第一曲", 3011),
            (302, "BV1second", "第二曲", 3021),
        ] {
            server.mock(move |when, then| {
                when.method(GET)
                    .path("/x/web-interface/view")
                    .query_param("aid", aid.to_string());
                then.status(200).json_body_obj(&view_body(
                    aid,
                    bvid,
                    title,
                    &json!([{"page": 1, "cid": cid, "part": title}]),
                ));
            });
        }
        let client = test_client(server.base_url());
        let reference = collection_reference(UgcCollectionKind::Collection);
        let selection = Some(Selection::Latest);
        let bounded = client
            .resolve_video_collection_with_options(
                &reference,
                selection.clone(),
                &UgcCollectionResolveOptions::default(),
            )
            .await?;
        let legacy = client
            .resolve_video_collection(&reference, selection)
            .await?;

        assert_eq!(bounded, legacy);
        assert_eq!(bounded.collection.title, "東方🎹合集");
        assert_eq!(bounded.collection.description, "小さな曲集");
        assert_eq!(
            bounded
                .collection
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["第一曲", "第二曲"]
        );
        assert_eq!(bounded.collection.items.len(), 2);
        assert_eq!(
            bounded.selected_items,
            [bounded.collection.items[0].clone()]
        );
        Ok(())
    }

    #[tokio::test]
    async fn bounded_series_resolution_keeps_series_metadata_and_owner_request_identity()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/series/archives")
                .query_param("mid", "123")
                .query_param("series_id", "456")
                .query_param("pn", "1");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {
                    "archives": [{"aid": 401, "title": "系列曲"}],
                    "page": {"total": 1}
                }
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/series/series")
                .query_param("series_id", "456");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {"meta": {
                    "name": "シリーズ",
                    "description": "シリーズ説明",
                    "cover": "https://example.test/series.jpg",
                    "mid": 999,
                    "total": 1
                }}
            }));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "401");
            then.status(200).json_body_obj(&view_body(
                401,
                "BV1series",
                "Series video",
                &json!([{"page": 1, "cid": 4011, "part": "Series page"}]),
            ));
        });

        let resolved = test_client(server.base_url())
            .resolve_video_collection_with_options(
                &collection_reference(UgcCollectionKind::Series),
                None,
                &UgcCollectionResolveOptions::default(),
            )
            .await?;
        assert_eq!(resolved.collection.title, "シリーズ");
        assert_eq!(resolved.collection.description, "シリーズ説明");
        assert_eq!(
            resolved.collection.owner.as_ref().map(|owner| owner.mid),
            Some(999)
        );
        Ok(())
    }

    #[tokio::test]
    async fn dropping_resolution_future_stops_before_later_archive_hydration() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-space/seasons_archives_list")
                .query_param("page_num", "1");
            then.status(200).json_body_obj(&archive_page(
                &json!([
                    {"aid": 501, "title": "First"},
                    {"aid": 502, "title": "Second"}
                ]),
                2,
            ));
        });
        let first_view = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "501");
            then.status(200)
                .delay(Duration::from_secs(2))
                .json_body_obj(&view_body(501, "BV1", "First", &json!([])));
        });
        let second_view = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "502");
            then.status(200)
                .json_body_obj(&view_body(502, "BV2", "Second", &json!([])));
        });
        let client = test_client(server.base_url());
        let reference = collection_reference(UgcCollectionKind::Collection);
        let options = UgcCollectionResolveOptions::default();
        let mut resolution =
            Box::pin(client.resolve_video_collection_with_options(&reference, None, &options));
        let first_view_requested = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if first_view.calls() > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        tokio::select! {
            result = &mut resolution => {
                return Err(anyhow::anyhow!("resolution finished before cancellation: {result:?}"));
            }
            result = first_view_requested => {
                result.map_err(|error| anyhow::anyhow!("first archive hydration request: {error}"))?;
            }
        }
        drop(resolution);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(second_view.calls(), 0);
        Ok(())
    }
}

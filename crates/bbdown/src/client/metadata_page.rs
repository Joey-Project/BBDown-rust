use super::{
    BiliClient, DynamicArchiveSeed, DynamicFeedItem, DynamicFeedKind, Owner, TagPolicy,
    VideoCollectionItem, VideoCollectionKind, VideoMetadata, dynamic_archive_seed,
    format_collection_page_title,
};
use crate::{Error, Input, Result};
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::num::NonZeroUsize;
use url::Url;

const MAX_ITEMS_LIMIT: usize = 4096;
const MAX_SOURCE_ITEMS_LIMIT: usize = 4096;
const MAX_REQUESTS_LIMIT: usize = 512;
const MAX_CURSOR_SEEN_ITEMS: usize = 4096;
const MAX_CURSOR_PROVIDER_OFFSETS: usize = 128;
const MAX_PROVIDER_OFFSET_BYTES: usize = 4096;
const MAX_CURSOR_RAW_RECORDS: usize = 4096;
const MAX_CURSOR_EXPANDED_PAGES: usize = 4096;
const CURSOR_VERSION: u8 = 1;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_OUTPUT_STRING_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataCollectionInfo {
    pub id: Option<u64>,
    pub kind: VideoCollectionKind,
    pub title: String,
    pub description: String,
    pub cover_url: Option<String>,
    pub pub_time: Option<i64>,
    pub owner: Option<Owner>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataConsistency {
    BestEffort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataStopReason {
    ItemLimit,
    SourceItemLimit,
    RequestLimit,
    End,
}

#[derive(Clone, Debug)]
pub struct MetadataPageOptions {
    pub max_items: NonZeroUsize,
    pub max_source_items: NonZeroUsize,
    pub max_requests: NonZeroUsize,
    pub cursor: Option<MetadataCursor>,
}

impl Default for MetadataPageOptions {
    fn default() -> Self {
        Self {
            max_items: NonZeroUsize::new(100).unwrap_or(NonZeroUsize::MIN),
            max_source_items: NonZeroUsize::new(100).unwrap_or(NonZeroUsize::MIN),
            max_requests: NonZeroUsize::new(10).unwrap_or(NonZeroUsize::MIN),
            cursor: None,
        }
    }
}

impl MetadataPageOptions {
    #[must_use]
    pub fn with_max_items(mut self, max_items: NonZeroUsize) -> Self {
        self.max_items = max_items;
        self
    }

    #[must_use]
    pub fn with_max_source_items(mut self, max_source_items: NonZeroUsize) -> Self {
        self.max_source_items = max_source_items;
        self
    }

    #[must_use]
    pub fn with_max_requests(mut self, max_requests: NonZeroUsize) -> Self {
        self.max_requests = max_requests;
        self
    }

    #[must_use]
    pub fn with_cursor(mut self, cursor: MetadataCursor) -> Self {
        self.cursor = Some(cursor);
        self
    }
}

#[derive(Clone, Debug)]
pub struct MetadataPage {
    pub collection: MetadataCollectionInfo,
    pub items: Vec<VideoCollectionItem>,
    pub next_cursor: Option<MetadataCursor>,
    pub requests_used: usize,
    pub source_items_examined: usize,
    pub stop_reason: MetadataStopReason,
    pub consistency: MetadataConsistency,
}

#[derive(Clone)]
pub struct MetadataCursor {
    state: CursorState,
}

impl fmt::Debug for MetadataCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MetadataCursor([REDACTED])")
    }
}

#[derive(Clone)]
struct CursorState {
    version: u8,
    input_binding: [u8; 32],
    api_base_binding: [u8; 32],
    cookie_binding: [u8; 32],
    offset: Option<String>,
    has_more: bool,
    feed_started: bool,
    seen_offsets: HashSet<String>,
    raw_records: VecDeque<Option<DynamicArchiveSeed>>,
    pending_seed: Option<DynamicArchiveSeed>,
    pending_expansion: Option<PendingExpansion>,
    seen_aids: HashSet<u64>,
    seen_items: HashSet<(u64, u64)>,
    expanded_pages: usize,
}

#[derive(Clone)]
struct PendingExpansion {
    seed: DynamicArchiveSeed,
    metadata: VideoMetadata,
    next_page_index: usize,
}

impl CursorState {
    fn new(input_binding: [u8; 32], api_base_binding: [u8; 32], cookie_binding: [u8; 32]) -> Self {
        Self {
            version: CURSOR_VERSION,
            input_binding,
            api_base_binding,
            cookie_binding,
            offset: None,
            has_more: true,
            feed_started: false,
            seen_offsets: HashSet::new(),
            raw_records: VecDeque::new(),
            pending_seed: None,
            pending_expansion: None,
            seen_aids: HashSet::new(),
            seen_items: HashSet::new(),
            expanded_pages: 0,
        }
    }

    fn has_pending_work(&self) -> bool {
        self.pending_seed.is_some()
            || !self.raw_records.is_empty()
            || self.pending_expansion.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SupportedInput {
    Following,
    Space { mid: u64 },
}

impl SupportedInput {
    fn parse(input: &Input) -> Result<Self> {
        match input {
            Input::FollowingFeed => Ok(Self::Following),
            Input::SpaceDynamic(mid) if *mid > 0 => Ok(Self::Space { mid: *mid }),
            Input::SpaceDynamic(_) => Err(Error::InvalidInput(
                "space dynamic metadata input mid must be greater than zero".to_owned(),
            )),
            _ => Err(Error::Unsupported(
                "bounded metadata pages currently support following and space dynamic feeds"
                    .to_owned(),
            )),
        }
    }

    fn binding(self) -> [u8; 32] {
        let text = match self {
            Self::Following => "following".to_owned(),
            Self::Space { mid } => format!("space_dynamic:{mid}"),
        };
        Sha256::digest(text.as_bytes()).into()
    }

    fn kind(self) -> DynamicFeedKind {
        match self {
            Self::Following => DynamicFeedKind::Following,
            Self::Space { mid } => DynamicFeedKind::Space { mid },
        }
    }

    fn collection_info(self) -> MetadataCollectionInfo {
        MetadataCollectionInfo {
            id: self.kind().collection_id(),
            kind: self.kind().collection_kind(),
            title: self.kind().collection_title(),
            description: self.kind().collection_description(),
            cover_url: None,
            pub_time: None,
            owner: match self {
                Self::Following => None,
                Self::Space { mid } => Some(Owner {
                    mid,
                    name: mid.to_string(),
                }),
            },
        }
    }
}

struct RequestBudget {
    limit: usize,
    used: usize,
}

struct PreparedMetadataPage {
    supported: SupportedInput,
    state: CursorState,
    client: BiliClient,
    collection: MetadataCollectionInfo,
}

impl RequestBudget {
    fn new(limit: usize) -> Self {
        Self { limit, used: 0 }
    }

    fn reserve(&mut self) -> bool {
        if self.used >= self.limit {
            return false;
        }
        self.used += 1;
        true
    }

    fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.used)
    }
}

pub(super) async fn resolve_metadata_page(
    client: &BiliClient,
    input: Input,
    mut options: MetadataPageOptions,
) -> Result<MetadataPage> {
    validate_limits(&options)?;
    let prepared = prepare_metadata_page(client, &input, options.cursor.take())?;
    let supported = prepared.supported;
    let mut state = prepared.state;
    let bounded_client = prepared.client;

    let mut budget = RequestBudget::new(options.max_requests.get());
    let mut source_items_examined = 0;
    let mut output_items = Vec::new();
    let mut output_string_bytes: usize = 0;
    let collection = prepared.collection;
    let mut mixin_key = None;

    loop {
        match collect_available_item(
            &mut state,
            &options,
            &mut output_items,
            &mut output_string_bytes,
        )? {
            ItemProgress::None => {}
            ItemProgress::Continue => continue,
            ItemProgress::Stop(stop_reason) => {
                return Ok(page_result(
                    collection,
                    output_items,
                    state,
                    budget.used,
                    source_items_examined,
                    stop_reason,
                ));
            }
        }

        if scan_next_source_record(
            &mut state,
            options.max_source_items.get(),
            &mut source_items_examined,
        ) == SourceProgress::Limited
        {
            return Ok(page_result(
                collection,
                output_items,
                state,
                budget.used,
                source_items_examined,
                MetadataStopReason::SourceItemLimit,
            ));
        }

        let had_pending_seed = state.pending_seed.is_some();
        if had_pending_seed && !fetch_pending_seed(&bounded_client, &mut state, &mut budget).await?
        {
            return Ok(page_result(
                collection,
                output_items,
                state,
                budget.used,
                source_items_examined,
                MetadataStopReason::RequestLimit,
            ));
        }
        if had_pending_seed {
            continue;
        }

        if !state.raw_records.is_empty() {
            continue;
        }
        if let Some(stop_reason) = stop_reason_before_fetch(
            &state,
            source_items_examined,
            options.max_source_items.get(),
        ) {
            return Ok(page_result(
                collection,
                output_items,
                state,
                budget.used,
                source_items_examined,
                stop_reason,
            ));
        }

        if !fetch_next_dynamic_page(
            &bounded_client,
            supported,
            &mut state,
            &mut budget,
            &mut mixin_key,
        )
        .await?
        {
            return Ok(page_result(
                collection,
                output_items,
                state,
                budget.used,
                source_items_examined,
                MetadataStopReason::RequestLimit,
            ));
        }
    }
}

fn prepare_metadata_page(
    client: &BiliClient,
    input: &Input,
    cursor: Option<MetadataCursor>,
) -> Result<PreparedMetadataPage> {
    let supported = SupportedInput::parse(input)?;
    let input_binding = supported.binding();
    let api_base_binding = api_base_binding(&client.config.endpoints.api_base)?;
    let cookie_binding = cookie_binding(client);
    let state = match cursor {
        Some(cursor) => {
            let state = cursor.state;
            validate_cursor(&state, input_binding, api_base_binding, cookie_binding)?;
            state
        }
        None => CursorState::new(input_binding, api_base_binding, cookie_binding),
    };
    validate_cursor_state(&state)?;

    // reqwest follows redirects by default. Disable that behavior for this bounded
    // operation so one reserved API attempt cannot silently fan out to other hosts.
    let bounded_client = BiliClient {
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        config: client.config.clone(),
        json_body_limit: Some(MAX_RESPONSE_BYTES),
        dynamic_device_cookie_cache: client.dynamic_device_cookie_cache.clone(),
    };
    Ok(PreparedMetadataPage {
        supported,
        state,
        client: bounded_client,
        collection: supported.collection_info(),
    })
}

fn stop_reason_before_fetch(
    state: &CursorState,
    source_items_examined: usize,
    source_limit: usize,
) -> Option<MetadataStopReason> {
    if !state.has_more {
        Some(MetadataStopReason::End)
    } else if source_items_examined >= source_limit {
        Some(MetadataStopReason::SourceItemLimit)
    } else {
        None
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ItemProgress {
    None,
    Continue,
    Stop(MetadataStopReason),
}

fn collect_available_item(
    state: &mut CursorState,
    options: &MetadataPageOptions,
    output_items: &mut Vec<VideoCollectionItem>,
    output_string_bytes: &mut usize,
) -> Result<ItemProgress> {
    let Some(item) = next_expanded_item(state)? else {
        return Ok(ItemProgress::None);
    };
    let identity = (item.aid, item.cid);
    if state.seen_items.contains(&identity) {
        return Ok(ItemProgress::Continue);
    }
    ensure_cursor_capacity(state)?;
    state.seen_items.insert(identity);
    let mut item = item;
    item.index = u32::try_from(state.seen_items.len())
        .map_err(|_| cursor_capacity_error("collection item index"))?;
    *output_string_bytes = output_string_bytes
        .checked_add(item_string_bytes(&item))
        .ok_or_else(|| cursor_capacity_error("output string bytes"))?;
    if *output_string_bytes > MAX_OUTPUT_STRING_BYTES {
        return Err(cursor_capacity_error("4 MiB output string bytes"));
    }
    output_items.push(item);
    if output_items.len() >= options.max_items.get() {
        let exhausted = !state.has_pending_work() && !state.has_more;
        let stop_reason = if exhausted {
            MetadataStopReason::End
        } else {
            MetadataStopReason::ItemLimit
        };
        return Ok(ItemProgress::Stop(stop_reason));
    }
    Ok(ItemProgress::Continue)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SourceProgress {
    Empty,
    Continue,
    Limited,
}

fn scan_next_source_record(
    state: &mut CursorState,
    source_limit: usize,
    source_items_examined: &mut usize,
) -> SourceProgress {
    if state.pending_seed.is_some() {
        return SourceProgress::Continue;
    }
    let Some(record) = state.raw_records.pop_front() else {
        return SourceProgress::Empty;
    };
    if *source_items_examined >= source_limit {
        state.raw_records.push_front(record);
        return SourceProgress::Limited;
    }
    *source_items_examined += 1;
    state.pending_seed = record;
    if state.pending_seed.is_some() {
        SourceProgress::Continue
    } else {
        SourceProgress::Empty
    }
}

async fn fetch_pending_seed(
    client: &BiliClient,
    state: &mut CursorState,
    budget: &mut RequestBudget,
) -> Result<bool> {
    let Some(seed) = state.pending_seed.clone() else {
        return Ok(true);
    };
    if state.seen_aids.contains(&seed.aid) {
        state.pending_seed = None;
        return Ok(true);
    }
    if state.seen_aids.len() >= MAX_CURSOR_SEEN_ITEMS {
        return Err(cursor_capacity_error("seen video identities"));
    }
    if !budget.reserve() {
        return Ok(false);
    }
    let metadata = client.fetch_video_by_aid(seed.aid, TagPolicy::Skip).await?;
    state.seen_aids.insert(seed.aid);
    state.pending_seed = None;
    extend_from_view(state, &seed, metadata)?;
    Ok(true)
}

async fn fetch_next_dynamic_page(
    client: &BiliClient,
    supported: SupportedInput,
    state: &mut CursorState,
    budget: &mut RequestBudget,
    mixin_key: &mut Option<String>,
) -> Result<bool> {
    let mut cookie_override = None;
    if let SupportedInput::Space { .. } = supported {
        let needs_nav = mixin_key.is_none();
        let downstream_requests = usize::from(needs_nav) + 1;
        match client
            .prepare_space_dynamic_cookie(|| {
                budget.remaining() >= downstream_requests.saturating_add(1) && budget.reserve()
            })
            .await?
        {
            super::DynamicDeviceCookiePreparation::Ready(cookie) => cookie_override = cookie,
            super::DynamicDeviceCookiePreparation::RequestLimit => return Ok(false),
        }
        if needs_nav {
            if budget.remaining() < 2 || !budget.reserve() {
                return Ok(false);
            }
            *mixin_key = Some(client.fetch_wbi_mixin_key().await?);
        }
    }
    if !budget.reserve() {
        return Ok(false);
    }
    let page = client
        .fetch_dynamic_feed_page(
            supported.kind(),
            state.offset.as_deref(),
            mixin_key.as_deref(),
            cookie_override.as_deref(),
        )
        .await?;
    accept_provider_page(state, page)?;
    Ok(true)
}

fn page_result(
    collection: MetadataCollectionInfo,
    items: Vec<VideoCollectionItem>,
    state: CursorState,
    requests_used: usize,
    source_items_examined: usize,
    stop_reason: MetadataStopReason,
) -> MetadataPage {
    let next_cursor = if stop_reason == MetadataStopReason::End {
        None
    } else {
        Some(MetadataCursor { state })
    };
    MetadataPage {
        collection,
        items,
        next_cursor,
        requests_used,
        source_items_examined,
        stop_reason,
        consistency: MetadataConsistency::BestEffort,
    }
}

fn validate_limits(options: &MetadataPageOptions) -> Result<()> {
    if options.max_items.get() > MAX_ITEMS_LIMIT {
        return Err(Error::InvalidInput(format!(
            "max_items exceeds bounded metadata limit {MAX_ITEMS_LIMIT}"
        )));
    }
    if options.max_source_items.get() > MAX_SOURCE_ITEMS_LIMIT {
        return Err(Error::InvalidInput(format!(
            "max_source_items exceeds bounded metadata limit {MAX_SOURCE_ITEMS_LIMIT}"
        )));
    }
    if options.max_requests.get() > MAX_REQUESTS_LIMIT {
        return Err(Error::InvalidInput(format!(
            "max_requests exceeds bounded metadata limit {MAX_REQUESTS_LIMIT}"
        )));
    }
    Ok(())
}

fn api_base_binding(raw: &str) -> Result<[u8; 32]> {
    let mut url = Url::parse(raw)?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(Sha256::digest(url.as_str().as_bytes()).into())
}

fn cookie_binding(client: &BiliClient) -> [u8; 32] {
    Sha256::digest(
        client
            .config
            .credentials
            .cookie
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    )
    .into()
}

fn validate_cursor(
    state: &CursorState,
    input_binding: [u8; 32],
    api_base_binding: [u8; 32],
    cookie_binding: [u8; 32],
) -> Result<()> {
    if state.version != CURSOR_VERSION {
        return Err(Error::InvalidInput(
            "metadata cursor version is unsupported".to_owned(),
        ));
    }
    if state.input_binding != input_binding {
        return Err(Error::InvalidInput(
            "metadata cursor does not match the requested input".to_owned(),
        ));
    }
    if state.api_base_binding != api_base_binding {
        return Err(Error::InvalidInput(
            "metadata cursor does not match the configured API base".to_owned(),
        ));
    }
    if state.cookie_binding != cookie_binding {
        return Err(Error::InvalidInput(
            "metadata cursor does not match the configured cookie context".to_owned(),
        ));
    }
    Ok(())
}

fn validate_cursor_state(state: &CursorState) -> Result<()> {
    if state.seen_offsets.len() > MAX_CURSOR_PROVIDER_OFFSETS
        || state.raw_records.len() > MAX_CURSOR_RAW_RECORDS
        || state.seen_aids.len() > MAX_CURSOR_SEEN_ITEMS
        || state.seen_items.len() > MAX_CURSOR_SEEN_ITEMS
        || state.expanded_pages > MAX_CURSOR_EXPANDED_PAGES
        || state.pending_expansion.as_ref().is_some_and(|expansion| {
            expansion.next_page_index > expansion.metadata.pages.len()
                || expansion.metadata.pages.len() > MAX_CURSOR_EXPANDED_PAGES
        })
    {
        return Err(cursor_capacity_error("cursor state"));
    }
    if state.has_more && state.feed_started && state.offset.is_none() {
        return Err(Error::InvalidInput(
            "metadata cursor is missing its provider offset".to_owned(),
        ));
    }
    if let Some(offset) = state.offset.as_ref()
        && (offset.is_empty()
            || offset.len() > MAX_PROVIDER_OFFSET_BYTES
            || state.seen_offsets.contains(offset))
    {
        return Err(Error::InvalidInput(
            "metadata cursor contains an invalid provider offset".to_owned(),
        ));
    }
    if state
        .seen_offsets
        .iter()
        .any(|offset| offset.is_empty() || offset.len() > MAX_PROVIDER_OFFSET_BYTES)
    {
        return Err(Error::InvalidInput(
            "metadata cursor contains an invalid provider offset".to_owned(),
        ));
    }
    Ok(())
}

fn accept_provider_page(state: &mut CursorState, page: super::DynamicFeedData) -> Result<()> {
    let next_offset = page.offset.filter(|offset| !offset.is_empty());
    let has_more = page.has_more.unwrap_or(false);
    if has_more {
        let Some(next_offset) = next_offset else {
            return Err(Error::InvalidInput(
                "dynamic feed reported more records without a provider offset".to_owned(),
            ));
        };
        if next_offset.len() > MAX_PROVIDER_OFFSET_BYTES {
            return Err(cursor_capacity_error("provider offset bytes"));
        }
        if state.offset.as_deref() == Some(next_offset.as_str())
            || state.seen_offsets.contains(&next_offset)
        {
            return Err(Error::InvalidInput(
                "dynamic feed provider offset repeated or formed a cycle".to_owned(),
            ));
        }
        if state.seen_offsets.len() >= MAX_CURSOR_PROVIDER_OFFSETS {
            return Err(cursor_capacity_error("provider offsets"));
        }
        if let Some(current) = state.offset.replace(next_offset) {
            state.seen_offsets.insert(current);
        }
        state.has_more = true;
    } else {
        state.offset = None;
        state.has_more = false;
    }
    state.feed_started = true;

    if page.items.len() > MAX_CURSOR_RAW_RECORDS.saturating_sub(state.raw_records.len()) {
        return Err(cursor_capacity_error("buffered provider records"));
    }
    state.raw_records.extend(
        page.items
            .into_iter()
            .map(|item| dynamic_archive_seed(item).filter(|seed| seed.aid > 0)),
    );
    Ok(())
}

fn extend_from_view(
    state: &mut CursorState,
    seed: &DynamicArchiveSeed,
    metadata: VideoMetadata,
) -> Result<()> {
    if metadata.aid == 0 || metadata.aid != seed.aid {
        return Err(Error::InvalidInput(
            "video view aid does not match the dynamic feed record".to_owned(),
        ));
    }
    if metadata.pages.len() > MAX_CURSOR_EXPANDED_PAGES.saturating_sub(state.expanded_pages) {
        return Err(cursor_capacity_error("expanded video pages"));
    }
    state.expanded_pages += metadata.pages.len();
    if !metadata.pages.is_empty() {
        state.pending_expansion = Some(PendingExpansion {
            seed: seed.clone(),
            metadata,
            next_page_index: 0,
        });
    }
    Ok(())
}

fn next_expanded_item(state: &mut CursorState) -> Result<Option<VideoCollectionItem>> {
    loop {
        let Some(expansion) = state.pending_expansion.as_ref() else {
            return Ok(None);
        };
        if expansion.next_page_index >= expansion.metadata.pages.len() {
            state.pending_expansion = None;
            continue;
        }
        let (item, is_last_page) = {
            let Some(expansion) = state.pending_expansion.as_mut() else {
                return Ok(None);
            };
            let page = expansion.metadata.pages[expansion.next_page_index].clone();
            expansion.next_page_index += 1;
            if page.cid == 0 {
                continue;
            }
            let page_count = u32::try_from(expansion.metadata.pages.len())
                .ok()
                .filter(|count| *count > 0)
                .unwrap_or(1);
            let title = if page_count == 1 {
                expansion.metadata.title.clone()
            } else {
                format_collection_page_title(&expansion.metadata.title, page.index, &page.title)
            };
            if title.trim().is_empty() {
                continue;
            }
            let item = VideoCollectionItem {
                index: 0,
                aid: page.aid,
                bvid: expansion
                    .metadata
                    .bvid
                    .clone()
                    .or_else(|| expansion.seed.bvid.clone()),
                cid: page.cid,
                title,
                cover_url: expansion
                    .metadata
                    .cover_url
                    .clone()
                    .or_else(|| expansion.seed.cover_url.clone()),
                description: expansion.metadata.description.clone(),
                pub_time: expansion.metadata.pub_time.or(expansion.seed.pub_time),
                owner: expansion
                    .metadata
                    .owner
                    .clone()
                    .or_else(|| expansion.seed.owner.clone()),
                duration_seconds: page.duration_seconds,
            };
            (
                item,
                expansion.next_page_index >= expansion.metadata.pages.len(),
            )
        };
        if is_last_page {
            state.pending_expansion = None;
        }
        return Ok(Some(item));
    }
}

fn item_string_bytes(item: &VideoCollectionItem) -> usize {
    item.bvid.as_ref().map_or(0, String::len)
        + item.title.len()
        + item.cover_url.as_ref().map_or(0, String::len)
        + item.description.len()
        + item.owner.as_ref().map_or(0, |owner| owner.name.len())
}

fn ensure_cursor_capacity(state: &CursorState) -> Result<()> {
    if state.seen_items.len() >= MAX_CURSOR_SEEN_ITEMS {
        return Err(cursor_capacity_error("seen collection items"));
    }
    Ok(())
}

fn cursor_capacity_error(label: &str) -> Error {
    Error::InvalidInput(format!(
        "metadata cursor exceeded its bounded {label} capacity"
    ))
}

// Keep this import used as an explicit compile-time guard that the dynamic provider
// records remain owned by this feature and never leak through the public cursor.
#[allow(dead_code)]
fn _provider_item_type_guard(_: DynamicFeedItem) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::validate_server_cookie_value;
    use crate::{
        ClientConfig, EndpointConfig, PgcWebPlayurlRoute, PlayurlMode, RestrictedAreaConfig,
        credentials::Credentials,
    };
    use httpmock::{Method::GET, MockServer};
    use serde_json::json;

    // Synthetic auth fixture from joey-private-v3: access-a.
    const AUTH_VALUE: &str = "codex_synth_v1_access_a";

    fn test_client(server: &MockServer) -> BiliClient {
        BiliClient::new(ClientConfig {
            endpoints: EndpointConfig {
                api_base: server.base_url(),
                pgc_base: server.base_url(),
                web_base: server.base_url(),
                intl_base: server.base_url(),
                intl_passport_base: server.base_url(),
                comment_base: server.base_url(),
                passport_base: server.base_url(),
                tv_api_base: server.base_url(),
                app_grpc_base: server.base_url(),
                app_pgc_grpc_base: server.base_url(),
                tv_passport_base: server.base_url(),
                tv_passport_poll_base: server.base_url(),
            },
            access_key_provider: None,
            credentials: Credentials::default(),
            restricted_area: RestrictedAreaConfig::default(),
            playurl_mode: PlayurlMode::Web,
            pgc_web_playurl_route: PgcWebPlayurlRoute::default(),
            user_agent: "metadata-test".to_owned(),
            request_timeout: std::time::Duration::from_secs(2),
        })
    }

    fn test_client_with_cookie(server: &MockServer, cookie: &str) -> BiliClient {
        let mut client = test_client(server);
        client.config.credentials.cookie = Some(cookie.to_owned());
        client
    }

    fn archive(aid: u64, _cid: u64, pages: usize) -> serde_json::Value {
        let value = json!({
            "type": "DYNAMIC_TYPE_AV",
            "modules": {
                "module_author": {"pub_ts": 1_700_000_000, "mid": 3, "name": "owner"},
                "module_dynamic": {
                    "major": {"type": "MAJOR_TYPE_ARCHIVE", "archive": {
                        "aid": aid, "bvid": format!("BV{aid}"), "cover": "cover"
                    }}
                }
            }
        });
        assert!(pages > 0);
        value
    }

    fn view(aid: u64, pages: &[(u64, &str)]) -> serde_json::Value {
        json!({"code": 0, "data": {
            "aid": aid, "bvid": format!("BV{aid}"), "title": format!("Video {aid}"),
            "desc": "description", "pages": pages.iter().enumerate().map(|(index, (cid, title))| json!({
                "page": index + 1, "cid": cid, "part": title, "duration": 10
            })).collect::<Vec<_>>()
        }})
    }

    fn dynamic_page(
        items: &[serde_json::Value],
        offset: Option<&str>,
        has_more: bool,
    ) -> serde_json::Value {
        json!({"code": 0, "data": {"items": items, "offset": offset, "has_more": has_more}})
    }

    fn nav_response() -> serde_json::Value {
        json!({"code": 0, "data": {"wbi_img": {
            "img_url": "https://i0.hdslb.com/bfs/wbi/0123456789abcdef0123456789abcdef.png",
            "sub_url": "https://i0.hdslb.com/bfs/wbi/fedcba9876543210fedcba9876543210.png"
        }}})
    }

    fn nonzero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap_or(NonZeroUsize::MIN)
    }

    #[test]
    fn server_cookie_values_reject_empty_oversized_and_invalid_octets() {
        for value in [Some(String::new()), Some("bad;value".to_owned())] {
            assert!(validate_server_cookie_value(value, "data.b_3").is_err());
        }
        assert!(validate_server_cookie_value(None, "data.b_3").is_err());
        assert!(validate_server_cookie_value(Some("x".repeat(257)), "data.b_3").is_err());
    }

    #[tokio::test]
    async fn space_device_cookie_init_is_budgeted_and_shared_with_client_clones()
    -> anyhow::Result<()> {
        // Synthetic ID fixtures: joey-private-v3 / id-a and id-b.
        const BUVID3: &str = "codex_synth_v1_id_a";
        const BUVID4: &str = "codex_synth_v1_id_b";
        let configured_cookie = format!("SESSDATA={AUTH_VALUE};buvid3=;buvid4= ;extra=preserved");
        let expected_feed_cookie =
            format!("SESSDATA={AUTH_VALUE};extra=preserved; buvid3={BUVID3}; buvid4={BUVID4}");
        let server = MockServer::start();
        let spi = server.mock(|when, then| {
            when.method(GET).path("/x/frontend/finger/spi");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {"b_3": BUVID3, "b_4": BUVID4}
            }));
        });
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&nav_response());
        });
        let feed = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/space")
                .header("cookie", expected_feed_cookie.as_str());
            then.status(200)
                .json_body_obj(&dynamic_page(&[archive(10, 100, 1)], None, false));
        });
        let view_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "10");
            then.status(200).json_body_obj(&view(10, &[(100, "one")]));
        });
        let client = test_client_with_cookie(&server, &configured_cookie);

        let insufficient = client
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default().with_max_requests(nonzero(2)),
            )
            .await?;
        assert_eq!(insufficient.requests_used, 0);
        assert_eq!(insufficient.stop_reason, MetadataStopReason::RequestLimit);
        assert_eq!(spi.calls(), 0);
        assert_eq!(nav.calls(), 0);
        assert_eq!(feed.calls(), 0);

        let first = client
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default().with_max_requests(nonzero(4)),
            )
            .await?;
        assert_eq!(first.requests_used, 4);
        assert_eq!(first.items.len(), 1);
        assert_eq!(spi.calls(), 1);
        assert_eq!(nav.calls(), 1);
        assert_eq!(feed.calls(), 1);
        assert_eq!(view_mock.calls(), 1);

        let clone = client.clone();
        let second = clone
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default().with_max_requests(nonzero(3)),
            )
            .await?;
        assert_eq!(second.requests_used, 3);
        assert_eq!(second.items.len(), 1);
        assert_eq!(spi.calls(), 1);
        assert_eq!(nav.calls(), 2);
        assert_eq!(feed.calls(), 2);
        assert_eq!(view_mock.calls(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn space_device_cookie_init_preserves_existing_names() -> anyhow::Result<()> {
        // Synthetic ID fixtures: joey-private-v3 / id-a and id-b.
        const EXISTING: &str = "codex_synth_v1_id_a";
        const SERVER_VALUE: &str = "codex_synth_v1_id_b";
        let cookie = format!(
            "buvid4=; SESSDATA={AUTH_VALUE};buvid3= ;buvid3={EXISTING};buvid4=;extra=preserved"
        );
        let server = MockServer::start();
        let spi = server.mock(|when, then| {
            when.method(GET).path("/x/frontend/finger/spi");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {"b_3": SERVER_VALUE, "b_4": SERVER_VALUE}
            }));
        });
        server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&nav_response());
        });
        let feed = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/space")
                .header(
                    "cookie",
                    format!(
                        "SESSDATA={AUTH_VALUE};buvid3={EXISTING};extra=preserved; buvid4={SERVER_VALUE}"
                    ),
                );
            then.status(200)
                .json_body_obj(&dynamic_page(&[], None, false));
        });
        let page = test_client_with_cookie(&server, &cookie)
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default().with_max_requests(nonzero(3)),
            )
            .await?;
        assert_eq!(page.stop_reason, MetadataStopReason::End);
        assert_eq!(page.requests_used, 3);
        assert_eq!(spi.calls(), 1);
        assert_eq!(feed.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn valid_device_pairs_remove_empty_duplicates_without_spi() -> anyhow::Result<()> {
        // Synthetic ID fixtures: joey-private-v3 / id-a and id-b.
        const BUVID3: &str = "codex_synth_v1_id_a";
        const BUVID4: &str = "codex_synth_v1_id_b";
        let cookie = format!(
            "SESSDATA={AUTH_VALUE};buvid3={BUVID3};buvid4={BUVID4};buvid3=;extra=preserved"
        );
        let server = MockServer::start();
        let spi = server.mock(|when, then| {
            when.method(GET).path("/x/frontend/finger/spi");
            then.status(500);
        });
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&nav_response());
        });
        let feed = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/space")
                .header(
                    "cookie",
                    format!(
                        "SESSDATA={AUTH_VALUE};buvid3={BUVID3};buvid4={BUVID4};extra=preserved"
                    ),
                );
            then.status(200)
                .json_body_obj(&dynamic_page(&[], None, false));
        });
        let page = test_client_with_cookie(&server, &cookie)
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default().with_max_requests(nonzero(2)),
            )
            .await?;
        assert_eq!(page.requests_used, 2);
        assert_eq!(page.stop_reason, MetadataStopReason::End);
        assert_eq!(spi.calls(), 0);
        assert_eq!(nav.calls(), 1);
        assert_eq!(feed.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn invalid_server_cookie_response_is_not_cached_and_can_be_retried() -> anyhow::Result<()>
    {
        // Synthetic ID fixtures: joey-private-v3 / id-a and id-b.
        const BUVID3: &str = "codex_synth_v1_id_a";
        const BUVID4: &str = "codex_synth_v1_id_b";
        let server = MockServer::start();
        let mut invalid_spi = server.mock(|when, then| {
            when.method(GET).path("/x/frontend/finger/spi");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {"b_3": "invalid;value", "b_4": BUVID4}
            }));
        });
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&nav_response());
        });
        let feed = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/space")
                .header(
                    "cookie",
                    format!("SESSDATA={AUTH_VALUE}; buvid3={BUVID3}; buvid4={BUVID4}"),
                );
            then.status(200)
                .json_body_obj(&dynamic_page(&[], None, false));
        });
        let client = test_client_with_cookie(&server, &format!("SESSDATA={AUTH_VALUE}"));

        let first_error = client
            .resolve_metadata_page(Input::SpaceDynamic(7), MetadataPageOptions::default())
            .await
            .err()
            .ok_or_else(|| anyhow::Error::msg("invalid SPI cookie value must fail"))?;
        assert!(
            matches!(first_error, Error::InvalidInput(message) if message.contains("not a valid server-issued cookie value"))
        );
        assert_eq!(invalid_spi.calls(), 1);
        assert_eq!(nav.calls(), 0);
        assert_eq!(feed.calls(), 0);

        invalid_spi.delete();
        let valid_spi = server.mock(|when, then| {
            when.method(GET).path("/x/frontend/finger/spi");
            then.status(200).json_body_obj(&json!({
                "code": 0,
                "data": {"b_3": BUVID3, "b_4": BUVID4}
            }));
        });
        let page = client
            .resolve_metadata_page(Input::SpaceDynamic(7), MetadataPageOptions::default())
            .await?;
        assert_eq!(page.stop_reason, MetadataStopReason::End);
        assert_eq!(page.requests_used, 3);
        assert_eq!(valid_spi.calls(), 1);
        assert_eq!(nav.calls(), 1);
        assert_eq!(feed.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn space_device_cookie_spi_is_bounded_and_does_not_follow_redirects() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        let spi = server.mock(|when, then| {
            when.method(GET).path("/x/frontend/finger/spi");
            then.status(302).header("Location", "/x/web-interface/nav");
        });
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&nav_response());
        });
        assert!(
            test_client_with_cookie(&server, &format!("SESSDATA={AUTH_VALUE}"))
                .resolve_metadata_page(Input::SpaceDynamic(7), MetadataPageOptions::default(),)
                .await
                .is_err()
        );
        assert_eq!(spi.calls(), 1);
        assert_eq!(nav.calls(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn space_device_cookie_spi_rejects_oversized_json_before_nav_or_feed()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let spi = server.mock(|when, then| {
            when.method(GET).path("/x/frontend/finger/spi");
            then.status(200).body(format!(
                "{{\"code\":0,\"data\":{{\"b_3\":\"id\",\"b_4\":\"id\",\"padding\":\"{}\"}}}}",
                "x".repeat(MAX_RESPONSE_BYTES)
            ));
        });
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&nav_response());
        });
        let feed = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/space");
            then.status(200)
                .json_body_obj(&dynamic_page(&[], None, false));
        });
        let err = test_client_with_cookie(&server, &format!("SESSDATA={AUTH_VALUE}"))
            .resolve_metadata_page(Input::SpaceDynamic(7), MetadataPageOptions::default())
            .await
            .err()
            .ok_or_else(|| anyhow::Error::msg("oversized SPI response must be rejected"))?;
        assert!(
            matches!(err, Error::InvalidInput(message) if message.contains("1048576-byte limit"))
        );
        assert_eq!(spi.calls(), 1);
        assert_eq!(nav.calls(), 0);
        assert_eq!(feed.calls(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn request_budget_counts_nav_feed_and_view() -> anyhow::Result<()> {
        let server = MockServer::start();
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&nav_response());
        });
        let feed = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/space");
            then.status(200)
                .json_body_obj(&dynamic_page(&[archive(10, 100, 1)], None, false));
        });
        let view_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "10");
            then.status(200).json_body_obj(&view(10, &[(100, "one")]));
        });
        let page = test_client(&server)
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default().with_max_requests(nonzero(3)),
            )
            .await?;
        assert_eq!(page.requests_used, 3);
        assert_eq!(page.source_items_examined, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.stop_reason, MetadataStopReason::End);
        assert_eq!(nav.calls(), 1);
        assert_eq!(feed.calls(), 1);
        assert_eq!(view_mock.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn request_budget_preserves_pending_view_for_next_call() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&nav_response());
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/space");
            then.status(200)
                .json_body_obj(&dynamic_page(&[archive(10, 100, 1)], None, false));
        });
        let view_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "10");
            then.status(200).json_body_obj(&view(10, &[(100, "one")]));
        });
        let client = test_client(&server);
        let first = client
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default().with_max_requests(nonzero(2)),
            )
            .await?;
        assert_eq!(first.requests_used, 2);
        assert_eq!(first.stop_reason, MetadataStopReason::RequestLimit);
        assert_eq!(first.source_items_examined, 1);
        assert_eq!(first.items.len(), 0);
        assert_eq!(view_mock.calls(), 0);

        let second = client
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default()
                    .with_cursor(
                        first
                            .next_cursor
                            .ok_or_else(|| anyhow::Error::msg("cursor after budget stop"))?,
                    )
                    .with_max_requests(nonzero(1)),
            )
            .await?;
        assert_eq!(second.requests_used, 1);
        assert_eq!(second.source_items_examined, 0);
        assert_eq!(second.items.len(), 1);
        assert_eq!(view_mock.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn buffered_pages_continue_without_nav_or_feed_and_keep_multipage_items()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let nav = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/nav");
            then.status(200).json_body_obj(&nav_response());
        });
        let feed = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/space");
            then.status(200)
                .json_body_obj(&dynamic_page(&[archive(10, 100, 2)], None, false));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "10");
            then.status(200)
                .json_body_obj(&view(10, &[(100, "one"), (101, "two")]));
        });
        let client = test_client(&server);
        let first = client
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default().with_max_items(nonzero(1)),
            )
            .await?;
        assert_eq!(first.items[0].cid, 100);
        let second = client
            .resolve_metadata_page(
                Input::SpaceDynamic(7),
                MetadataPageOptions::default()
                    .with_cursor(
                        first
                            .next_cursor
                            .ok_or_else(|| anyhow::Error::msg("second page cursor"))?,
                    )
                    .with_max_items(nonzero(1))
                    .with_max_requests(nonzero(1)),
            )
            .await?;
        assert_eq!(second.items[0].cid, 101);
        assert_eq!(second.requests_used, 0);
        assert_eq!(second.stop_reason, MetadataStopReason::End);
        assert_eq!(nav.calls(), 1);
        assert_eq!(feed.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn source_filtering_duplicates_and_empty_pages_consume_scan_budget() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        let feed = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/all")
                .query_param_missing("offset");
            then.status(200).json_body_obj(&dynamic_page(
                &[
                    json!({"type": "DYNAMIC_TYPE_FORWARD"}),
                    archive(10, 100, 1),
                    archive(10, 100, 1),
                ],
                Some("next"),
                true,
            ));
        });
        let next_feed = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/all")
                .query_param("offset", "next");
            then.status(200)
                .json_body_obj(&dynamic_page(&[], Some("final"), true));
        });
        let view_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "10");
            then.status(200).json_body_obj(&view(10, &[(100, "one")]));
        });
        let client = test_client(&server);
        let first = client
            .resolve_metadata_page(
                Input::FollowingFeed,
                MetadataPageOptions::default()
                    .with_max_source_items(nonzero(2))
                    .with_max_requests(nonzero(2)),
            )
            .await?;
        assert_eq!(first.source_items_examined, 2);
        assert_eq!(first.items.len(), 1);
        assert_eq!(feed.calls(), 1);
        assert_eq!(view_mock.calls(), 1);

        let second = client
            .resolve_metadata_page(
                Input::FollowingFeed,
                MetadataPageOptions::default()
                    .with_cursor(
                        first
                            .next_cursor
                            .ok_or_else(|| anyhow::Error::msg("buffered duplicate"))?,
                    )
                    .with_max_source_items(nonzero(2))
                    .with_max_requests(nonzero(1)),
            )
            .await?;
        assert_eq!(second.source_items_examined, 1);
        assert_eq!(second.requests_used, 1);
        assert_eq!(second.items.len(), 0);
        assert_eq!(second.stop_reason, MetadataStopReason::RequestLimit);
        assert_eq!(feed.calls(), 1);
        assert_eq!(next_feed.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn missing_repeated_and_cyclic_offsets_are_errors() -> anyhow::Result<()> {
        for (offset, has_more, expected) in [
            (None, true, "without a provider offset"),
            (Some("same"), true, "repeated or formed a cycle"),
        ] {
            let server = MockServer::start();
            let client = test_client(&server);
            server.mock(|when, then| {
                when.method(GET)
                    .path("/x/polymer/web-dynamic/v1/feed/all")
                    .query_param("offset", "same");
                then.status(200)
                    .json_body_obj(&dynamic_page(&[], offset, has_more));
            });
            let err = client
                .resolve_metadata_page(
                    Input::FollowingFeed,
                    MetadataPageOptions::default().with_cursor(MetadataCursor {
                        state: CursorState {
                            offset: Some("same".to_owned()),
                            feed_started: true,
                            ..CursorState::new(
                                SupportedInput::Following.binding(),
                                api_base_binding(&server.base_url())?,
                                cookie_binding(&client),
                            )
                        },
                    }),
                )
                .await
                .err()
                .ok_or_else(|| anyhow::Error::msg("bad provider cursor must fail"))?;
            assert!(matches!(err, Error::InvalidInput(message) if message.contains(expected)));
        }

        let server = MockServer::start();
        let client = test_client(&server);
        let mut state = CursorState::new(
            SupportedInput::Following.binding(),
            api_base_binding(&server.base_url())?,
            cookie_binding(&client),
        );
        state.feed_started = true;
        state.offset = Some("b".to_owned());
        state.seen_offsets.insert("a".to_owned());
        let repeated_page = serde_json::from_value(json!({
            "has_more": true,
            "offset": "a",
            "items": []
        }))?;
        let err = accept_provider_page(&mut state, repeated_page)
            .err()
            .ok_or_else(|| anyhow::Error::msg("offset cycle must fail"))?;
        assert!(matches!(err, Error::InvalidInput(message) if message.contains("cycle")));
        Ok(())
    }

    #[tokio::test]
    async fn cursor_bindings_and_unsupported_inputs_fail_before_network() -> anyhow::Result<()> {
        let server = MockServer::start();
        let feed = server.mock(|when, then| {
            when.method(GET).path("/x/polymer/web-dynamic/v1/feed/all");
            then.status(200)
                .json_body_obj(&dynamic_page(&[], None, false));
        });
        let client = test_client(&server);
        let cursor = MetadataCursor {
            state: CursorState::new(
                SupportedInput::Following.binding(),
                api_base_binding(&server.base_url())?,
                cookie_binding(&client),
            ),
        };
        let before = feed.calls();
        assert!(
            client
                .resolve_metadata_page(
                    Input::RelatedRecommendations(1),
                    MetadataPageOptions::default().with_cursor(cursor.clone()),
                )
                .await
                .is_err()
        );
        assert!(
            client
                .resolve_metadata_page(
                    Input::SpaceDynamic(7),
                    MetadataPageOptions::default().with_cursor(cursor.clone()),
                )
                .await
                .is_err()
        );
        assert!(
            client
                .resolve_metadata_page(
                    Input::SpaceDynamic(0),
                    MetadataPageOptions::default().with_cursor(cursor.clone()),
                )
                .await
                .is_err()
        );
        assert_eq!(feed.calls(), before);

        let mut other_base = test_client(&server);
        other_base.config.endpoints.api_base = "http://127.0.0.1:1".to_owned();
        assert!(
            other_base
                .resolve_metadata_page(
                    Input::FollowingFeed,
                    MetadataPageOptions::default().with_cursor(cursor.clone()),
                )
                .await
                .is_err()
        );

        let mut other_cookie = test_client(&server);
        /* synthetic-token fixture: joey-private-v3 / api-key-a */
        other_cookie.config.credentials.cookie = Some("codex_synth_v1_api_key_a".to_owned());
        assert!(
            other_cookie
                .resolve_metadata_page(
                    Input::FollowingFeed,
                    MetadataPageOptions::default().with_cursor(cursor),
                )
                .await
                .is_err()
        );
        assert_eq!(feed.calls(), before);
        Ok(())
    }

    #[tokio::test]
    async fn request_budget_never_calls_tags_playurl_or_media() -> anyhow::Result<()> {
        let server = MockServer::start();
        let feed = server.mock(|when, then| {
            when.method(GET).path("/x/polymer/web-dynamic/v1/feed/all");
            then.status(200)
                .json_body_obj(&dynamic_page(&[archive(10, 100, 1)], None, false));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "10");
            then.status(200).json_body_obj(&view(10, &[(100, "one")]));
        });
        let tags = server.mock(|when, then| {
            when.method(GET).path("/x/tag/archive/tags");
            then.status(500);
        });
        let playurl = server.mock(|when, then| {
            when.method(GET).path("/x/player/playurl");
            then.status(500);
        });
        let media = server.mock(|when, then| {
            when.method(GET).path("/media");
            then.status(500);
        });
        let result = test_client(&server)
            .resolve_metadata_page(Input::FollowingFeed, MetadataPageOptions::default())
            .await?;
        assert_eq!(result.requests_used, 2);
        assert_eq!(feed.calls(), 1);
        assert_eq!(tags.calls(), 0);
        assert_eq!(playurl.calls(), 0);
        assert_eq!(media.calls(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn invalid_view_pages_consume_source_and_view_budget_without_emitting_items()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let feed = server.mock(|when, then| {
            when.method(GET).path("/x/polymer/web-dynamic/v1/feed/all");
            then.status(200)
                .json_body_obj(&dynamic_page(&[archive(10, 100, 1)], None, false));
        });
        let view_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "10");
            then.status(200)
                .json_body_obj(&view(10, &[(0, "zero cid")]));
        });
        let page = test_client(&server)
            .resolve_metadata_page(Input::FollowingFeed, MetadataPageOptions::default())
            .await?;
        assert_eq!(page.items.len(), 0);
        assert_eq!(page.source_items_examined, 1);
        assert_eq!(page.requests_used, 2);
        assert_eq!(page.stop_reason, MetadataStopReason::End);
        assert_eq!(feed.calls(), 1);
        assert_eq!(view_mock.calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn mismatched_view_aid_and_oversized_responses_fail_without_followup_requests()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/x/polymer/web-dynamic/v1/feed/all");
            then.status(200)
                .json_body_obj(&dynamic_page(&[archive(10, 100, 1)], None, false));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "10");
            then.status(200)
                .json_body_obj(&view(11, &[(100, "wrong aid")]));
        });
        let err = test_client(&server)
            .resolve_metadata_page(Input::FollowingFeed, MetadataPageOptions::default())
            .await
            .err()
            .ok_or_else(|| anyhow::Error::msg("view aid mismatch must be rejected"))?;
        assert!(matches!(err, Error::InvalidInput(message) if message.contains("does not match")));

        let server = MockServer::start();
        let feed = server.mock(|when, then| {
            when.method(GET).path("/x/polymer/web-dynamic/v1/feed/all");
            then.status(200).body(format!(
                "{{\"code\":0,\"data\":{{\"items\":[],\"has_more\":false,\"padding\":\"{}\"}}}}",
                "x".repeat(MAX_RESPONSE_BYTES)
            ));
        });
        let view_mock = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/view");
            then.status(200)
                .json_body_obj(&view(10, &[(100, "unreachable")]));
        });
        let err = test_client(&server)
            .resolve_metadata_page(Input::FollowingFeed, MetadataPageOptions::default())
            .await
            .err()
            .ok_or_else(|| anyhow::Error::msg("oversized response must be rejected"))?;
        assert!(
            matches!(err, Error::InvalidInput(message) if message.contains("1048576-byte limit"))
        );
        assert_eq!(feed.calls(), 1);
        assert_eq!(view_mock.calls(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn option_hard_limits_are_checked_before_network() -> anyhow::Result<()> {
        let server = MockServer::start();
        let feed = server.mock(|when, then| {
            when.method(GET).path("/x/polymer/web-dynamic/v1/feed/all");
            then.status(200)
                .json_body_obj(&dynamic_page(&[], None, false));
        });
        let err = test_client(&server)
            .resolve_metadata_page(
                Input::FollowingFeed,
                MetadataPageOptions::default().with_max_items(nonzero(MAX_ITEMS_LIMIT + 1)),
            )
            .await
            .err()
            .ok_or_else(|| anyhow::Error::msg("overlarge options must be rejected"))?;
        assert!(matches!(err, Error::InvalidInput(message) if message.contains("max_items")));
        assert_eq!(feed.calls(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn bounded_api_attempt_does_not_follow_redirects() -> anyhow::Result<()> {
        let server = MockServer::start();
        let feed = server.mock(|when, then| {
            when.method(GET).path("/x/polymer/web-dynamic/v1/feed/all");
            then.status(302)
                .header("Location", "/x/web-interface/view?aid=10");
        });
        let view_mock = server.mock(|when, then| {
            when.method(GET).path("/x/web-interface/view");
            then.status(200)
                .json_body_obj(&view(10, &[(100, "should not follow")]));
        });
        assert!(
            test_client(&server)
                .resolve_metadata_page(Input::FollowingFeed, MetadataPageOptions::default())
                .await
                .is_err()
        );
        assert_eq!(feed.calls(), 1);
        assert_eq!(view_mock.calls(), 0);
        Ok(())
    }
}

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

mod app_playurl;
mod bv;
mod cancellation;
mod category_catalog;
mod client;
mod credential_preflight;
mod credentials;
mod danmaku;
mod download;
mod error;
mod feed_list;
mod input;
mod login;
mod models;
mod playback;
mod progress;
mod range_transfer;
mod selection;

pub use cancellation::DownloadCancellationToken;
pub use category_catalog::{CategoryCatalog, CategoryCatalogEntry, category_catalog};
pub use client::{
    BiliClient, CategoryVideoBatch, CategoryVideoItem, CategoryVideoQuery, CategoryVideoStats,
    ClientConfig, DEFAULT_CATEGORY_VIDEO_BATCH_SIZE, DEFAULT_CATEGORY_VIDEO_MAX_RESPONSE_BYTES,
    DEFAULT_RANKING_VIDEO_MAX_RESPONSE_BYTES, EndpointConfig, MetadataCollectionInfo,
    MetadataConsistency, MetadataCursor, MetadataPage, MetadataPageOptions, MetadataStopReason,
    PGC_INDEX_MAX_FILTER_GROUPS, PGC_INDEX_MAX_FILTER_VALUES, PGC_INDEX_MAX_ORDERS,
    PGC_INDEX_MAX_PAGE_SIZE, PGC_METADATA_SERVICE_DEFAULT_MAX_RESPONSE_BYTES,
    PGC_TIMETABLE_MAX_DAYS, PGC_TIMETABLE_MAX_ROWS, POPULAR_VIDEO_MAX_PAGE_SIZE,
    PgcIndexConditions, PgcIndexFilter, PgcIndexFilterSelection, PgcIndexFilterValue, PgcIndexItem,
    PgcIndexKind, PgcIndexOrder, PgcIndexOrderOption, PgcIndexPage, PgcIndexQuery, PgcIndexSort,
    PgcMetadataCapability, PgcMetadataRoute, PgcMetadataService, PgcTimetable, PgcTimetableDay,
    PgcTimetableEpisode, PgcTimetableKind, PgcWebPlayurlRoute, PlayurlMode, PopularVideoItem,
    PopularVideoPage, PopularVideoQuery, PopularVideoStats, RankingVideoItem, RankingVideoOptions,
    RankingVideoPage, RankingVideoStats, RestrictedArea, RestrictedAreaConfig, RestrictedAreaProxy,
    RestrictedAreaProxyKind, VIDEO_SEARCH_MAX_ITEMS, VideoSearchItem, VideoSearchOrder,
    VideoSearchPage, VideoSearchQuery,
};
pub use credential_preflight::{
    CredentialPreflightIssue, CredentialPreflightMode, CredentialPreflightReport,
    CredentialPreflightRequestPath, CredentialPreflightRequirement,
    CredentialPreflightRequirementStatus, credential_preflight_requirements,
    credential_preflight_requirements_for_media_paths,
    credential_preflight_requirements_for_media_request,
};
pub use credentials::{
    AccessKeyIdentityKeypair, AccessKeyProvider, AccessKeyProviderSecret, AccessKeyRefreshKeypair,
    AccessKeyRefreshProvider, CredentialAccountIdentity, CredentialHealthProbe,
    CredentialHealthReport, CredentialHealthScope, CredentialHealthStatus, CredentialHealthSummary,
    CredentialHealthSummaryStatus, CredentialKind, CredentialLifecycleCredentialStatus,
    CredentialLifecycleMetadata, CredentialLifecyclePolicy, CredentialLifecycleSource,
    CredentialLifecycleStatus, CredentialProfileLifecycleStatus, CredentialProfileMetadata,
    CredentialProfileSecrets, CredentialProfileSelection, CredentialProfiles,
    CredentialRefreshSecret, CredentialSource, CredentialStore, Credentials,
    DEFAULT_CREDENTIAL_PROFILE,
};
pub use danmaku::{
    DanmakuFormat, DanmakuFormats, DanmakuXmlMerge, merge_xml_append_only, merge_xml_preserving,
};
pub use download::{
    CdnProbeResult, DanmakuAssStatistics, DanmakuUpdateOptions, DanmakuUpdatePolicy,
    DanmakuUpdateReport, DownloadArchive, DownloadArchiveEntryRecord, DownloadArchiveRecord,
    DownloadFileKind, DownloadMode, DownloadOptions, DownloadOutputConflict, DownloadPathTemplates,
    DownloadPreflight, DownloadReport, DownloadReportSummary, DownloadedFile, DuplicateDecision,
    EntryDanmakuUpdateReport, EntryDownloadReport, EntryDownloadSummary, MediaHostOptions,
    MuxOptions, MuxReport, RetryPolicy, SidecarOptions, StagedDanmakuFile, StagedDanmakuUpdate,
    StreamSelection, SubtitleAiPolicy, archive_entry_allows_danmaku_update, probe_media_cdns,
    probe_media_cdns_with_progress,
};
pub use error::{Error, Result};
pub use input::Input;
pub use login::{
    AccessKeyAutomaticRefreshReadiness, AccessKeyLoginConfig, AccessKeyLoginCredentials,
    AccessKeyLoginTicket, AccessKeyLoginTicketOutput, AccessKeyRefreshRequest,
    AccessKeyRenewalAction, AccessKeyRenewalDecision, AccessKeyRenewalReason, QrLoginCredentials,
    QrLoginCredentialsState, QrLoginKind, QrLoginState, QrLoginTicket, QrLoginTicketOutput,
    TvAccessKeyLoginCredentials, TvAccessKeyRefreshRequest, WebCookieRefreshCredentials,
    WebCookieRefreshInfo, WebCookieRefreshRequest,
};
pub use models::{
    ChapterTrack, CodecFamily, DanmakuTrack, DownloadEntry, DownloadPlan, EpisodeMetadata,
    FlvSegment, MediaStream, Owner, PageMetadata, ResolvedContent, SeasonEpisodeGroup,
    SeasonEpisodeGroupKind, SeasonMetadata, SeasonResolution, StreamDiagnostics, StreamQuality,
    StreamResolverAttempt, StreamResolverOutcome, StreamSet, StreamSource, SubtitleFormat,
    SubtitleTrack, Tag, UgcCollectionKind, UgcCollectionReference, VideoCollectionItem,
    VideoCollectionKind, VideoCollectionMetadata, VideoCollectionResolution, VideoMetadata,
};
pub use playback::{
    HttpHeaderSpec, MediaCacheKey, MediaRequestKind, MediaRequestSpec, PlaybackAbrGroup,
    PlaybackAbrGroupKind, PlaybackAbrLevel, PlaybackAbrMetadata, PlaybackCodecFamily,
    PlaybackCodecPreference, PlaybackEntry, PlaybackEntryCacheKey, PlaybackPlan,
    PlaybackSelectionHint, PlaybackSelectionHints, PlaybackSelectionReason, PlaybackVariant,
    PlaybackVariantCacheKey, PlaybackVariantKind,
};
pub use progress::{
    CdnCandidateExclusionReason, DownloadProgressEvent, DownloadProgressSink,
    DownloadTransferDiagnostic, DownloadTransferFailureReason, DownloadTransferFallbackReason,
    DownloadTransferPhase, DownloadTransferRequestOutcome, NoopDownloadProgress,
};
pub use selection::{IndexSelection, IndexSelector, Selection};

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

mod app_playurl;
mod bv;
mod cancellation;
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
mod selection;

pub use cancellation::DownloadCancellationToken;
pub use client::{
    BiliClient, ClientConfig, EndpointConfig, PlayurlMode, RestrictedArea, RestrictedAreaConfig,
    RestrictedAreaProxy, RestrictedAreaProxyKind,
};
pub use credential_preflight::{
    CredentialPreflightIssue, CredentialPreflightMode, CredentialPreflightReport,
    CredentialPreflightRequestPath, CredentialPreflightRequirement,
    CredentialPreflightRequirementStatus, credential_preflight_requirements,
    credential_preflight_requirements_for_media_paths,
    credential_preflight_requirements_for_media_request,
};
pub use credentials::{
    AccessKeyProvider, AccessKeyProviderSecret, AccessKeyRefreshKeypair, AccessKeyRefreshProvider,
    CredentialHealthProbe, CredentialHealthReport, CredentialHealthScope, CredentialHealthStatus,
    CredentialHealthSummary, CredentialHealthSummaryStatus, CredentialKind,
    CredentialLifecycleCredentialStatus, CredentialLifecycleMetadata, CredentialLifecyclePolicy,
    CredentialLifecycleSource, CredentialLifecycleStatus, CredentialProfileLifecycleStatus,
    CredentialProfileMetadata, CredentialProfileSecrets, CredentialProfileSelection,
    CredentialProfiles, CredentialRefreshSecret, CredentialSource, CredentialStore, Credentials,
    DEFAULT_CREDENTIAL_PROFILE,
};
pub use danmaku::{DanmakuFormat, DanmakuFormats, DanmakuXmlMerge, merge_xml_append_only};
pub use download::{
    DanmakuUpdateOptions, DanmakuUpdateReport, DownloadArchive, DownloadArchiveEntryRecord,
    DownloadArchiveRecord, DownloadFileKind, DownloadMode, DownloadOptions, DownloadOutputConflict,
    DownloadPathTemplates, DownloadPreflight, DownloadReport, DownloadReportSummary,
    DownloadedFile, DuplicateDecision, EntryDanmakuUpdateReport, EntryDownloadReport,
    EntryDownloadSummary, MediaHostOptions, MuxOptions, MuxReport, RetryPolicy, SidecarOptions,
    StreamSelection, SubtitleAiPolicy, archive_entry_allows_danmaku_update,
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
    FlvSegment, MediaStream, Owner, PageMetadata, ResolvedContent, SeasonMetadata,
    SeasonResolution, StreamDiagnostics, StreamQuality, StreamResolverAttempt,
    StreamResolverOutcome, StreamSet, StreamSource, SubtitleFormat, SubtitleTrack, Tag,
    VideoCollectionItem, VideoCollectionKind, VideoCollectionMetadata, VideoCollectionResolution,
    VideoMetadata,
};
pub use playback::{
    HttpHeaderSpec, MediaCacheKey, MediaRequestKind, MediaRequestSpec, PlaybackAbrGroup,
    PlaybackAbrGroupKind, PlaybackAbrLevel, PlaybackAbrMetadata, PlaybackCodecFamily,
    PlaybackCodecPreference, PlaybackEntry, PlaybackEntryCacheKey, PlaybackPlan,
    PlaybackSelectionHint, PlaybackSelectionHints, PlaybackSelectionReason, PlaybackVariant,
    PlaybackVariantCacheKey, PlaybackVariantKind,
};
pub use progress::{DownloadProgressEvent, DownloadProgressSink, NoopDownloadProgress};
pub use selection::{IndexSelection, IndexSelector, Selection};

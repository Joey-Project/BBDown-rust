#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

mod app_playurl;
mod bv;
mod client;
mod credentials;
mod danmaku;
mod download;
mod error;
mod feed_list;
mod input;
mod login;
mod models;
mod playback;
mod selection;

pub use client::{
    BiliClient, ClientConfig, EndpointConfig, PlayurlMode, RestrictedArea, RestrictedAreaConfig,
    RestrictedAreaProxy, RestrictedAreaProxyKind,
};
pub use credentials::{
    CredentialHealthProbe, CredentialHealthReport, CredentialHealthStatus, CredentialKind,
    CredentialSource, CredentialStore, Credentials,
};
pub use danmaku::{DanmakuFormat, DanmakuFormats};
pub use download::{
    DownloadArchive, DownloadArchiveEntryRecord, DownloadArchiveRecord, DownloadFileKind,
    DownloadMode, DownloadOptions, DownloadOutputConflict, DownloadPathTemplates,
    DownloadPreflight, DownloadReport, DownloadedFile, DuplicateDecision, EntryDownloadReport,
    MediaHostOptions, MuxOptions, MuxReport, RetryPolicy, SidecarOptions, StreamSelection,
};
pub use error::{Error, Result};
pub use input::Input;
pub use login::{QrLoginKind, QrLoginState, QrLoginTicket, QrLoginTicketOutput};
pub use models::{
    CodecFamily, DanmakuTrack, DownloadEntry, DownloadPlan, EpisodeMetadata, FlvSegment,
    MediaStream, Owner, PageMetadata, ResolvedContent, SeasonMetadata, SeasonResolution,
    StreamDiagnostics, StreamQuality, StreamResolverAttempt, StreamResolverOutcome, StreamSet,
    StreamSource, SubtitleFormat, SubtitleTrack, Tag, VideoCollectionItem, VideoCollectionKind,
    VideoCollectionMetadata, VideoCollectionResolution, VideoMetadata,
};
pub use playback::{
    HttpHeaderSpec, MediaCacheKey, MediaRequestKind, MediaRequestSpec, PlaybackAbrGroup,
    PlaybackAbrGroupKind, PlaybackAbrLevel, PlaybackAbrMetadata, PlaybackCodecFamily,
    PlaybackCodecPreference, PlaybackEntry, PlaybackEntryCacheKey, PlaybackPlan,
    PlaybackSelectionHint, PlaybackSelectionHints, PlaybackSelectionReason, PlaybackVariant,
    PlaybackVariantCacheKey, PlaybackVariantKind,
};
pub use selection::{IndexSelection, IndexSelector, Selection};

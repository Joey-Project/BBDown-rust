#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

mod bv;
mod client;
mod credentials;
mod danmaku;
mod download;
mod error;
mod input;
mod login;
mod models;
mod selection;

pub use client::{
    BiliClient, ClientConfig, EndpointConfig, RestrictedArea, RestrictedAreaConfig,
    RestrictedAreaProxy, RestrictedAreaProxyKind,
};
pub use credentials::{CredentialSource, CredentialStore, Credentials};
pub use download::{
    DanmakuFormat, DownloadArchive, DownloadArchiveEntryRecord, DownloadArchiveRecord,
    DownloadFileKind, DownloadMode, DownloadOptions, DownloadOutputConflict, DownloadPreflight,
    DownloadReport, DownloadedFile, DuplicateDecision, EntryDownloadReport, MuxOptions, MuxReport,
    RetryPolicy, SidecarOptions, StreamSelection,
};
pub use error::{Error, Result};
pub use input::Input;
pub use login::{QrLoginKind, QrLoginState, QrLoginTicket};
pub use models::{
    DanmakuTrack, DownloadEntry, DownloadPlan, EpisodeMetadata, FlvSegment, MediaStream, Owner,
    PageMetadata, ResolvedContent, SeasonMetadata, SeasonResolution, StreamDiagnostics,
    StreamQuality, StreamResolverAttempt, StreamResolverOutcome, StreamSet, StreamSource,
    SubtitleFormat, SubtitleTrack, Tag, VideoCollectionItem, VideoCollectionKind,
    VideoCollectionMetadata, VideoCollectionResolution, VideoMetadata,
};
pub use selection::Selection;

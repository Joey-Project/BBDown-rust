#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

mod app_playurl;
mod bv;
mod cancellation;
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
mod progress;
mod selection;

pub use cancellation::DownloadCancellationToken;
pub use client::{
    BiliClient, ClientConfig, EndpointConfig, PlayurlMode, RestrictedArea, RestrictedAreaConfig,
    RestrictedAreaProxy, RestrictedAreaProxyKind,
};
pub use credentials::{
    CredentialHealthProbe, CredentialHealthReport, CredentialHealthScope, CredentialHealthStatus,
    CredentialKind, CredentialProfileSelection, CredentialProfiles, CredentialSource,
    CredentialStore, Credentials, DEFAULT_CREDENTIAL_PROFILE,
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
    AccessKeyLoginConfig, AccessKeyLoginCredentials, AccessKeyLoginTicket,
    AccessKeyLoginTicketOutput, QrLoginKind, QrLoginState, QrLoginTicket, QrLoginTicketOutput,
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

#[cfg(test)]
mod public_api_tests {
    use super::{
        BiliClient, ClientConfig, DownloadCancellationToken, DownloadOptions,
        DownloadProgressEvent, DownloadProgressSink, DownloadReportSummary, NoopDownloadProgress,
        StreamSelection, SubtitleAiPolicy,
    };
    use std::path::PathBuf;

    #[test]
    fn v0_5_embedding_surface_is_reexported() {
        fn accepts_progress_sink(_sink: &dyn DownloadProgressSink) {}

        let _client = BiliClient::new(ClientConfig::default());
        let _options = DownloadOptions::new("downloads")
            .with_stream_selection(StreamSelection::audio_language("Japanese"))
            .with_subtitles(true)
            .with_subtitle_ai_policy(SubtitleAiPolicy::PreferNonAi);
        let _summary = DownloadReportSummary::default();

        let cancellation = DownloadCancellationToken::new();
        cancellation.cancel_with_reason("stopped by test");
        assert!(cancellation.cancelled_error().is_cancelled());

        let event = DownloadProgressEvent::PlanCancelled {
            title: "example".to_owned(),
            output_dir: PathBuf::from("downloads"),
            completed_entries: 0,
            error: "stopped by test".to_owned(),
        };
        let sink = NoopDownloadProgress;
        sink.on_download_progress(&event);
        accepts_progress_sink(&sink);
    }
}

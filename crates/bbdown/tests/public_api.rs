use bbdown_core::{
    BiliClient, ClientConfig, CredentialHealthSummaryStatus, CredentialKind,
    CredentialLifecycleMetadata, CredentialLifecyclePolicy, CredentialLifecycleSource,
    CredentialLifecycleStatus, CredentialProfileMetadata, CredentialProfiles,
    DownloadCancellationToken, DownloadOptions, DownloadProgressEvent, DownloadProgressSink,
    DownloadReportSummary, NoopDownloadProgress, StreamSelection, SubtitleAiPolicy,
};
use std::path::PathBuf;

#[test]
fn embedding_surface_is_reexported() -> anyhow::Result<()> {
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

    let mut credential_metadata = CredentialProfileMetadata::default();
    credential_metadata.set_credential(
        CredentialKind::AccessKey,
        CredentialLifecycleMetadata::default()
            .with_source(CredentialLifecycleSource::AccessKeyLogin)
            .with_refresh_token_present(true),
    );
    assert!(
        credential_metadata
            .credential(CredentialKind::AccessKey)
            .is_some()
    );

    let lifecycle_policy = CredentialLifecyclePolicy::at_unix_millis(1_700_000_000_000);
    let profiles = CredentialProfiles::default();
    let lifecycle_status = profiles.profile_lifecycle_status("default", &lifecycle_policy)?;
    assert_eq!(lifecycle_status.status, CredentialLifecycleStatus::Missing);

    let health_status = CredentialHealthSummaryStatus::Unknown;
    assert_eq!(format!("{health_status:?}"), "Unknown");
    Ok(())
}

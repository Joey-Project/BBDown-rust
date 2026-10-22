use bbdown_core::{
    BiliClient, CategoryVideoQuery, CategoryVideoStats, ClientConfig,
    CredentialHealthSummaryStatus, CredentialKind, CredentialLifecycleMetadata,
    CredentialLifecyclePolicy, CredentialLifecycleSource, CredentialLifecycleStatus,
    CredentialPreflightMode, CredentialPreflightReport, CredentialProfileMetadata,
    CredentialProfiles, DEFAULT_CATEGORY_VIDEO_BATCH_SIZE,
    DEFAULT_CATEGORY_VIDEO_MAX_RESPONSE_BYTES, DEFAULT_RANKING_VIDEO_MAX_RESPONSE_BYTES,
    DanmakuUpdateOptions, DanmakuUpdatePolicy, DownloadCancellationToken, DownloadOptions,
    DownloadProgressEvent, DownloadProgressSink, DownloadReportSummary, Input, MetadataPageOptions,
    NoopDownloadProgress, POPULAR_VIDEO_MAX_PAGE_SIZE, PgcIndexFilterSelection, PgcIndexKind,
    PgcIndexOrder, PgcIndexQuery, PgcIndexSort, PgcTimetable, PgcTimetableDay, PgcTimetableEpisode,
    PgcTimetableKind, PlayurlMode, PopularVideoQuery, PopularVideoStats, RankingVideoOptions,
    RestrictedAreaConfig, SeasonEpisodeGroup, SeasonEpisodeGroupKind, StreamSelection,
    SubtitleAiPolicy, UgcCollectionKind, UgcCollectionReference, VideoCollectionKind,
};
use std::num::NonZeroUsize;
use std::path::PathBuf;

#[test]
fn embedding_surface_is_reexported() -> anyhow::Result<()> {
    fn accepts_progress_sink(_sink: &dyn DownloadProgressSink) {}

    let _client = BiliClient::new(ClientConfig::default());
    let collection_reference = UgcCollectionReference {
        id: 167_822,
        kind: UgcCollectionKind::Collection,
        owner_mid: 210_798,
        title: "Example collection".to_owned(),
        description: "Example description".to_owned(),
        cover_url: None,
        item_count: 1,
    };
    assert_eq!(
        collection_reference.as_input(),
        Input::SpaceCollectionList {
            list_id: 167_822,
            owner_mid: 210_798,
        }
    );
    let related_input = Input::parse("related:av170001")?;
    assert_eq!(related_input, Input::RelatedRecommendations(170_001));
    assert_eq!(related_input.aid_hint()?, None);
    let metadata_options = MetadataPageOptions::default()
        .with_max_items(NonZeroUsize::MIN)
        .with_max_source_items(NonZeroUsize::MIN)
        .with_max_requests(NonZeroUsize::MIN);
    assert_eq!(metadata_options.max_items, NonZeroUsize::MIN);
    assert_eq!(metadata_options.max_source_items, NonZeroUsize::MIN);
    assert_eq!(metadata_options.max_requests, NonZeroUsize::MIN);
    assert!(metadata_options.cursor.is_none());
    assert_eq!(
        serde_json::to_value(VideoCollectionKind::RelatedRecommendation)?,
        serde_json::json!("related_recommendation")
    );
    let _options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::audio_language("Japanese"))
        .with_subtitles(true)
        .with_subtitle_ai_policy(SubtitleAiPolicy::PreferNonAi);
    let update_options =
        DanmakuUpdateOptions::default().with_update_policy(DanmakuUpdatePolicy::Preserve);
    assert_eq!(update_options.update_policy, DanmakuUpdatePolicy::Preserve);
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
    let preflight = CredentialPreflightReport::from_client_context(
        CredentialPreflightMode::Warn,
        &lifecycle_status,
        PlayurlMode::Web,
        &RestrictedAreaConfig::default(),
    );
    assert!(!preflight.has_blocking_issues());

    let health_status = CredentialHealthSummaryStatus::Unknown;
    assert_eq!(format!("{health_status:?}"), "Unknown");
    Ok(())
}

#[test]
fn season_episode_group_types_are_reexported() {
    let group = SeasonEpisodeGroup {
        kind: SeasonEpisodeGroupKind::Main,
        source_id: None,
        title: None,
        episode_positions: vec![0, 1],
    };
    assert_eq!(group.episode_positions, vec![0, 1]);
}

#[test]
fn pgc_timetable_types_are_reexported() -> anyhow::Result<()> {
    let episode = PgcTimetableEpisode {
        episode_id: None,
        season_id: 456,
        title: "Example episode".to_owned(),
        pub_index: "第1话".to_owned(),
        pub_time: "20:00".to_owned(),
        pub_ts: 1_792_430_400,
        published: false,
        delayed: true,
        delay_index: Some("延期".to_owned()),
        delay_reason: Some("schedule change".to_owned()),
        cover_url: Some("https://example.test/episode.jpg".to_owned()),
    };
    let timetable = PgcTimetable {
        kind: PgcTimetableKind::Anime,
        latest: vec![episode.clone()],
        days: vec![PgcTimetableDay {
            date: "10-20".to_owned(),
            date_ts: 1_792_454_400,
            day_of_week: 2,
            is_today: true,
            episodes: vec![episode],
        }],
    };
    let serialized = serde_json::to_value(timetable)?;
    assert_eq!(serialized["kind"], "anime");
    assert!(serialized["latest"][0]["episode_id"].is_null());
    assert_eq!(serialized["days"][0]["episodes"][0]["season_id"], 456);
    assert_eq!(serialized["days"][0]["episodes"][0]["published"], false);
    Ok(())
}

#[test]
fn pgc_index_query_types_are_reexported() {
    let query = PgcIndexQuery::new(PgcIndexKind::Movie)
        .with_order(PgcIndexOrder::ReleaseDate)
        .with_sort(PgcIndexSort::Ascending)
        .with_page(2)
        .with_page_size(5)
        .with_filters(PgcIndexFilterSelection {
            area: Some("1".to_owned()),
            ..PgcIndexFilterSelection::default()
        });
    assert_eq!(query.page, 2);
    assert_eq!(query.page_size, 5);
    assert_eq!(query.filters.area.as_deref(), Some("1"));
}

#[test]
fn popular_video_types_are_reexported() {
    let query = PopularVideoQuery::default().with_page(2).with_page_size(5);
    assert_eq!(query.page, 2);
    assert_eq!(query.page_size, 5);
    assert_eq!(POPULAR_VIDEO_MAX_PAGE_SIZE, 20);

    let stats = PopularVideoStats {
        view: Some(10),
        ..PopularVideoStats::default()
    };
    assert_eq!(stats.view, Some(10));
    assert_eq!(stats.like, None);
}

#[test]
fn ranking_video_types_and_options_are_reexported() {
    let client = BiliClient::new(ClientConfig::default());
    assert_eq!(DEFAULT_RANKING_VIDEO_MAX_RESPONSE_BYTES, 1024 * 1024);
    assert_eq!(
        RankingVideoOptions::default().max_response_bytes,
        DEFAULT_RANKING_VIDEO_MAX_RESPONSE_BYTES
    );
    let _configured_call = client.ranking_videos_with_options(RankingVideoOptions {
        max_response_bytes: DEFAULT_RANKING_VIDEO_MAX_RESPONSE_BYTES * 2,
    });
    let page = bbdown_core::RankingVideoPage {
        note: None,
        items: vec![bbdown_core::RankingVideoItem {
            rank: 1,
            aid: 42,
            bvid: None,
            title: "Example".to_owned(),
            author: None,
            author_mid: None,
            duration_seconds: None,
            pub_time: None,
            cover_url: None,
            description: None,
            category_name: None,
            stats: Some(bbdown_core::RankingVideoStats {
                view: Some(7),
                ..bbdown_core::RankingVideoStats::default()
            }),
        }],
    };
    assert_eq!(page.items[0].rank, 1);
    assert_eq!(
        page.items[0].stats.as_ref().and_then(|stats| stats.view),
        Some(7)
    );
}

#[test]
fn category_video_types_are_reexported() {
    let client = BiliClient::new(ClientConfig::default());
    let query = CategoryVideoQuery::new(17)
        .with_batch_size(8)
        .with_max_response_bytes(4096);
    assert_eq!(query.rid, 17);
    assert_eq!(query.batch_size, 8);
    assert_eq!(query.max_response_bytes, 4096);
    assert_eq!(DEFAULT_CATEGORY_VIDEO_BATCH_SIZE, 20);
    assert_eq!(DEFAULT_CATEGORY_VIDEO_MAX_RESPONSE_BYTES, 1024 * 1024);
    let _configured_call = client.category_videos(query);

    let stats = CategoryVideoStats {
        view: Some(12),
        ..CategoryVideoStats::default()
    };
    assert_eq!(stats.view, Some(12));
    assert_eq!(stats.like, None);
    let batch = bbdown_core::CategoryVideoBatch {
        requested_rid: 17,
        requested_batch_size: 8,
        items: vec![],
    };
    assert_eq!(batch.requested_rid, 17);
    assert_eq!(batch.items, Vec::new());
}

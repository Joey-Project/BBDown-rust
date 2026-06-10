use crate::{
    BiliClient, DownloadEntry, DownloadPlan, Error, FlvSegment, Input, MediaStream, Result,
    Selection, SubtitleFormat, SubtitleTrack,
};
use futures_util::StreamExt;
use md5::{Digest, Md5};
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const MAX_FILE_NAME_BYTES: usize = 80;
const MAX_FILE_COMPONENT_BYTES: usize = 240;
const MAX_SUBTITLE_EXTENSION_BYTES: usize = 16;
const MAX_COVER_EXTENSION_BYTES: usize = 16;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DownloadMode {
    #[default]
    All,
    VideoOnly,
    AudioOnly,
    SubtitleOnly,
    DanmakuOnly,
    CoverOnly,
}

impl DownloadMode {
    const fn allows_mux(self) -> bool {
        matches!(self, Self::All)
    }

    const fn archive_key_token(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::VideoOnly => "video",
            Self::AudioOnly => "audio",
            Self::SubtitleOnly => "subtitle",
            Self::DanmakuOnly => "danmaku",
            Self::CoverOnly => "cover",
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct DownloadOptions {
    pub output_dir: PathBuf,
    pub retry: RetryPolicy,
    pub stream_selection: StreamSelection,
    pub resume: bool,
    pub mode: DownloadMode,
    pub include_subtitles: bool,
    pub include_danmaku: bool,
    pub sidecars: SidecarOptions,
    pub mux: MuxOptions,
    pub download_idle_timeout: Option<Duration>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("."),
            retry: RetryPolicy::default(),
            stream_selection: StreamSelection::default(),
            resume: true,
            mode: DownloadMode::All,
            include_subtitles: true,
            include_danmaku: true,
            sidecars: SidecarOptions::default(),
            mux: MuxOptions::Disabled,
            download_idle_timeout: Some(Duration::from_secs(30)),
        }
    }
}

impl DownloadOptions {
    #[must_use]
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    #[must_use]
    pub fn with_stream_selection(mut self, stream_selection: StreamSelection) -> Self {
        self.stream_selection = stream_selection;
        self
    }

    #[must_use]
    pub fn with_resume(mut self, resume: bool) -> Self {
        self.resume = resume;
        self
    }

    #[must_use]
    pub fn with_download_mode(mut self, mode: DownloadMode) -> Self {
        self.mode = mode;
        self
    }

    #[must_use]
    pub fn with_cover(mut self, include_cover: bool) -> Self {
        self.sidecars.cover = include_cover;
        self
    }

    #[must_use]
    pub fn with_subtitles(mut self, include_subtitles: bool) -> Self {
        self.include_subtitles = include_subtitles;
        self.sidecars.subtitles = include_subtitles;
        self
    }

    #[must_use]
    pub fn with_danmaku(mut self, include_danmaku: bool) -> Self {
        self.include_danmaku = include_danmaku;
        self.sidecars.danmaku = include_danmaku;
        self
    }

    #[must_use]
    pub fn with_mux(mut self, mux: MuxOptions) -> Self {
        self.mux = mux;
        self
    }

    #[must_use]
    pub fn with_download_idle_timeout(mut self, download_idle_timeout: Option<Duration>) -> Self {
        self.download_idle_timeout = download_idle_timeout;
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SidecarOptions {
    pub cover: bool,
    pub subtitles: bool,
    pub danmaku: bool,
}

impl Default for SidecarOptions {
    fn default() -> Self {
        Self {
            cover: false,
            subtitles: true,
            danmaku: true,
        }
    }
}

impl SidecarOptions {
    #[must_use]
    pub const fn new(cover: bool, subtitles: bool, danmaku: bool) -> Self {
        Self {
            cover,
            subtitles,
            danmaku,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamSelection {
    pub video_quality: Option<u32>,
    pub audio_quality: Option<u32>,
}

impl StreamSelection {
    #[must_use]
    pub const fn new(video_quality: Option<u32>, audio_quality: Option<u32>) -> Self {
        Self {
            video_quality,
            audio_quality,
        }
    }

    #[must_use]
    pub const fn video(video_quality: u32) -> Self {
        Self {
            video_quality: Some(video_quality),
            audio_quality: None,
        }
    }

    #[must_use]
    pub const fn audio(audio_quality: u32) -> Self {
        Self {
            video_quality: None,
            audio_quality: Some(audio_quality),
        }
    }

    #[must_use]
    pub const fn has_selection(self) -> bool {
        self.video_quality.is_some() || self.audio_quality.is_some()
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff: Duration,
}

impl RetryPolicy {
    #[must_use]
    pub const fn new(max_attempts: u32, backoff: Duration) -> Self {
        Self {
            max_attempts,
            backoff,
        }
    }

    #[must_use]
    pub const fn single_attempt() -> Self {
        Self {
            max_attempts: 1,
            backoff: Duration::ZERO,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff: Duration::from_millis(250),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MuxOptions {
    Disabled,
    Ffmpeg { binary: PathBuf },
}

impl MuxOptions {
    #[must_use]
    pub fn ffmpeg(binary: impl Into<PathBuf>) -> Self {
        Self::Ffmpeg {
            binary: binary.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateDecision {
    Replace,
    KeepBoth,
    Cancel,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadArchive {
    pub records: Vec<DownloadArchiveRecord>,
}

impl DownloadArchive {
    #[must_use]
    pub fn new(records: Vec<DownloadArchiveRecord>) -> Self {
        Self { records }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(Error::Io(error)),
        };
        serde_json::from_str(&raw).map_err(Error::from)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let save_path = archive_storage_path(path)?;
        if save_path.is_dir() {
            return Err(Error::InvalidInput(
                "download archive path is a directory".to_owned(),
            ));
        }
        if let Some(parent) = save_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let temporary_path = archive_sidecar_path(&save_path, ".bbdown-archive-tmp");
        std::fs::write(&temporary_path, serde_json::to_vec_pretty(self)?)?;
        replace_archive_file(&temporary_path, &save_path)?;
        Ok(())
    }

    pub fn record_download(
        &mut self,
        plan: &DownloadPlan,
        options: &DownloadOptions,
        report: &DownloadReport,
    ) {
        let record =
            DownloadArchiveRecord::from_report(plan, options, report, current_unix_seconds());
        let record_output_key = comparable_output_path_key(&record.output_dir);
        self.records.retain(|existing| {
            comparable_output_path_key(&existing.output_dir) != record_output_key
        });
        self.records.push(record);
    }

    #[must_use]
    pub fn records_for_plan(&self, plan: &DownloadPlan) -> Vec<DownloadArchiveRecord> {
        let plan_matches = archive_plan_matches_for_all_modes(plan);
        self.records
            .iter()
            .filter(|record| {
                plan_matches
                    .iter()
                    .any(|plan_match| plan_match.matches_record(record))
            })
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn records_for_plan_with_mode(
        &self,
        plan: &DownloadPlan,
        mode: DownloadMode,
    ) -> Vec<DownloadArchiveRecord> {
        let plan_match = ArchivePlanMatch::new(plan, mode);
        self.records
            .iter()
            .filter(|record| plan_match.matches_record(record))
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadArchiveRecord {
    pub content_key: String,
    pub title: String,
    pub output_dir: PathBuf,
    pub completed_at_unix: u64,
    pub entries: Vec<DownloadArchiveEntryRecord>,
}

impl DownloadArchiveRecord {
    fn from_report(
        plan: &DownloadPlan,
        options: &DownloadOptions,
        report: &DownloadReport,
        completed_at_unix: u64,
    ) -> Self {
        Self {
            content_key: download_plan_content_key_for_mode(plan, options.mode),
            title: report.title.clone(),
            output_dir: archive_record_path(&report.output_dir),
            completed_at_unix,
            entries: plan
                .entries
                .iter()
                .zip(&report.entries)
                .map(|(plan_entry, report_entry)| DownloadArchiveEntryRecord {
                    content_key: download_entry_content_key_for_mode(plan_entry, options.mode),
                    index: report_entry.index,
                    aid: plan_entry.aid,
                    bvid: plan_entry.bvid.clone(),
                    cid: plan_entry.cid,
                    epid: plan_entry.epid,
                    title: report_entry.title.clone(),
                    directory: archive_record_path(&report_entry.directory),
                    files: report_entry
                        .files
                        .iter()
                        .map(|file| archive_record_path(&file.path))
                        .collect(),
                    mux_output: report_entry
                        .mux
                        .as_ref()
                        .map(|mux| archive_record_path(&mux.output_path)),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadArchiveEntryRecord {
    pub content_key: String,
    pub index: u32,
    pub aid: u64,
    pub bvid: Option<String>,
    pub cid: u64,
    pub epid: Option<u64>,
    pub title: String,
    pub directory: PathBuf,
    pub files: Vec<PathBuf>,
    pub mux_output: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadPreflight {
    pub content_key: String,
    pub title: String,
    pub planned_output_dir: PathBuf,
    pub archived_records: Vec<DownloadArchiveRecord>,
    pub output_conflict: Option<DownloadOutputConflict>,
    #[doc(hidden)]
    #[serde(default)]
    pub reserved_output_dirs: Vec<PathBuf>,
}

impl DownloadPreflight {
    pub fn inspect(
        plan: &DownloadPlan,
        options: &DownloadOptions,
        archive: Option<&DownloadArchive>,
    ) -> Result<Self> {
        let planned_output_dir = default_plan_output_dir(plan, options);
        let output_conflict =
            path_is_occupied(&planned_output_dir)?.then_some(DownloadOutputConflict {
                path: planned_output_dir.clone(),
            });
        let archived_records = archive.map_or_else(Vec::new, |archive| {
            archive_records_for_preflight(archive, plan, options.mode, &planned_output_dir)
        });
        let reserved_output_dirs = archive.map_or_else(Vec::new, |archive| {
            archive
                .records
                .iter()
                .map(|record| record.output_dir.clone())
                .collect()
        });
        Ok(Self {
            content_key: download_plan_content_key_for_mode(plan, options.mode),
            title: plan.title.clone(),
            planned_output_dir: planned_output_dir.clone(),
            archived_records,
            output_conflict,
            reserved_output_dirs,
        })
    }

    #[must_use]
    pub fn requires_decision(&self) -> bool {
        !self.archived_records.is_empty() || self.output_conflict.is_some()
    }

    #[must_use]
    pub const fn suggested_decision(&self) -> DuplicateDecision {
        DuplicateDecision::Cancel
    }

    pub fn output_dir_for_decision(&self, decision: DuplicateDecision) -> Result<PathBuf> {
        match decision {
            DuplicateDecision::KeepBoth => next_available_output_dir_avoiding(
                &self.planned_output_dir,
                &self.reserved_output_dirs_for_decision(),
            ),
            DuplicateDecision::Replace | DuplicateDecision::Cancel => {
                Ok(self.planned_output_dir.clone())
            }
        }
    }

    fn reserved_output_dirs_for_decision(&self) -> Vec<PathBuf> {
        let mut reserved = self.reserved_output_dirs.clone();
        for record in &self.archived_records {
            if !path_is_reserved(&record.output_dir, &reserved) {
                reserved.push(record.output_dir.clone());
            }
        }
        reserved
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadOutputConflict {
    pub path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadReport {
    pub title: String,
    pub output_dir: PathBuf,
    pub entries: Vec<EntryDownloadReport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EntryDownloadReport {
    pub index: u32,
    pub title: String,
    pub directory: PathBuf,
    pub files: Vec<DownloadedFile>,
    pub mux: Option<MuxReport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadedFile {
    pub kind: DownloadFileKind,
    pub path: PathBuf,
    pub bytes_written: u64,
    pub resumed_from: u64,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadFileKind {
    Video,
    Audio,
    FlvSegment,
    Cover,
    Subtitle,
    Danmaku,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MuxReport {
    pub output_path: PathBuf,
    pub command: Vec<String>,
}

impl BiliClient {
    pub async fn download_input(
        &self,
        raw: &str,
        selection: Option<Selection>,
        options: DownloadOptions,
    ) -> Result<DownloadReport> {
        let input = Input::parse(raw)?;
        self.download(input, selection, options).await
    }

    pub async fn download(
        &self,
        input: Input,
        selection: Option<Selection>,
        options: DownloadOptions,
    ) -> Result<DownloadReport> {
        let plan = self
            .plan_for_download(input, selection, options.mode)
            .await?;
        self.download_plan(&plan, options).await
    }

    pub async fn download_plan(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
    ) -> Result<DownloadReport> {
        validate_plan_stream_selection(plan, &options)?;
        let output_dir = default_plan_output_dir(plan, &options);
        self.download_plan_to_output_dir(plan, options, output_dir)
            .await
    }

    pub async fn download_plan_with_archive_decision(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        archive: &mut DownloadArchive,
        decision: DuplicateDecision,
    ) -> Result<DownloadReport> {
        let preflight = DownloadPreflight::inspect(plan, &options, Some(archive))?;
        self.download_plan_with_archive_preflight_decision(
            plan, options, archive, &preflight, decision,
        )
        .await
    }

    pub async fn download_plan_with_archive_preflight_decision(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        archive: &mut DownloadArchive,
        preflight: &DownloadPreflight,
        decision: DuplicateDecision,
    ) -> Result<DownloadReport> {
        validate_archive_preflight(plan, &options, archive, preflight)?;
        let mut effective_options = options;
        let output_dir = match decision {
            DuplicateDecision::Cancel if preflight.requires_decision() => {
                return Err(Error::InvalidInput(
                    "download canceled because archive or output conflict requires a decision"
                        .to_owned(),
                ));
            }
            DuplicateDecision::Cancel => default_plan_output_dir(plan, &effective_options),
            DuplicateDecision::Replace => {
                if path_is_occupied(&preflight.planned_output_dir)? {
                    validate_plan_stream_selection(plan, &effective_options)?;
                    remove_output_root_if_exists(&preflight.planned_output_dir).await?;
                    effective_options.resume = false;
                }
                preflight.planned_output_dir.clone()
            }
            DuplicateDecision::KeepBoth => preflight.output_dir_for_decision(decision)?,
        };
        if decision != DuplicateDecision::Replace && path_is_occupied(&output_dir)? {
            return Err(Error::InvalidInput(format!(
                "output directory appeared after duplicate preflight: {}",
                output_dir.display()
            )));
        }
        let report = self
            .download_plan_to_output_dir(plan, effective_options.clone(), output_dir)
            .await?;
        archive.record_download(plan, &effective_options, &report);
        Ok(report)
    }

    async fn download_plan_to_output_dir(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        output_dir: PathBuf,
    ) -> Result<DownloadReport> {
        validate_plan_stream_selection(plan, &options)?;
        fs::create_dir_all(&output_dir).await?;
        let mut entries = Vec::new();
        for entry in &plan.entries {
            entries.push(self.download_entry(entry, &output_dir, &options).await?);
        }
        Ok(DownloadReport {
            title: plan.title.clone(),
            output_dir,
            entries,
        })
    }

    async fn download_entry(
        &self,
        entry: &DownloadEntry,
        output_dir: &Path,
        options: &DownloadOptions,
    ) -> Result<EntryDownloadReport> {
        let entry_dir = output_dir.join(entry_dir_name(entry));
        fs::create_dir_all(&entry_dir).await?;
        let mut files = Vec::new();
        self.download_entry_media(entry, &entry_dir, options, &mut files)
            .await?;
        self.download_entry_sidecars(entry, &entry_dir, options, &mut files)
            .await?;
        let mux = self.mux_entry(entry, &entry_dir, &files, options).await?;
        Ok(EntryDownloadReport {
            index: entry.index,
            title: entry.title.clone(),
            directory: entry_dir,
            files,
            mux,
        })
    }

    async fn download_entry_media(
        &self,
        entry: &DownloadEntry,
        entry_dir: &Path,
        options: &DownloadOptions,
        files: &mut Vec<DownloadedFile>,
    ) -> Result<()> {
        match options.mode {
            DownloadMode::All => {
                let has_dash_pair =
                    !entry.streams.videos.is_empty() && !entry.streams.audios.is_empty();
                let use_flv_fallback = !has_dash_pair && !entry.streams.flv_segments.is_empty();
                if has_dash_pair {
                    let video = select_media_stream(
                        &entry.streams.videos,
                        options.stream_selection.video_quality,
                        "video",
                    )?;
                    let audio = select_media_stream(
                        &entry.streams.audios,
                        options.stream_selection.audio_quality,
                        "audio",
                    )?;
                    files.push(
                        self.download_media_stream(
                            video,
                            DownloadFileKind::Video,
                            entry_dir,
                            options,
                        )
                        .await?,
                    );
                    files.push(
                        self.download_media_stream(
                            audio,
                            DownloadFileKind::Audio,
                            entry_dir,
                            options,
                        )
                        .await?,
                    );
                } else if use_flv_fallback {
                    if options.stream_selection.has_selection() {
                        return Err(Error::InvalidInput(
                            "stream quality selection requires DASH media; selected entry only has FLV segments"
                                .to_owned(),
                        ));
                    }
                    for segment in &entry.streams.flv_segments {
                        files.push(
                            self.download_flv_segment(segment, entry_dir, options)
                                .await?,
                        );
                    }
                } else {
                    return Err(Error::MissingField("complete DASH media or FLV segments"));
                }
            }
            DownloadMode::VideoOnly => {
                let video = select_media_stream(
                    &entry.streams.videos,
                    options.stream_selection.video_quality,
                    "video",
                )?;
                files.push(
                    self.download_media_stream(video, DownloadFileKind::Video, entry_dir, options)
                        .await?,
                );
            }
            DownloadMode::AudioOnly => {
                let audio = select_media_stream(
                    &entry.streams.audios,
                    options.stream_selection.audio_quality,
                    "audio",
                )?;
                files.push(
                    self.download_media_stream(audio, DownloadFileKind::Audio, entry_dir, options)
                        .await?,
                );
            }
            DownloadMode::SubtitleOnly | DownloadMode::DanmakuOnly | DownloadMode::CoverOnly => {}
        }
        Ok(())
    }

    async fn download_entry_sidecars(
        &self,
        entry: &DownloadEntry,
        entry_dir: &Path,
        options: &DownloadOptions,
        files: &mut Vec<DownloadedFile>,
    ) -> Result<()> {
        if should_download_cover(options) {
            if let Some(cover_url) = entry.cover_url.as_deref().filter(|url| !url.is_empty()) {
                files.push(
                    self.download_url_to_file(
                        cover_url,
                        &entry_dir.join(cover_file_name(cover_url)),
                        DownloadFileKind::Cover,
                        None,
                        options,
                    )
                    .await?,
                );
            } else if matches!(options.mode, DownloadMode::CoverOnly) {
                return Err(Error::MissingField("cover URL"));
            }
        }
        if should_download_subtitles(options) {
            let mut seen_subtitles = HashSet::new();
            for (index, subtitle) in entry.subtitles.iter().enumerate() {
                if !seen_subtitles.insert(subtitle_dedup_key(&subtitle.url)) {
                    continue;
                }
                files.push(
                    self.download_subtitle(index, subtitle, entry_dir, options)
                        .await?,
                );
            }
            if matches!(options.mode, DownloadMode::SubtitleOnly) && seen_subtitles.is_empty() {
                return Err(Error::MissingField("subtitle tracks"));
            }
        }
        if should_download_danmaku(options) {
            files.push(
                self.download_url_to_file(
                    &entry.danmaku.xml_url,
                    &entry_dir.join("danmaku.xml"),
                    DownloadFileKind::Danmaku,
                    None,
                    options,
                )
                .await?,
            );
        }
        Ok(())
    }

    async fn download_media_stream(
        &self,
        stream: &MediaStream,
        kind: DownloadFileKind,
        entry_dir: &Path,
        options: &DownloadOptions,
    ) -> Result<DownloadedFile> {
        let label = match kind {
            DownloadFileKind::Video => "video",
            DownloadFileKind::Audio => "audio",
            DownloadFileKind::FlvSegment
            | DownloadFileKind::Cover
            | DownloadFileKind::Subtitle
            | DownloadFileKind::Danmaku => "media",
        };
        let path = entry_dir.join(media_file_name(label, stream));
        self.download_candidate_urls_to_file(
            &candidate_urls(&stream.base_url, &stream.backup_urls),
            &path,
            kind,
            stream.size,
            options,
        )
        .await
    }

    async fn download_flv_segment(
        &self,
        segment: &FlvSegment,
        entry_dir: &Path,
        options: &DownloadOptions,
    ) -> Result<DownloadedFile> {
        let path = entry_dir.join(format!("segment-{:03}.flv", segment.order));
        self.download_candidate_urls_to_file(
            &candidate_urls(&segment.url, &segment.backup_urls),
            &path,
            DownloadFileKind::FlvSegment,
            segment.size,
            options,
        )
        .await
    }

    async fn download_subtitle(
        &self,
        index: usize,
        subtitle: &SubtitleTrack,
        entry_dir: &Path,
        options: &DownloadOptions,
    ) -> Result<DownloadedFile> {
        let path = entry_dir.join(subtitle_file_name(index, subtitle));
        self.download_url_to_file(
            &subtitle.url,
            &path,
            DownloadFileKind::Subtitle,
            None,
            options,
        )
        .await
    }

    async fn download_url_to_file(
        &self,
        url: &str,
        path: &Path,
        kind: DownloadFileKind,
        expected_size: Option<u64>,
        options: &DownloadOptions,
    ) -> Result<DownloadedFile> {
        let attempts = options.retry.max_attempts.max(1);
        let mut last_error = None;
        for attempt in 1..=attempts {
            match self
                .try_download_url_to_file(url, path, kind.clone(), expected_size, options)
                .await
            {
                Ok(file) => return Ok(file),
                Err(error) if attempt < attempts => {
                    last_error = Some(error);
                    if !options.retry.backoff.is_zero() {
                        tokio::time::sleep(options.retry.backoff).await;
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| Error::InvalidInput("download retry failed".to_owned())))
    }

    async fn download_candidate_urls_to_file(
        &self,
        urls: &[String],
        path: &Path,
        kind: DownloadFileKind,
        expected_size: Option<u64>,
        options: &DownloadOptions,
    ) -> Result<DownloadedFile> {
        let mut last_error = None;
        for url in urls {
            match self
                .download_url_to_file(url, path, kind.clone(), expected_size, options)
                .await
            {
                Ok(file) => return Ok(file),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| Error::InvalidInput("empty download URL list".to_owned())))
    }

    async fn try_download_url_to_file(
        &self,
        url: &str,
        path: &Path,
        kind: DownloadFileKind,
        expected_size: Option<u64>,
        options: &DownloadOptions,
    ) -> Result<DownloadedFile> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let existing_len = existing_file_len(path).await?;
        let resume_from = if options.resume { existing_len } else { 0 };
        let response = self.send_download_request(url, resume_from).await?;
        let status = response.status();
        if resume_from > 0 && status == StatusCode::RANGE_NOT_SATISFIABLE {
            if content_range_complete_len(response.headers()) == Some(resume_from)
                && expected_size.is_none_or(|size| size == resume_from)
            {
                return Ok(DownloadedFile {
                    kind,
                    path: path.to_path_buf(),
                    bytes_written: 0,
                    resumed_from: resume_from,
                });
            }
            return Err(Error::InvalidInput(
                "server rejected resume range for a different file length".to_owned(),
            ));
        }
        let response = response
            .error_for_status()
            .map_err(BiliClient::http_error_without_url)?;
        let has_content_range = response.headers().contains_key(CONTENT_RANGE);
        let content_range = content_range(response.headers())?;
        let response_content_len = response.content_length();
        let append =
            validate_resume_response(status, resume_from, has_content_range, content_range)?;
        let start_offset = if append { resume_from } else { 0 };
        let full_retry_after_ignored_range = resume_from > 0 && !append;
        let validation_expected_size = validation_size_for_full_retry(
            expected_size,
            content_range,
            response_content_len,
            full_retry_after_ignored_range,
        );
        if full_retry_after_ignored_range && validation_expected_size.is_none() {
            return Err(Error::InvalidInput(
                "server ignored resume range without a verifiable full response length".to_owned(),
            ));
        }
        let replace_existing = existing_len > 0 && !append;
        let write_path = if replace_existing {
            temporary_download_path(path)
        } else {
            path.to_path_buf()
        };
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&write_path)
            .await?;
        let write_result = write_response_body_to_file(
            &mut file,
            response,
            content_range,
            start_offset,
            validation_expected_size,
            options.download_idle_timeout,
        )
        .await;
        drop(file);
        let bytes_written = match write_result {
            Ok(bytes_written) => bytes_written,
            Err(error) => {
                if replace_existing {
                    let _ = fs::remove_file(&write_path).await;
                }
                return Err(error);
            }
        };
        if is_unexpected_empty_response(&kind, bytes_written) {
            if !append {
                let _ = fs::remove_file(&write_path).await;
            }
            return Err(Error::InvalidInput("empty media response".to_owned()));
        }
        if replace_existing {
            replace_file(&write_path, path).await?;
        }
        Ok(DownloadedFile {
            kind,
            path: path.to_path_buf(),
            bytes_written,
            resumed_from: start_offset,
        })
    }

    async fn send_download_request(
        &self,
        url: &str,
        resume_from: u64,
    ) -> Result<reqwest::Response> {
        let mut request = self.http.get(url).headers(self.media_headers()?);
        if resume_from > 0 {
            request = request.header(RANGE, format!("bytes={resume_from}-"));
        }
        tokio::time::timeout(self.config.request_timeout, request.send())
            .await
            .map_err(|_| Error::InvalidInput("download request timeout elapsed".to_owned()))?
            .map_err(BiliClient::http_error_without_url)
    }

    async fn mux_entry(
        &self,
        entry: &DownloadEntry,
        entry_dir: &Path,
        files: &[DownloadedFile],
        options: &DownloadOptions,
    ) -> Result<Option<MuxReport>> {
        if !options.mode.allows_mux() {
            return Ok(None);
        }
        let MuxOptions::Ffmpeg { binary } = &options.mux else {
            return Ok(None);
        };
        let media_files = files
            .iter()
            .filter(|file| file.kind.is_media())
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        if media_files.is_empty() {
            return Ok(None);
        }
        let output_path = entry_dir.join(format!("{}.mp4", safe_file_name(&entry.title)));
        let mux_output_path = temporary_mux_path(&output_path);
        remove_file_if_exists(&mux_output_path).await?;
        let mut args = Vec::new();
        args.push(OsString::from("-y"));
        args.push(OsString::from("-nostdin"));
        if only_flv_segments(files) {
            let list_path = entry_dir.join("ffmpeg-concat.txt");
            fs::write(&list_path, concat_file_list(&media_files, entry_dir)).await?;
            args.extend([
                OsString::from("-f"),
                OsString::from("concat"),
                OsString::from("-safe"),
                OsString::from("0"),
                OsString::from("-i"),
                list_path.into_os_string(),
            ]);
        } else {
            for media_file in &media_files {
                args.push(OsString::from("-i"));
                args.push(media_file.as_os_str().to_os_string());
            }
        }
        args.extend([
            OsString::from("-c"),
            OsString::from("copy"),
            mux_output_path.as_os_str().to_os_string(),
        ]);
        let status = Command::new(binary)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await?;
        if !status.success() {
            let _ = fs::remove_file(&mux_output_path).await;
            return Err(Error::MuxFailed {
                status: status.code().map_or_else(
                    || "terminated by signal".to_owned(),
                    |code| code.to_string(),
                ),
            });
        }
        let Ok(metadata) = fs::metadata(&mux_output_path).await else {
            let _ = fs::remove_file(&mux_output_path).await;
            return Err(Error::MuxFailed {
                status: "missing output file".to_owned(),
            });
        };
        if !metadata.is_file() {
            let _ = fs::remove_file(&mux_output_path).await;
            return Err(Error::MuxFailed {
                status: "missing output file".to_owned(),
            });
        }
        if metadata.len() == 0 {
            let _ = fs::remove_file(&mux_output_path).await;
            return Err(Error::MuxFailed {
                status: "empty output file".to_owned(),
            });
        }
        replace_file(&mux_output_path, &output_path).await?;
        Ok(Some(MuxReport {
            output_path,
            command: command_report(binary, &args),
        }))
    }
}

impl DownloadFileKind {
    fn is_media(&self) -> bool {
        matches!(self, Self::Video | Self::Audio | Self::FlvSegment)
    }

    fn expects_non_empty_response(&self) -> bool {
        matches!(
            self,
            Self::Video | Self::Audio | Self::FlvSegment | Self::Cover
        )
    }
}

fn command_report(binary: &Path, args: &[OsString]) -> Vec<String> {
    let mut command = Vec::with_capacity(args.len() + 1);
    command.push(binary.to_string_lossy().into_owned());
    command.extend(args.iter().map(|arg| arg.to_string_lossy().into_owned()));
    command
}

async fn write_response_body_to_file(
    file: &mut tokio::fs::File,
    response: reqwest::Response,
    content_range: Option<ParsedContentRange>,
    start_offset: u64,
    expected_size: Option<u64>,
    download_idle_timeout: Option<Duration>,
) -> Result<u64> {
    let mut bytes_written = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = match next_download_chunk(&mut stream, download_idle_timeout).await {
        Ok(chunk) => chunk,
        Err(error) => {
            rollback_download_file(file, start_offset).await?;
            return Err(error);
        }
    } {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                rollback_download_file(file, start_offset).await?;
                return Err(BiliClient::http_error_without_url(error));
            }
        };
        if let Err(error) = file.write_all(&chunk).await {
            rollback_download_file(file, start_offset).await?;
            return Err(Error::Io(error));
        }
        bytes_written += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
    }
    if let Err(error) = file.flush().await {
        rollback_download_file(file, start_offset).await?;
        return Err(Error::Io(error));
    }
    if let Err(error) =
        validate_download_completion(expected_size, content_range, start_offset, bytes_written)
    {
        rollback_download_file(file, start_offset).await?;
        return Err(error);
    }
    Ok(bytes_written)
}

async fn existing_file_len(path: &Path) -> Result<u64> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(Error::Io(error)),
    }
}

fn temporary_download_path(path: &Path) -> PathBuf {
    temporary_path_with_suffix(path, ".bbdown-download")
}

fn temporary_replace_path(path: &Path) -> PathBuf {
    temporary_path_with_suffix(path, ".bbdown-replace")
}

fn temporary_mux_path(path: &Path) -> PathBuf {
    temporary_path_with_suffix(path, ".bbdown-mux")
}

fn temporary_path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let base = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("download");
    let budget = MAX_FILE_COMPONENT_BYTES.saturating_sub(suffix.len()).max(1);
    path.with_file_name(format!(
        "{}{suffix}",
        safe_file_name_with_budget(base, budget)
    ))
}

fn archive_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let base = path.file_name().map_or_else(
        || "download-archive".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!("{base}{suffix}"))
}

fn archive_storage_path(path: &Path) -> Result<PathBuf> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(path.to_path_buf());
        }
        Err(error) => return Err(Error::Io(error)),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(path.to_path_buf());
    }
    let target = std::fs::read_link(path)?;
    let target = if target.is_absolute() {
        target
    } else {
        path.parent().unwrap_or_else(|| Path::new("")).join(target)
    };
    Ok(canonicalize_existing_prefix(&absolute_path(&target)))
}

fn replace_archive_file(source: &Path, target: &Path) -> Result<()> {
    let target = archive_storage_path(target)?;
    let backup = archive_sidecar_path(&target, ".bbdown-archive-backup");
    match std::fs::remove_file(&backup) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::Io(error)),
    }
    match std::fs::rename(&target, &backup) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::Io(error)),
    }
    match std::fs::rename(source, &target) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup);
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::rename(&backup, &target);
            Err(Error::Io(error))
        }
    }
}

async fn replace_file(source: &Path, target: &Path) -> Result<()> {
    match fs::rename(source, target).await {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
            return Err(Error::Io(error));
        }
        Err(_) => {}
    }

    let backup = temporary_replace_path(target);
    match fs::remove_file(&backup).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::Io(error)),
    }
    fs::rename(target, &backup).await?;
    match fs::rename(source, target).await {
        Ok(()) => {
            let _ = fs::remove_file(&backup).await;
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&backup, target).await;
            Err(Error::Io(error))
        }
    }
}

fn candidate_urls(primary: &str, backups: &[String]) -> Vec<String> {
    let mut urls = Vec::with_capacity(backups.len() + 1);
    urls.push(primary.to_owned());
    urls.extend(backups.iter().filter(|url| !url.is_empty()).cloned());
    urls
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedContentRange {
    start: u64,
    end: u64,
    complete_len: Option<u64>,
}

impl ParsedContentRange {
    fn body_len(self) -> Result<u64> {
        self.end
            .checked_sub(self.start)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| Error::InvalidInput("invalid Content-Range span".to_owned()))
    }

    fn final_len(self) -> Result<u64> {
        self.end
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("invalid Content-Range end".to_owned()))
    }
}

async fn next_download_chunk<S>(
    stream: &mut S,
    idle_timeout: Option<Duration>,
) -> Result<Option<S::Item>>
where
    S: futures_util::Stream + Unpin,
{
    match idle_timeout {
        Some(timeout) => match tokio::time::timeout(timeout, stream.next()).await {
            Ok(chunk) => Ok(chunk),
            Err(_) => Err(Error::InvalidInput(
                "download idle timeout elapsed".to_owned(),
            )),
        },
        None => Ok(stream.next().await),
    }
}

async fn rollback_download_file(file: &tokio::fs::File, len: u64) -> Result<()> {
    file.set_len(len).await?;
    Ok(())
}

async fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Io(error)),
    }
}

async fn remove_output_root_if_exists(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::Io(error)),
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path).await?;
    } else {
        fs::remove_file(path).await?;
    }
    Ok(())
}

fn validate_resume_response(
    status: StatusCode,
    resume_from: u64,
    has_content_range: bool,
    content_range: Option<ParsedContentRange>,
) -> Result<bool> {
    let append = resume_from > 0 && status == StatusCode::PARTIAL_CONTENT;
    if status != StatusCode::PARTIAL_CONTENT && has_content_range {
        return Err(Error::InvalidInput(
            "server returned Content-Range without partial content".to_owned(),
        ));
    }
    if status == StatusCode::PARTIAL_CONTENT {
        let range = content_range.ok_or_else(|| {
            Error::InvalidInput("server returned partial content without Content-Range".to_owned())
        })?;
        let expected_start = if append { resume_from } else { 0 };
        if range.start != expected_start {
            return Err(Error::InvalidInput(
                "server returned an unexpected Content-Range for resume".to_owned(),
            ));
        }
    }
    Ok(append)
}

fn validation_size_for_full_retry(
    expected_size: Option<u64>,
    content_range: Option<ParsedContentRange>,
    response_content_len: Option<u64>,
    full_retry_after_ignored_range: bool,
) -> Option<u64> {
    expected_size.or_else(|| {
        if full_retry_after_ignored_range && content_range.is_none() {
            response_content_len
        } else {
            None
        }
    })
}

fn is_unexpected_empty_response(kind: &DownloadFileKind, bytes_written: u64) -> bool {
    kind.expects_non_empty_response() && bytes_written == 0
}

fn validate_download_completion(
    expected_size: Option<u64>,
    content_range: Option<ParsedContentRange>,
    start_offset: u64,
    bytes_written: u64,
) -> Result<()> {
    if let Some(range) = content_range {
        let range_body_len = range.body_len()?;
        if bytes_written != range_body_len {
            return Err(Error::InvalidInput(
                "download body length did not match Content-Range".to_owned(),
            ));
        }
        let final_len = range.final_len()?;
        if let Some(total) = range.complete_len {
            if final_len != total {
                return Err(Error::InvalidInput(
                    "download did not reach Content-Range total length".to_owned(),
                ));
            }
            if expected_size.is_some_and(|size| size != total) {
                return Err(Error::InvalidInput(
                    "Content-Range total length did not match expected media size".to_owned(),
                ));
            }
        } else if let Some(size) = expected_size {
            if size != final_len {
                return Err(Error::InvalidInput(
                    "downloaded file length did not match expected media size".to_owned(),
                ));
            }
        } else {
            return Err(Error::InvalidInput(
                "Content-Range total length is unknown".to_owned(),
            ));
        }
        if expected_size.is_some_and(|size| size != final_len) {
            return Err(Error::InvalidInput(
                "downloaded file length did not match expected media size".to_owned(),
            ));
        }
        return Ok(());
    }

    if let Some(expected_size) = expected_size {
        let final_len = start_offset
            .checked_add(bytes_written)
            .ok_or_else(|| Error::InvalidInput("downloaded file length overflowed".to_owned()))?;
        if final_len != expected_size {
            return Err(Error::InvalidInput(
                "downloaded file length did not match expected media size".to_owned(),
            ));
        }
    }
    Ok(())
}

fn content_range(headers: &reqwest::header::HeaderMap) -> Result<Option<ParsedContentRange>> {
    let Some(value) = headers.get(CONTENT_RANGE) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| Error::InvalidInput("invalid Content-Range".to_owned()))?;
    let range = value
        .strip_prefix("bytes ")
        .ok_or_else(|| Error::InvalidInput("invalid Content-Range".to_owned()))?;
    if range.starts_with("*/") {
        return Ok(None);
    }
    let (span, complete) = range
        .split_once('/')
        .ok_or_else(|| Error::InvalidInput("invalid Content-Range".to_owned()))?;
    let (start, end) = span
        .split_once('-')
        .ok_or_else(|| Error::InvalidInput("invalid Content-Range".to_owned()))?;
    let complete_len = if complete == "*" {
        None
    } else {
        Some(
            complete
                .parse()
                .map_err(|_| Error::InvalidInput("invalid Content-Range".to_owned()))?,
        )
    };
    Ok(Some(ParsedContentRange {
        start: start
            .parse()
            .map_err(|_| Error::InvalidInput("invalid Content-Range".to_owned()))?,
        end: end
            .parse()
            .map_err(|_| Error::InvalidInput("invalid Content-Range".to_owned()))?,
        complete_len,
    }))
}

fn content_range_complete_len(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let value = headers.get(CONTENT_RANGE)?.to_str().ok()?;
    value.strip_prefix("bytes */")?.parse().ok()
}

fn only_flv_segments(files: &[DownloadedFile]) -> bool {
    let media = files
        .iter()
        .filter(|file| file.kind.is_media())
        .collect::<Vec<_>>();
    !media.is_empty()
        && media
            .iter()
            .all(|file| file.kind == DownloadFileKind::FlvSegment)
}

fn should_download_cover(options: &DownloadOptions) -> bool {
    match options.mode {
        DownloadMode::All => options.sidecars.cover,
        DownloadMode::CoverOnly => true,
        DownloadMode::VideoOnly
        | DownloadMode::AudioOnly
        | DownloadMode::SubtitleOnly
        | DownloadMode::DanmakuOnly => false,
    }
}

fn should_download_subtitles(options: &DownloadOptions) -> bool {
    match options.mode {
        DownloadMode::All => options.include_subtitles && options.sidecars.subtitles,
        DownloadMode::SubtitleOnly => true,
        DownloadMode::VideoOnly
        | DownloadMode::AudioOnly
        | DownloadMode::DanmakuOnly
        | DownloadMode::CoverOnly => false,
    }
}

fn should_download_danmaku(options: &DownloadOptions) -> bool {
    match options.mode {
        DownloadMode::All => options.include_danmaku && options.sidecars.danmaku,
        DownloadMode::DanmakuOnly => true,
        DownloadMode::VideoOnly
        | DownloadMode::AudioOnly
        | DownloadMode::SubtitleOnly
        | DownloadMode::CoverOnly => false,
    }
}

fn concat_file_list(paths: &[PathBuf], base: &Path) -> String {
    paths.iter().fold(String::new(), |mut output, path| {
        let list_path = path.strip_prefix(base).unwrap_or(path);
        let escaped = list_path.to_string_lossy().replace('\'', "'\\''");
        let _ = writeln!(output, "file '{escaped}'");
        output
    })
}

fn default_plan_output_dir(plan: &DownloadPlan, options: &DownloadOptions) -> PathBuf {
    options.output_dir.join(safe_file_name(&plan.title))
}

fn next_available_output_dir_avoiding(base: &Path, reserved: &[PathBuf]) -> Result<PathBuf> {
    if !path_is_occupied(base)? && !path_is_reserved(base, reserved) {
        return Ok(base.to_path_buf());
    }
    let parent = base.parent().unwrap_or_else(|| Path::new(""));
    let stem = base
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("download");
    let mut index = 2;
    loop {
        let candidate = parent.join(format!("{stem} ({index})"));
        if !path_is_occupied(&candidate)? && !path_is_reserved(&candidate, reserved) {
            return Ok(candidate);
        }
        index += 1;
    }
}

fn path_is_occupied(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::Io(error)),
    }
}

fn path_is_reserved(path: &Path, reserved: &[PathBuf]) -> bool {
    let path_key = comparable_output_path_key(path);
    reserved
        .iter()
        .any(|reserved_path| comparable_output_path_key(reserved_path) == path_key)
}

fn validate_archive_preflight(
    plan: &DownloadPlan,
    options: &DownloadOptions,
    archive: &DownloadArchive,
    preflight: &DownloadPreflight,
) -> Result<()> {
    if preflight.content_key != download_plan_content_key_for_mode(plan, options.mode) {
        return Err(Error::InvalidInput(
            "download preflight does not match the download plan".to_owned(),
        ));
    }
    let planned_output_dir = default_plan_output_dir(plan, options);
    if comparable_output_path_key(&preflight.planned_output_dir)
        != comparable_output_path_key(&planned_output_dir)
    {
        return Err(Error::InvalidInput(
            "download preflight does not match the download output options".to_owned(),
        ));
    }
    let current_archived_records =
        archive_records_for_preflight(archive, plan, options.mode, &planned_output_dir);
    if preflight.archived_records != current_archived_records {
        return Err(Error::InvalidInput(
            "download preflight does not match the current download archive".to_owned(),
        ));
    }
    let current_reserved_keys = archive
        .records
        .iter()
        .map(|record| comparable_output_path_key(&record.output_dir))
        .collect::<HashSet<_>>();
    let preflight_reserved_keys = preflight
        .reserved_output_dirs_for_decision()
        .iter()
        .map(|path| comparable_output_path_key(path))
        .collect::<HashSet<_>>();
    if preflight_reserved_keys != current_reserved_keys {
        return Err(Error::InvalidInput(
            "download preflight does not match the current download archive".to_owned(),
        ));
    }
    Ok(())
}

fn archive_records_for_preflight(
    archive: &DownloadArchive,
    plan: &DownloadPlan,
    mode: DownloadMode,
    planned_output_dir: &Path,
) -> Vec<DownloadArchiveRecord> {
    let plan_match = ArchivePlanMatch::new(plan, mode);
    let output_key = comparable_output_path_key(planned_output_dir);
    archive
        .records
        .iter()
        .filter(|record| {
            plan_match.matches_record(record)
                || comparable_output_path_key(&record.output_dir) == output_key
        })
        .cloned()
        .collect()
}

struct ArchivePlanMatch {
    content_key: String,
    entry_keys: HashSet<String>,
    accepts_legacy_keys: bool,
}

fn archive_plan_matches_for_all_modes(plan: &DownloadPlan) -> Vec<ArchivePlanMatch> {
    [
        DownloadMode::All,
        DownloadMode::VideoOnly,
        DownloadMode::AudioOnly,
        DownloadMode::SubtitleOnly,
        DownloadMode::DanmakuOnly,
        DownloadMode::CoverOnly,
    ]
    .into_iter()
    .map(|mode| ArchivePlanMatch::new(plan, mode))
    .collect()
}

impl ArchivePlanMatch {
    fn new(plan: &DownloadPlan, mode: DownloadMode) -> Self {
        Self {
            content_key: download_plan_content_key_for_mode(plan, mode),
            entry_keys: plan
                .entries
                .iter()
                .map(|entry| download_entry_content_key_for_mode(entry, mode))
                .collect(),
            accepts_legacy_keys: matches!(mode, DownloadMode::All),
        }
    }

    fn matches_record(&self, record: &DownloadArchiveRecord) -> bool {
        record.content_key == self.content_key
            || record.entries.iter().any(|entry| {
                self.entry_keys.contains(&entry.content_key)
                    || (self.accepts_legacy_keys
                        && !archive_content_key_has_mode(&record.content_key)
                        && !archive_content_key_has_mode(&entry.content_key)
                        && self.entry_keys.contains(&archive_entry_content_key(entry)))
            })
    }
}

fn archive_content_key_has_mode(key: &str) -> bool {
    key.starts_with("mode=")
}

fn archive_record_path(path: &Path) -> PathBuf {
    comparable_output_path(path)
}

fn comparable_output_path_key(path: &Path) -> String {
    comparable_output_path(path)
        .components()
        .map(path_component_key)
        .collect::<Vec<_>>()
        .join("\0")
}

fn comparable_output_path(path: &Path) -> PathBuf {
    canonicalize_existing_prefix(&absolute_path(path))
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    }
}

fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    let mut existing_prefix = path.to_path_buf();
    let mut missing_components = Vec::new();
    while !existing_prefix.exists() {
        let Some(file_name) = existing_prefix.file_name() else {
            break;
        };
        missing_components.push(file_name.to_os_string());
        if !existing_prefix.pop() {
            break;
        }
    }
    let mut normalized = std::fs::canonicalize(&existing_prefix).unwrap_or(existing_prefix);
    for component in missing_components.iter().rev() {
        normalized.push(component);
    }
    lexical_clean_path(&normalized)
}

fn lexical_clean_path(path: &Path) -> PathBuf {
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => clean.push(prefix.as_os_str()),
            std::path::Component::RootDir => clean.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                let _ = clean.pop();
            }
            std::path::Component::Normal(part) => clean.push(part),
        }
    }
    clean
}

fn path_component_key(component: std::path::Component<'_>) -> String {
    let value = component.as_os_str().to_string_lossy();
    if cfg!(windows) || cfg!(target_os = "macos") {
        value.to_lowercase()
    } else {
        value.into_owned()
    }
}

fn download_plan_content_key(plan: &DownloadPlan) -> String {
    let mut key = String::from("plan");
    for entry in &plan.entries {
        key.push('|');
        key.push_str(&download_entry_content_key(entry));
    }
    key
}

fn download_plan_content_key_for_mode(plan: &DownloadPlan, mode: DownloadMode) -> String {
    let key = download_plan_content_key(plan);
    if matches!(mode, DownloadMode::All) {
        key
    } else {
        format!("mode={};{key}", mode.archive_key_token())
    }
}

fn download_entry_content_key(entry: &DownloadEntry) -> String {
    format!("aid={};cid={}", entry.aid, entry.cid)
}

fn download_entry_content_key_for_mode(entry: &DownloadEntry, mode: DownloadMode) -> String {
    let key = download_entry_content_key(entry);
    if matches!(mode, DownloadMode::All) {
        key
    } else {
        format!("mode={};{key}", mode.archive_key_token())
    }
}

fn archive_entry_content_key(entry: &DownloadArchiveEntryRecord) -> String {
    format!("aid={};cid={}", entry.aid, entry.cid)
}

fn entry_dir_name(entry: &DownloadEntry) -> String {
    let prefix = format!("P{:03}-{}-", entry.index, entry_content_identity(entry));
    format_file_component(&prefix, &entry.title, "")
}

fn format_file_component(prefix: &str, variable: &str, suffix: &str) -> String {
    let used = prefix.len().saturating_add(suffix.len());
    let variable_budget = MAX_FILE_COMPONENT_BYTES.saturating_sub(used).max(1);
    let component = format!(
        "{prefix}{}{suffix}",
        safe_file_name_with_budget(variable, variable_budget)
    );
    truncate_utf8_component(&component, MAX_FILE_COMPONENT_BYTES)
}

fn entry_content_identity(entry: &DownloadEntry) -> String {
    let primary = entry
        .bvid
        .as_deref()
        .filter(|value| !value.is_empty())
        .map_or_else(
            || {
                entry
                    .epid
                    .map_or_else(|| format!("av{}", entry.aid), |epid| format!("ep{epid}"))
            },
            safe_file_name,
        );
    format!("{primary}-cid{}", entry.cid)
}

fn safe_file_name(raw: &str) -> String {
    safe_file_name_with_budget(raw, MAX_FILE_NAME_BYTES)
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn safe_file_name_with_budget(raw: &str, max_bytes: usize) -> String {
    let mut value = raw
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            character if character.is_control() => '-',
            character => character,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_owned();
    if value.is_empty() {
        "untitled".clone_into(&mut value);
    }
    truncate_utf8_component(&value, max_bytes)
}

fn truncate_utf8_component(value: &str, max_bytes: usize) -> String {
    let limit = max_bytes.max(1);
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let next = index + character.len_utf8();
        if next > limit {
            break;
        }
        end = next;
    }
    let truncated = value[..end].trim().trim_matches(['-', '.', '_']).to_owned();
    if truncated.is_empty() {
        fallback_file_component(limit)
    } else {
        truncated
    }
}

fn fallback_file_component(max_bytes: usize) -> String {
    if max_bytes >= "untitled".len() {
        "untitled".to_owned()
    } else {
        "u".repeat(max_bytes.max(1))
    }
}

fn media_extension(url: &str, mime_type: Option<&str>) -> &'static str {
    if mime_type.is_some_and(|value| value.contains("mp4")) {
        return "m4s";
    }
    if url_path_extension(url).is_some_and(|extension| extension.eq_ignore_ascii_case("flv")) {
        return "flv";
    }
    "m4s"
}

fn media_file_name(label: &str, stream: &MediaStream) -> String {
    let identity = media_stream_identity(stream);
    let prefix = format!("{label}-{}-", stream.id);
    let suffix = format!(
        ".{}",
        media_extension(&stream.base_url, stream.mime_type.as_deref())
    );
    format_file_component(&prefix, &identity, &suffix)
}

fn select_media_stream<'a>(
    streams: &'a [MediaStream],
    requested_id: Option<u32>,
    kind: &str,
) -> Result<&'a MediaStream> {
    if let Some(id) = requested_id {
        return streams.iter().find(|stream| stream.id == id).ok_or_else(|| {
            Error::InvalidInput(format!(
                "requested {kind} quality {id} is not available; available {kind} qualities: {}",
                available_stream_ids(streams)
            ))
        });
    }
    streams
        .first()
        .ok_or(Error::MissingField("selected media stream"))
}

fn validate_plan_stream_selection(plan: &DownloadPlan, options: &DownloadOptions) -> Result<()> {
    let selection = options.stream_selection;
    if !selection.has_selection() {
        return Ok(());
    }
    validate_stream_selection_mode(selection, options.mode)?;
    for entry in &plan.entries {
        validate_entry_stream_selection(entry, selection, options.mode)?;
    }
    Ok(())
}

fn validate_stream_selection_mode(selection: StreamSelection, mode: DownloadMode) -> Result<()> {
    match mode {
        DownloadMode::VideoOnly if selection.audio_quality.is_some() => Err(Error::InvalidInput(
            "audio quality selection requires all or audio-only download mode".to_owned(),
        )),
        DownloadMode::AudioOnly if selection.video_quality.is_some() => Err(Error::InvalidInput(
            "video quality selection requires all or video-only download mode".to_owned(),
        )),
        DownloadMode::All | DownloadMode::VideoOnly | DownloadMode::AudioOnly => Ok(()),
        DownloadMode::SubtitleOnly | DownloadMode::DanmakuOnly | DownloadMode::CoverOnly => {
            Err(Error::InvalidInput(
                "stream quality selection requires a media download mode".to_owned(),
            ))
        }
    }
}

fn validate_entry_stream_selection(
    entry: &DownloadEntry,
    selection: StreamSelection,
    mode: DownloadMode,
) -> Result<()> {
    match mode {
        DownloadMode::All => {
            let has_dash_pair =
                !entry.streams.videos.is_empty() && !entry.streams.audios.is_empty();
            let use_flv_fallback = !has_dash_pair && !entry.streams.flv_segments.is_empty();
            if has_dash_pair {
                let _ =
                    select_media_stream(&entry.streams.videos, selection.video_quality, "video")?;
                let _ =
                    select_media_stream(&entry.streams.audios, selection.audio_quality, "audio")?;
            } else if use_flv_fallback {
                return Err(Error::InvalidInput(
                    "stream quality selection requires DASH media; selected entry only has FLV segments"
                        .to_owned(),
                ));
            } else {
                return Err(Error::MissingField("complete DASH media or FLV segments"));
            }
        }
        DownloadMode::VideoOnly => {
            let _ = select_media_stream(&entry.streams.videos, selection.video_quality, "video")?;
        }
        DownloadMode::AudioOnly => {
            let _ = select_media_stream(&entry.streams.audios, selection.audio_quality, "audio")?;
        }
        DownloadMode::SubtitleOnly | DownloadMode::DanmakuOnly | DownloadMode::CoverOnly => {}
    }
    Ok(())
}

fn available_stream_ids(streams: &[MediaStream]) -> String {
    if streams.is_empty() {
        return "none".to_owned();
    }
    let mut ids = Vec::new();
    for stream in streams {
        if !ids.contains(&stream.id) {
            ids.push(stream.id);
        }
    }
    ids.into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn media_stream_identity(stream: &MediaStream) -> String {
    let mut parts = Vec::new();
    push_identity_part(&mut parts, stream.codecs.as_deref());
    push_identity_part(&mut parts, stream.mime_type.as_deref());
    if let Some(bandwidth) = stream.bandwidth {
        parts.push(format!("bw{bandwidth}"));
    }
    if let (Some(width), Some(height)) = (stream.width, stream.height) {
        parts.push(format!("{width}x{height}"));
    }
    push_identity_part(&mut parts, stream.frame_rate.as_deref());
    if let Some(size) = stream.size {
        parts.push(format!("s{size}"));
    }
    if !parts.is_empty() {
        return parts.join("-");
    }
    short_identity_hash(&url_identity_source(&stream.base_url))
}

fn push_identity_part(parts: &mut Vec<String>, value: Option<&str>) {
    if let Some(token) = value.map(file_name_token).filter(|token| !token.is_empty()) {
        parts.push(token);
    }
}

fn file_name_token(raw: &str) -> String {
    raw.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else if matches!(character, '.' | '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(['-', '.', '_'])
        .to_owned()
}

fn short_identity_hash(raw: &str) -> String {
    let digest = format!("{:x}", Md5::digest(raw.as_bytes()));
    format!("h{}", &digest[..8])
}

fn url_identity_source(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        let path = parsed.path();
        if !path.is_empty() {
            return path.to_owned();
        }
    }
    url.split(['?', '#']).next().unwrap_or(url).to_owned()
}

fn subtitle_extension(subtitle: &SubtitleTrack) -> String {
    let extension = match subtitle.format {
        SubtitleFormat::Json => "json".to_owned(),
        SubtitleFormat::Ass => "ass".to_owned(),
        SubtitleFormat::Unknown => {
            url_path_extension(&subtitle.url).unwrap_or_else(|| "subtitle".to_owned())
        }
    };
    let sanitized = file_name_token(&extension);
    let extension = if sanitized.is_empty() {
        "subtitle".to_owned()
    } else {
        sanitized
    };
    safe_file_name_with_budget(&extension, MAX_SUBTITLE_EXTENSION_BYTES)
}

fn subtitle_file_name(index: usize, subtitle: &SubtitleTrack) -> String {
    let prefix = "subtitle-";
    let suffix = format!(
        "-{:02}-{}.{}",
        index.saturating_add(1),
        short_identity_hash(&subtitle.url),
        subtitle_extension(subtitle)
    );
    format_file_component(prefix, &subtitle.language, &suffix)
}

fn cover_file_name(url: &str) -> String {
    let extension = cover_extension(url);
    let suffix = format!("-{}.{}", short_identity_hash(url), extension);
    format_file_component("cover-", "image", &suffix)
}

fn cover_extension(url: &str) -> String {
    let extension = url_path_extension(url).unwrap_or_else(|| "jpg".to_owned());
    let sanitized = file_name_token(&extension);
    let extension = if sanitized.is_empty() {
        "jpg".to_owned()
    } else {
        sanitized
    };
    safe_file_name_with_budget(&extension, MAX_COVER_EXTENSION_BYTES)
}

fn url_path_extension(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| {
            Path::new(parsed.path())
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .map(ToOwned::to_owned)
        })
        .filter(|extension| !extension.is_empty())
}

fn subtitle_dedup_key(url: &str) -> String {
    url::Url::parse(url).map_or_else(
        |_| url.split('#').next().unwrap_or(url).to_owned(),
        |mut parsed| {
            parsed.set_fragment(None);
            parsed.to_string()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DownloadArchive, DownloadArchiveEntryRecord, DownloadArchiveRecord, DownloadMode,
        DownloadOptions, DownloadPreflight, DownloadReport, DownloadedFile, DuplicateDecision,
        EntryDownloadReport, MAX_FILE_COMPONENT_BYTES, MAX_FILE_NAME_BYTES,
        MAX_SUBTITLE_EXTENSION_BYTES, MuxOptions, RetryPolicy, SidecarOptions, StreamSelection,
        archive_sidecar_path, comparable_output_path, cover_file_name, default_plan_output_dir,
        download_entry_content_key, download_plan_content_key, entry_dir_name, media_file_name,
        path_is_occupied, safe_file_name, safe_file_name_with_budget, select_media_stream,
        subtitle_dedup_key, subtitle_extension, subtitle_file_name, temporary_download_path,
        temporary_mux_path, temporary_replace_path,
    };
    use crate::models::{
        DanmakuTrack, DownloadEntry, DownloadPlan, FlvSegment, MediaStream, StreamDiagnostics,
        StreamQuality, StreamSet, StreamSource, SubtitleFormat, SubtitleTrack,
    };
    use crate::{BiliClient, ClientConfig, Credentials, DownloadFileKind};
    use httpmock::MockServer;
    use httpmock::prelude::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::time::Duration;
    #[cfg(unix)]
    use std::{fs as std_fs, os::unix::fs::PermissionsExt};

    #[test]
    fn media_file_name_distinguishes_stream_identity_without_query() {
        let mut stream = MediaStream {
            id: 80,
            base_url: "https://cdn.example/path/video.m4s?token=first".to_owned(),
            backup_urls: Vec::new(),
            codecs: Some("avc1.640028".to_owned()),
            bandwidth: None,
            width: None,
            height: None,
            frame_rate: None,
            mime_type: Some("video/mp4".to_owned()),
            size: None,
        };
        let first = media_file_name("video", &stream);
        stream.base_url = "https://cdn.example/path/video.m4s?token=second".to_owned();
        assert_eq!(media_file_name("video", &stream), first);
        stream.base_url = "https://mirror.example/other/path/video.m4s?token=third".to_owned();
        assert_eq!(media_file_name("video", &stream), first);
        stream.codecs = Some("hev1.1.6.L120.90".to_owned());
        assert_ne!(media_file_name("video", &stream), first);
        stream.codecs = None;
        stream.mime_type = None;
        assert!(media_file_name("video", &stream).starts_with("video-80-h"));
    }

    #[test]
    fn select_media_stream_reports_available_ids() -> crate::Result<()> {
        let streams = vec![
            media_stream(80, "https://cdn.example/80.m4s"),
            media_stream(80, "https://cdn.example/80-hevc.m4s"),
            media_stream(64, "https://cdn.example/64.m4s"),
        ];

        let selected = select_media_stream(&streams, Some(64), "video")?;
        assert_eq!(selected.id, 64);

        let Err(error) = select_media_stream(&streams, Some(32), "video") else {
            return Err(crate::Error::InvalidInput(
                "unexpectedly selected missing video stream".to_owned(),
            ));
        };
        assert_eq!(
            error.to_string(),
            "invalid input: requested video quality 32 is not available; available video qualities: 80, 64"
        );
        Ok(())
    }

    #[test]
    fn download_options_builders_configure_embedding_controls() -> anyhow::Result<()> {
        let options = DownloadOptions::new("downloads")
            .with_retry_policy(RetryPolicy::new(5, Duration::from_secs(2)))
            .with_stream_selection(super::StreamSelection::video(80))
            .with_download_idle_timeout(None)
            .with_resume(false)
            .with_cover(true)
            .with_subtitles(false)
            .with_danmaku(false)
            .with_mux(MuxOptions::ffmpeg("ffmpeg-custom"));

        assert_eq!(options.output_dir.as_path(), Path::new("downloads"));
        assert_eq!(options.retry.max_attempts, 5);
        assert_eq!(options.retry.backoff, Duration::from_secs(2));
        assert_eq!(options.stream_selection.video_quality, Some(80));
        assert_eq!(options.stream_selection.audio_quality, None);
        assert_eq!(options.download_idle_timeout, None);
        assert!(!options.resume);
        assert!(!options.include_subtitles);
        assert!(!options.include_danmaku);
        assert!(options.sidecars.cover);
        assert!(!options.sidecars.subtitles);
        assert!(!options.sidecars.danmaku);
        let MuxOptions::Ffmpeg { binary } = options.mux else {
            return Err(anyhow::anyhow!("expected ffmpeg mux options"));
        };
        assert_eq!(binary.as_path(), Path::new("ffmpeg-custom"));

        let audio_selection = super::StreamSelection::audio(30216);
        assert_eq!(audio_selection.video_quality, None);
        assert_eq!(audio_selection.audio_quality, Some(30216));
        Ok(())
    }

    #[test]
    fn safe_file_name_limits_utf8_bytes() {
        let raw = "界".repeat(200);
        let name = safe_file_name(&raw);

        assert!(name.len() <= MAX_FILE_NAME_BYTES);
        assert!(std::str::from_utf8(name.as_bytes()).is_ok());
    }

    #[test]
    fn safe_file_name_with_tiny_budget_stays_in_budget() {
        let name = safe_file_name_with_budget("界", 1);

        assert!(name.len() <= 1);
        assert_eq!(name, "u");
    }

    #[test]
    fn entry_dir_name_limits_final_component_bytes() {
        let server = MockServer::start();
        let mut plan = test_plan(&server);
        plan.entries[0].title = "界".repeat(200);

        assert!(entry_dir_name(&plan.entries[0]).len() <= MAX_FILE_COMPONENT_BYTES);
    }

    #[test]
    fn entry_dir_name_distinguishes_content_identity() {
        let server = MockServer::start();
        let first = test_plan(&server);
        let mut second = test_plan(&server);
        second.entries[0].aid = 170_002;
        second.entries[0].bvid = Some("BV1yy411c7mD".to_owned());
        second.entries[0].cid = 3;

        assert_ne!(
            entry_dir_name(&first.entries[0]),
            entry_dir_name(&second.entries[0])
        );
    }

    #[test]
    fn subtitle_file_name_distinguishes_duplicate_languages() {
        let first = SubtitleTrack {
            language: "en".to_owned(),
            language_doc: Some("English".to_owned()),
            url: "https://subtitle.example/first.ass".to_owned(),
            format: SubtitleFormat::Ass,
        };
        let second = SubtitleTrack {
            language: "en".to_owned(),
            language_doc: Some("English".to_owned()),
            url: "https://subtitle.example/second.ass".to_owned(),
            format: SubtitleFormat::Ass,
        };

        assert_ne!(
            subtitle_file_name(0, &first),
            subtitle_file_name(1, &second)
        );
    }

    #[test]
    fn subtitle_file_name_limits_unknown_extension_bytes() {
        let subtitle = SubtitleTrack {
            language: "en".to_owned(),
            language_doc: Some("English".to_owned()),
            url: format!("https://subtitle.example/file.{}", "x".repeat(200)),
            format: SubtitleFormat::Unknown,
        };

        assert!(subtitle_extension(&subtitle).len() <= MAX_SUBTITLE_EXTENSION_BYTES);
        assert!(subtitle_file_name(0, &subtitle).len() <= MAX_FILE_COMPONENT_BYTES);
    }

    #[test]
    fn download_entry_content_key_ignores_display_index() {
        let server = MockServer::start();
        let mut first = test_plan(&server).entries.remove(0);
        let mut second = first.clone();
        first.index = 1;
        second.index = 99;

        assert_eq!(
            download_entry_content_key(&first),
            download_entry_content_key(&second)
        );
    }

    #[test]
    fn download_entry_content_key_matches_episode_and_video_forms() {
        let server = MockServer::start();
        let mut video_entry = test_plan(&server).entries.remove(0);
        let mut episode_entry = video_entry.clone();
        video_entry.epid = None;
        video_entry.bvid = Some("BV1xx411c7mD".to_owned());
        episode_entry.epid = Some(664_928);
        episode_entry.bvid = None;

        assert_eq!(
            download_entry_content_key(&video_entry),
            download_entry_content_key(&episode_entry)
        );
    }

    #[test]
    fn temporary_file_names_reserve_suffix_budget() {
        let path = std::path::PathBuf::from("a".repeat(MAX_FILE_COMPONENT_BYTES));

        for temporary in [
            temporary_download_path(&path),
            temporary_replace_path(&path),
            temporary_mux_path(&path),
        ] {
            assert!(
                temporary
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.len() <= MAX_FILE_COMPONENT_BYTES)
            );
        }
    }

    #[test]
    fn subtitle_dedup_key_ignores_url_fragment() {
        assert_eq!(
            subtitle_dedup_key("https://subtitle.example/track.ass#first"),
            subtitle_dedup_key("https://subtitle.example/track.ass#second")
        );
    }

    #[tokio::test]
    async fn downloads_media_sidecars_and_danmaku() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        server.mock(|when, then| {
            when.method(GET).path("/cover.jpg");
            then.status(200).body("cover");
        });
        server.mock(|when, then| {
            when.method(GET).path("/subtitle.ass");
            then.status(200).body("[Script Info]");
        });
        server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200).body("<i/>");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        cover: true,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        let entry = &report.entries[0];
        assert_eq!(entry.files.len(), 5);
        let video = entry
            .files
            .iter()
            .find(|file| file.kind == DownloadFileKind::Video)
            .ok_or_else(|| anyhow::anyhow!("missing video"))?;
        assert_eq!(tokio::fs::read_to_string(&video.path).await?, "video");
        let cover = entry
            .files
            .iter()
            .find(|file| file.kind == DownloadFileKind::Cover)
            .ok_or_else(|| anyhow::anyhow!("missing cover"))?;
        assert_eq!(tokio::fs::read_to_string(&cover.path).await?, "cover");
        assert!(
            cover
                .path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| {
                    name.starts_with("cover-image-h")
                        && Path::new(name)
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("jpg"))
                })
        );
        assert!(entry.mux.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn download_video_only_skips_audio_sidecars_and_mux() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        let audio_mock = server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let cover_mock = server.mock(|when, then| {
            when.method(GET).path("/cover.jpg");
            then.status(200).body("cover");
        });
        let subtitle_mock = server.mock(|when, then| {
            when.method(GET).path("/subtitle.ass");
            then.status(200).body("[Script Info]");
        });
        let danmaku_mock = server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200).body("<i/>");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);

        let report = client
            .download_plan(
                &plan,
                DownloadOptions::new(temp.path())
                    .with_retry_policy(RetryPolicy::single_attempt())
                    .with_download_mode(DownloadMode::VideoOnly)
                    .with_cover(true)
                    .with_mux(MuxOptions::ffmpeg("ffmpeg-not-used")),
            )
            .await?;

        let entry = &report.entries[0];
        assert_eq!(entry.files.len(), 1);
        assert_eq!(entry.files[0].kind, DownloadFileKind::Video);
        assert_eq!(
            tokio::fs::read_to_string(&entry.files[0].path).await?,
            "video"
        );
        assert!(entry.mux.is_none());
        assert_eq!(audio_mock.calls(), 0);
        assert_eq!(cover_mock.calls(), 0);
        assert_eq!(subtitle_mock.calls(), 0);
        assert_eq!(danmaku_mock.calls(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn download_audio_only_skips_video_sidecars_and_mux() -> anyhow::Result<()> {
        let server = MockServer::start();
        let video_mock = server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let cover_mock = server.mock(|when, then| {
            when.method(GET).path("/cover.jpg");
            then.status(200).body("cover");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);

        let report = client
            .download_plan(
                &plan,
                DownloadOptions::new(temp.path())
                    .with_retry_policy(RetryPolicy::single_attempt())
                    .with_download_mode(DownloadMode::AudioOnly)
                    .with_mux(MuxOptions::ffmpeg("ffmpeg-not-used")),
            )
            .await?;

        let entry = &report.entries[0];
        assert_eq!(entry.files.len(), 1);
        assert_eq!(entry.files[0].kind, DownloadFileKind::Audio);
        assert_eq!(
            tokio::fs::read_to_string(&entry.files[0].path).await?,
            "audio"
        );
        assert!(entry.mux.is_none());
        assert_eq!(video_mock.calls(), 0);
        assert_eq!(cover_mock.calls(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn sidecar_only_modes_do_not_require_media_streams() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/cover.jpg");
            then.status(200).body("cover");
        });
        server.mock(|when, then| {
            when.method(GET).path("/subtitle.ass");
            then.status(200).body("[Script Info]");
        });
        server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200).body("<i/>");
        });
        let client = BiliClient::new(ClientConfig::default());

        for (mode, kind, expected_body) in [
            (DownloadMode::CoverOnly, DownloadFileKind::Cover, "cover"),
            (
                DownloadMode::SubtitleOnly,
                DownloadFileKind::Subtitle,
                "[Script Info]",
            ),
            (DownloadMode::DanmakuOnly, DownloadFileKind::Danmaku, "<i/>"),
        ] {
            let temp = tempfile::tempdir()?;
            let mut plan = test_plan(&server);
            plan.entries[0].streams.videos.clear();
            plan.entries[0].streams.audios.clear();
            let report = client
                .download_plan(
                    &plan,
                    DownloadOptions::new(temp.path())
                        .with_retry_policy(RetryPolicy::single_attempt())
                        .with_download_mode(mode)
                        .with_mux(MuxOptions::ffmpeg("ffmpeg-not-used")),
                )
                .await?;
            let entry = &report.entries[0];

            assert_eq!(entry.files.len(), 1);
            assert_eq!(entry.files[0].kind, kind);
            assert_eq!(
                tokio::fs::read_to_string(&entry.files[0].path).await?,
                expected_body
            );
            assert!(entry.mux.is_none());
        }
        Ok(())
    }

    #[tokio::test]
    async fn sidecar_only_modes_reject_stream_quality_selection() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions::new(temp.path())
                    .with_stream_selection(StreamSelection::video(80))
                    .with_download_mode(DownloadMode::CoverOnly),
            )
            .await
        else {
            return Err(anyhow::anyhow!(
                "sidecar-only stream quality selection should fail"
            ));
        };

        assert!(
            error
                .to_string()
                .contains("stream quality selection requires a media download mode")
        );
        Ok(())
    }

    #[tokio::test]
    async fn can_skip_cover_sidecar_downloads() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let cover_mock = server.mock(|when, then| {
            when.method(GET).path("/cover.jpg");
            then.status(200).body("cover");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);

        let report = client
            .download_plan(
                &plan,
                DownloadOptions::new(temp.path())
                    .with_retry_policy(RetryPolicy::single_attempt())
                    .with_cover(false)
                    .with_subtitles(false)
                    .with_danmaku(false)
                    .with_mux(MuxOptions::Disabled),
            )
            .await?;

        assert_eq!(cover_mock.calls(), 0);
        assert!(
            report.entries[0]
                .files
                .iter()
                .all(|file| file.kind != DownloadFileKind::Cover)
        );
        Ok(())
    }

    #[tokio::test]
    async fn rejects_empty_cover_sidecar_response() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        server.mock(|when, then| {
            when.method(GET).path("/cover.jpg");
            then.status(200).body("");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let cover_url = plan.entries[0]
            .cover_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("missing cover URL"))?;
        let cover_path = temp
            .path()
            .join(safe_file_name(&plan.title))
            .join(entry_dir_name(&plan.entries[0]))
            .join(cover_file_name(cover_url));

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        cover: true,
                        subtitles: false,
                        danmaku: false,
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("empty cover response should fail"));
        };

        assert!(error.to_string().contains("empty media response"));
        assert!(!cover_path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn invalid_audio_selection_fails_before_media_writes() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let output_dir = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: output_dir.clone(),
                    retry: RetryPolicy::single_attempt(),
                    stream_selection: super::StreamSelection {
                        video_quality: Some(80),
                        audio_quality: Some(30216),
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("missing audio selection should fail"));
        };

        assert!(error.to_string().contains("requested audio quality 30216"));
        assert!(!output_dir.exists());
        Ok(())
    }

    #[tokio::test]
    async fn multi_entry_invalid_selection_fails_before_any_media_writes() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let output_dir = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        let mut second = plan.entries[0].clone();
        second.index = 2;
        second.cid = 3;
        second.title = "Second".to_owned();
        second.streams.audios[0].id = 30216;
        plan.entries.push(second);

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: output_dir.clone(),
                    retry: RetryPolicy::single_attempt(),
                    stream_selection: super::StreamSelection {
                        video_quality: Some(80),
                        audio_quality: Some(30280),
                    },
                    sidecars: SidecarOptions {
                        subtitles: false,
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("later missing audio selection should fail"));
        };

        assert!(error.to_string().contains("requested audio quality 30280"));
        assert!(!output_dir.exists());
        Ok(())
    }

    #[tokio::test]
    async fn skips_duplicate_subtitle_urls() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let subtitle_mock = server.mock(|when, then| {
            when.method(GET).path("/subtitle.ass");
            then.status(200).body("[Script Info]");
        });
        server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200).body("<i/>");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].subtitles.push(SubtitleTrack {
            language: "en".to_owned(),
            language_doc: Some("English duplicate".to_owned()),
            url: format!("{}/subtitle.ass#duplicate", server.base_url()),
            format: SubtitleFormat::Ass,
        });

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        assert_eq!(subtitle_mock.calls(), 1);
        assert_eq!(
            report.entries[0]
                .files
                .iter()
                .filter(|file| file.kind == DownloadFileKind::Subtitle)
                .count(),
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn resumes_partial_files_with_range_request() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(206)
                .header("Content-Range", "bytes 3-5/6")
                .body("new");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].streams.videos[0].size = Some(6);
        plan.entries[0].subtitles.clear();
        plan.entries[0].danmaku.xml_url = format!("{}/danmaku.xml", server.base_url());
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        tokio::fs::write(
            output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0])),
            "old",
        )
        .await?;

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        let file = &report.entries[0].files[0];
        assert_eq!(file.resumed_from, 3);
        assert_eq!(tokio::fs::read_to_string(&file.path).await?, "oldnew");
        Ok(())
    }

    #[tokio::test]
    async fn media_download_does_not_send_cookie_header() -> anyhow::Result<()> {
        let server = MockServer::start();
        let cookie_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("cookie", "SESSDATA=secret");
            then.status(500);
        });
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig {
            credentials: Credentials {
                cookie: Some("SESSDATA=secret".to_owned()),
                access_key: None,
                tv_access_key: None,
            },
            ..ClientConfig::default()
        });
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));

        client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        assert_eq!(cookie_mock.calls(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn falls_back_to_backup_media_url() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/primary.m4s");
            then.status(500);
        });
        server.mock(|when, then| {
            when.method(GET).path("/backup.m4s");
            then.status(200).body("backup");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/primary.m4s", server.base_url()));
        plan.entries[0].streams.videos[0]
            .backup_urls
            .push(format!("{}/backup.m4s", server.base_url()));

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        assert_eq!(
            tokio::fs::read_to_string(&report.entries[0].files[0].path).await?,
            "backup"
        );
        Ok(())
    }

    #[tokio::test]
    async fn matching_416_resume_response_is_treated_as_complete() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(416).header("Content-Range", "bytes */3");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(3);
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        tokio::fs::write(
            output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0])),
            "old",
        )
        .await?;

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        let file = &report.entries[0].files[0];
        assert_eq!(file.bytes_written, 0);
        assert_eq!(file.resumed_from, 3);
        assert_eq!(tokio::fs::read_to_string(&file.path).await?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn rejects_mismatched_content_range_on_resume() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(206)
                .header("Content-Range", "bytes 0-2/6")
                .body("bad");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("mismatched Content-Range should fail"));
        };

        assert!(error.to_string().contains("Content-Range"));
        assert_eq!(tokio::fs::read_to_string(&path).await?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn rejects_content_range_on_non_partial_response() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(200)
                .header("Content-Range", "bytes 3-5/6")
                .body("new");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(6);
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!(
                "non-partial Content-Range response should fail"
            ));
        };

        assert!(error.to_string().contains("partial content"));
        assert_eq!(tokio::fs::read_to_string(&path).await?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn rejects_unsatisfied_content_range_on_non_partial_response() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(200).header("Content-Range", "bytes */3");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!(
                "unsatisfied non-partial Content-Range response should fail"
            ));
        };

        assert!(error.to_string().contains("partial content"));
        assert_eq!(tokio::fs::read_to_string(&path).await?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn preserves_partial_file_when_full_retry_fails_after_ignored_range() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(200).body("new");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(6);
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("short full retry should fail"));
        };

        assert!(error.to_string().contains("expected media size"));
        assert_eq!(tokio::fs::read_to_string(&path).await?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn replaces_partial_file_when_full_retry_succeeds_after_ignored_range()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(200).body("oldnew");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(6);
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        let file = &report.entries[0].files[0];
        assert_eq!(file.bytes_written, 6);
        assert_eq!(file.resumed_from, 0);
        assert_eq!(tokio::fs::read_to_string(&path).await?, "oldnew");
        Ok(())
    }

    #[tokio::test]
    async fn replaces_existing_file_when_ignored_range_has_content_length() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(200).body("maybe-full");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        let file = &report.entries[0].files[0];
        assert_eq!(file.bytes_written, 10);
        assert_eq!(file.resumed_from, 0);
        assert_eq!(tokio::fs::read_to_string(&path).await?, "maybe-full");
        Ok(())
    }

    #[tokio::test]
    async fn rejects_ignored_range_without_length_proof() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || -> anyhow::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer)?;
            stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nmaybe-full")?;
            Ok(())
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let path = temp.path().join("video.m4s");
        tokio::fs::write(&path, "existing").await?;
        let options = DownloadOptions {
            retry: RetryPolicy::single_attempt(),
            sidecars: SidecarOptions {
                danmaku: false,
                ..SidecarOptions::default()
            },
            mux: MuxOptions::Disabled,
            ..DownloadOptions::default()
        };

        let Err(error) = client
            .download_url_to_file(
                &format!("http://{address}/video.m4s"),
                &path,
                DownloadFileKind::Video,
                None,
                &options,
            )
            .await
        else {
            return Err(anyhow::anyhow!("unverified full retry should fail"));
        };

        handle
            .join()
            .map_err(|_| anyhow::anyhow!("server thread panicked"))??;
        assert!(
            error
                .to_string()
                .contains("verifiable full response length")
        );
        assert_eq!(tokio::fs::read_to_string(&path).await?, "existing");
        Ok(())
    }

    #[tokio::test]
    async fn no_resume_preserves_existing_file_when_fresh_write_fails() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("new");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(6);
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    resume: false,
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("invalid fresh write should fail"));
        };

        assert!(error.to_string().contains("expected media size"));
        assert_eq!(tokio::fs::read_to_string(&path).await?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn no_resume_replaces_existing_file_after_fresh_write_validates() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("new");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(3);
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    resume: false,
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        assert_eq!(tokio::fs::read_to_string(&path).await?, "new");
        Ok(())
    }

    #[tokio::test]
    async fn rejects_short_content_range_body_on_resume() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(206)
                .header("Content-Range", "bytes 3-5/6")
                .body("ne");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(6);
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("short Content-Range body should fail"));
        };

        assert!(error.to_string().contains("Content-Range"));
        assert_eq!(tokio::fs::read_to_string(&path).await?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn rejects_unknown_total_content_range_without_expected_size() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(206)
                .header("Content-Range", "bytes 3-5/*")
                .body("new");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!(
                "unknown total Content-Range should fail without expected size"
            ));
        };

        assert!(error.to_string().contains("Content-Range total length"));
        assert_eq!(tokio::fs::read_to_string(&path).await?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn accepts_unknown_total_content_range_when_expected_size_matches() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(206)
                .header("Content-Range", "bytes 3-5/*")
                .body("new");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(6);
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        assert_eq!(tokio::fs::read_to_string(&path).await?, "oldnew");
        Ok(())
    }

    #[tokio::test]
    async fn rejects_content_range_total_mismatch_on_resume() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/video.m4s")
                .header("range", "bytes=3-");
            then.status(206)
                .header("Content-Range", "bytes 3-5/999")
                .body("new");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(6);
        let output_dir = test_entry_dir(temp.path(), &plan);
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        tokio::fs::write(&path, "old").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("Content-Range total mismatch should fail"));
        };

        assert!(error.to_string().contains("Content-Range total length"));
        assert_eq!(tokio::fs::read_to_string(&path).await?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn rejects_expected_media_size_mismatch() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(6);
        let path = temp
            .path()
            .join(safe_file_name(&plan.title))
            .join(entry_dir_name(&plan.entries[0]))
            .join(media_file_name("video", &plan.entries[0].streams.videos[0]));

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("media size mismatch should fail"));
        };

        assert!(error.to_string().contains("expected media size"));
        assert_eq!(tokio::fs::metadata(&path).await?.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_empty_unknown_size_media_response() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let path = temp
            .path()
            .join(safe_file_name(&plan.title))
            .join(entry_dir_name(&plan.entries[0]))
            .join(media_file_name("video", &plan.entries[0].streams.videos[0]));

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("empty media response should fail"));
        };

        assert!(error.to_string().contains("empty media response"));
        assert!(!path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn rejects_empty_zero_size_media_response() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let path = temp.path().join("video.m4s");
        let options = DownloadOptions {
            retry: RetryPolicy::single_attempt(),
            sidecars: SidecarOptions {
                danmaku: false,
                ..SidecarOptions::default()
            },
            mux: MuxOptions::Disabled,
            ..DownloadOptions::default()
        };

        let Err(error) = client
            .download_url_to_file(
                &format!("{}/video.m4s", server.base_url()),
                &path,
                DownloadFileKind::Video,
                Some(0),
                &options,
            )
            .await
        else {
            return Err(anyhow::anyhow!(
                "empty zero-size media response should fail"
            ));
        };

        assert!(error.to_string().contains("empty media response"));
        assert!(!path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn uses_flv_segments_when_dash_pair_is_incomplete() -> anyhow::Result<()> {
        let server = MockServer::start();
        let video_mock = server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(500);
        });
        let flv_mock = server.mock(|when, then| {
            when.method(GET).path("/segment.flv");
            then.status(200).body("segment");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].streams.audios.clear();
        plan.entries[0].streams.flv_segments = vec![FlvSegment {
            order: 1,
            url: format!("{}/segment.flv", server.base_url()),
            backup_urls: Vec::new(),
            size: Some(7),
            length_ms: Some(1000),
        }];
        plan.entries[0].subtitles.clear();

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        assert_eq!(video_mock.calls(), 0);
        assert_eq!(flv_mock.calls(), 1);
        assert_eq!(
            report.entries[0].files[0].kind,
            DownloadFileKind::FlvSegment
        );
        assert_eq!(
            tokio::fs::read_to_string(&report.entries[0].files[0].path).await?,
            "segment"
        );
        Ok(())
    }

    #[tokio::test]
    async fn rejects_incomplete_dash_without_flv_fallback() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].streams.audios.clear();
        plan.entries[0].subtitles.clear();

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("incomplete DASH without FLV should fail"));
        };

        assert!(error.to_string().contains("complete DASH media"));
        Ok(())
    }

    #[tokio::test]
    async fn media_download_uses_idle_timeout_instead_of_request_total_timeout()
    -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || -> anyhow::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer)?;
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")?;
            stream.flush()?;
            std::thread::sleep(Duration::from_millis(100));
            stream.write_all(b"ok")?;
            Ok(())
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig {
            request_timeout: Duration::from_millis(20),
            ..ClientConfig::default()
        });
        let path = temp.path().join("video.m4s");
        let options = DownloadOptions {
            retry: RetryPolicy::single_attempt(),
            sidecars: SidecarOptions {
                danmaku: false,
                ..SidecarOptions::default()
            },
            mux: MuxOptions::Disabled,
            download_idle_timeout: Some(Duration::from_secs(1)),
            ..DownloadOptions::default()
        };

        let file = client
            .download_url_to_file(
                &format!("http://{address}/video.m4s"),
                &path,
                DownloadFileKind::Video,
                Some(2),
                &options,
            )
            .await?;

        handle
            .join()
            .map_err(|_| anyhow::anyhow!("server thread panicked"))??;
        assert_eq!(tokio::fs::read_to_string(&file.path).await?, "ok");
        Ok(())
    }

    #[tokio::test]
    async fn media_download_request_timeout_still_bounds_response_headers() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || -> anyhow::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer)?;
            std::thread::sleep(Duration::from_millis(100));
            Ok(())
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig {
            request_timeout: Duration::from_millis(20),
            ..ClientConfig::default()
        });
        let path = temp.path().join("video.m4s");
        let options = DownloadOptions {
            retry: RetryPolicy::single_attempt(),
            sidecars: SidecarOptions {
                danmaku: false,
                ..SidecarOptions::default()
            },
            mux: MuxOptions::Disabled,
            download_idle_timeout: Some(Duration::from_secs(1)),
            ..DownloadOptions::default()
        };

        let Err(error) = client
            .download_url_to_file(
                &format!("http://{address}/video.m4s"),
                &path,
                DownloadFileKind::Video,
                None,
                &options,
            )
            .await
        else {
            return Err(anyhow::anyhow!("hung response headers should time out"));
        };

        handle
            .join()
            .map_err(|_| anyhow::anyhow!("server thread panicked"))??;
        assert!(error.to_string().contains("download request timeout"));
        Ok(())
    }

    #[tokio::test]
    async fn retries_failed_downloads() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || -> anyhow::Result<()> {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept()?;
                let mut buffer = [0; 1024];
                let _ = stream.read(&mut buffer)?;
                if attempt == 0 {
                    stream.write_all(
                        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n",
                    )?;
                } else {
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nretry-ok")?;
                }
            }
            Ok(())
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let path = temp.path().join("video.m4s");
        let options = DownloadOptions {
            retry: RetryPolicy {
                max_attempts: 2,
                backoff: Duration::from_millis(1),
            },
            sidecars: SidecarOptions {
                danmaku: false,
                ..SidecarOptions::default()
            },
            mux: MuxOptions::Disabled,
            ..DownloadOptions::default()
        };

        let file = client
            .download_url_to_file(
                &format!("http://{address}/video.m4s"),
                &path,
                DownloadFileKind::Video,
                None,
                &options,
            )
            .await?;

        handle
            .join()
            .map_err(|_| anyhow::anyhow!("server thread panicked"))??;
        assert_eq!(tokio::fs::read_to_string(&file.path).await?, "retry-ok");
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_mux_success_is_reported() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let ffmpeg = write_fake_ffmpeg(temp.path(), fake_ffmpeg_creates_output_body())?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let output_dir = temp.path().join("downloads");
        let entry_dir = test_entry_dir(&output_dir, &plan);
        tokio::fs::create_dir_all(&entry_dir).await?;
        tokio::fs::write(entry_dir.join("Main.mp4"), "stale").await?;

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir,
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Ffmpeg { binary: ffmpeg },
                    ..DownloadOptions::default()
                },
            )
            .await?;

        let mux = report.entries[0]
            .mux
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing mux report"))?;
        assert_eq!(mux.command[1], "-y");
        assert!(mux.command.iter().any(|arg| arg == "-nostdin"));
        assert!(mux.command.iter().any(|arg| arg == "-c"));
        assert!(mux.output_path.ends_with("Main.mp4"));
        assert_eq!(tokio::fs::read_to_string(&mux.output_path).await?, "muxed");
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_mux_requires_output_file() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let ffmpeg = write_fake_ffmpeg(temp.path(), "exit 0")?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().join("downloads"),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Ffmpeg { binary: ffmpeg },
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("missing mux output should fail"));
        };

        assert!(error.to_string().contains("missing output file"));
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_mux_rejects_empty_output_file() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let ffmpeg = write_fake_ffmpeg(
            temp.path(),
            "last=\nfor arg do last=$arg; done\n: > \"$last\"\nexit 0",
        )?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().join("downloads"),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Ffmpeg { binary: ffmpeg },
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("empty mux output should fail"));
        };

        assert!(error.to_string().contains("empty output file"));
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_mux_rejects_stale_output_file() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let ffmpeg = write_fake_ffmpeg(temp.path(), "exit 0")?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let entry_dir = test_entry_dir(&temp.path().join("downloads"), &plan);
        tokio::fs::create_dir_all(&entry_dir).await?;
        tokio::fs::write(entry_dir.join("Main.mp4"), "stale").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().join("downloads"),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Ffmpeg { binary: ffmpeg },
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("stale mux output should fail"));
        };

        assert!(error.to_string().contains("missing output file"));
        assert_eq!(
            tokio::fs::read_to_string(entry_dir.join("Main.mp4")).await?,
            "stale"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_mux_failure_is_reported() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let ffmpeg = write_fake_ffmpeg(temp.path(), "exit 7")?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().join("downloads"),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Ffmpeg { binary: ffmpeg },
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("ffmpeg failure should propagate"));
        };

        assert!(error.to_string().contains("status 7"));
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_mux_failure_preserves_existing_output() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let ffmpeg = write_fake_ffmpeg(temp.path(), "exit 7")?;
        let output_dir = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let entry_dir = test_entry_dir(&output_dir, &plan);
        tokio::fs::create_dir_all(&entry_dir).await?;
        let output_path = entry_dir.join("Main.mp4");
        tokio::fs::write(&output_path, "existing").await?;

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir,
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Ffmpeg { binary: ffmpeg },
                    ..DownloadOptions::default()
                },
            )
            .await
        else {
            return Err(anyhow::anyhow!("ffmpeg failure should propagate"));
        };

        assert!(error.to_string().contains("status 7"));
        assert_eq!(tokio::fs::read_to_string(output_path).await?, "existing");
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn flv_mux_concat_file_uses_paths_relative_to_entry_dir() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/segment.flv");
            then.status(200).body("segment");
        });
        let temp = tempfile::Builder::new()
            .prefix(".bbdown-flv-mux-")
            .tempdir_in(".")?;
        let ffmpeg = write_fake_ffmpeg(temp.path(), fake_ffmpeg_creates_output_body())?;
        let output_dir = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].streams.videos.clear();
        plan.entries[0].streams.audios.clear();
        plan.entries[0].streams.flv_segments = vec![FlvSegment {
            order: 1,
            url: format!("{}/segment.flv", server.base_url()),
            backup_urls: Vec::new(),
            size: Some(7),
            length_ms: Some(1000),
        }];
        plan.entries[0].subtitles.clear();

        client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: output_dir.clone(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Ffmpeg { binary: ffmpeg },
                    ..DownloadOptions::default()
                },
            )
            .await?;

        let concat_path = output_dir
            .join(safe_file_name(&plan.title))
            .join(entry_dir_name(&plan.entries[0]))
            .join("ffmpeg-concat.txt");
        assert_eq!(
            tokio::fs::read_to_string(concat_path).await?,
            "file 'segment-001.flv'\n"
        );
        Ok(())
    }

    #[test]
    fn download_preflight_reports_archive_hit_and_output_conflict() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let plan = test_plan(&server);
        let options = DownloadOptions::new(temp.path().join("downloads"));
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        std::fs::create_dir_all(&planned_output_dir)?;
        let record = DownloadArchiveRecord {
            content_key: download_plan_content_key(&plan),
            title: plan.title.clone(),
            output_dir: planned_output_dir.clone(),
            completed_at_unix: 42,
            entries: Vec::new(),
        };
        let archive = DownloadArchive::new(vec![record]);

        let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;

        assert!(preflight.requires_decision());
        assert_eq!(preflight.archived_records.len(), 1);
        assert_eq!(
            preflight
                .output_conflict
                .as_ref()
                .map(|conflict| &conflict.path),
            Some(&planned_output_dir)
        );
        assert_eq!(preflight.suggested_decision(), DuplicateDecision::Cancel);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn download_preflight_reports_broken_symlink_output_conflict() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let plan = test_plan(&server);
        let options = DownloadOptions::new(temp.path().join("downloads"));
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        let output_parent = planned_output_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing planned output parent"))?;
        std::fs::create_dir_all(output_parent)?;
        std::os::unix::fs::symlink(temp.path().join("missing-target"), &planned_output_dir)?;

        assert!(!planned_output_dir.exists());
        let preflight = DownloadPreflight::inspect(&plan, &options, None)?;

        assert!(preflight.requires_decision());
        assert_eq!(
            preflight
                .output_conflict
                .as_ref()
                .map(|conflict| &conflict.path),
            Some(&planned_output_dir)
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn output_occupancy_reports_metadata_errors() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let restricted = temp.path().join("restricted");
        std_fs::create_dir(&restricted)?;
        let original_permissions = std_fs::metadata(&restricted)?.permissions();
        let mut denied_permissions = original_permissions.clone();
        denied_permissions.set_mode(0o000);
        std_fs::set_permissions(&restricted, denied_permissions)?;

        let result = path_is_occupied(&restricted.join("Mock video"));

        std_fs::set_permissions(&restricted, original_permissions)?;
        match result {
            Err(crate::Error::Io(error))
                if error.kind() == std::io::ErrorKind::PermissionDenied => {}
            Ok(occupied) => {
                anyhow::bail!("expected metadata permission error, got occupied={occupied}");
            }
            Err(error) => {
                anyhow::bail!("expected metadata permission error, got {error}");
            }
        }
        Ok(())
    }

    #[test]
    fn download_preflight_reports_entry_overlap_from_archive() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let plan = test_plan(&server);
        let entry = &plan.entries[0];
        let archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: "plan|different-entry-set".to_owned(),
            title: "Archived collection".to_owned(),
            output_dir: temp.path().join("downloads").join("Archived collection"),
            completed_at_unix: 42,
            entries: vec![DownloadArchiveEntryRecord {
                content_key: download_entry_content_key(entry),
                index: entry.index,
                aid: entry.aid,
                bvid: entry.bvid.clone(),
                cid: entry.cid,
                epid: entry.epid,
                title: entry.title.clone(),
                directory: temp.path().join("downloads").join("Archived collection"),
                files: Vec::new(),
                mux_output: None,
            }],
        }]);

        let preflight =
            DownloadPreflight::inspect(&plan, &DownloadOptions::new(temp.path()), Some(&archive))?;

        assert!(preflight.requires_decision());
        assert_eq!(preflight.archived_records.len(), 1);
        assert_eq!(preflight.archived_records[0].title, "Archived collection");
        Ok(())
    }

    #[test]
    fn download_preflight_reports_entry_overlap_after_index_change() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let plan = test_plan(&server);
        let mut archived_entry = plan.entries[0].clone();
        archived_entry.index += 10;
        let archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: "plan|different-entry-set".to_owned(),
            title: "Archived collection".to_owned(),
            output_dir: temp.path().join("downloads").join("Archived collection"),
            completed_at_unix: 42,
            entries: vec![DownloadArchiveEntryRecord {
                content_key: download_entry_content_key(&archived_entry),
                index: archived_entry.index,
                aid: archived_entry.aid,
                bvid: archived_entry.bvid.clone(),
                cid: archived_entry.cid,
                epid: archived_entry.epid,
                title: archived_entry.title,
                directory: temp.path().join("downloads").join("Archived collection"),
                files: Vec::new(),
                mux_output: None,
            }],
        }]);

        let preflight =
            DownloadPreflight::inspect(&plan, &DownloadOptions::new(temp.path()), Some(&archive))?;

        assert!(preflight.requires_decision());
        assert_eq!(preflight.archived_records.len(), 1);
        assert_eq!(preflight.archived_records[0].entries[0].index, 11);
        Ok(())
    }

    #[test]
    fn download_preflight_matches_legacy_bvid_entry_key() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let plan = test_plan(&server);
        let mut archived_entry = plan.entries[0].clone();
        archived_entry.bvid = None;
        archived_entry.epid = Some(664_928);
        let archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: "plan|legacy-entry-set".to_owned(),
            title: "Archived collection".to_owned(),
            output_dir: temp.path().join("downloads").join("Archived collection"),
            completed_at_unix: 42,
            entries: vec![DownloadArchiveEntryRecord {
                content_key: format!(
                    "aid={};bvid={};cid={};epid={}",
                    archived_entry.aid,
                    archived_entry.bvid.as_deref().unwrap_or_default(),
                    archived_entry.cid,
                    archived_entry.epid.unwrap_or_default()
                ),
                index: archived_entry.index,
                aid: archived_entry.aid,
                bvid: archived_entry.bvid.clone(),
                cid: archived_entry.cid,
                epid: archived_entry.epid,
                title: archived_entry.title,
                directory: temp.path().join("downloads").join("Archived collection"),
                files: Vec::new(),
                mux_output: None,
            }],
        }]);

        let preflight =
            DownloadPreflight::inspect(&plan, &DownloadOptions::new(temp.path()), Some(&archive))?;

        assert!(preflight.requires_decision());
        assert_eq!(preflight.archived_records.len(), 1);
        Ok(())
    }

    #[test]
    fn download_preflight_reports_same_output_archive_record() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let plan = test_plan(&server);
        let options = DownloadOptions::new(temp.path().join("downloads"));
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        let archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: "plan|different-content".to_owned(),
            title: "Different content".to_owned(),
            output_dir: planned_output_dir.join("."),
            completed_at_unix: 42,
            entries: Vec::new(),
        }]);

        let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;

        assert!(preflight.requires_decision());
        assert!(preflight.output_conflict.is_none());
        assert_eq!(preflight.archived_records.len(), 1);
        assert_eq!(preflight.archived_records[0].title, "Different content");
        Ok(())
    }

    #[test]
    fn download_preflight_round_trip_preserves_reserved_output_dirs() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let output_base = temp.path().join("downloads");
        let plan = test_plan(&server);
        let options = DownloadOptions::new(output_base.clone());
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        std::fs::create_dir_all(&planned_output_dir)?;
        let reserved_output_dir = output_base.join("Mock video (2)");
        let archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: "plan|unrelated".to_owned(),
            title: "Unrelated archived content".to_owned(),
            output_dir: reserved_output_dir,
            completed_at_unix: 42,
            entries: Vec::new(),
        }]);
        let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;
        let raw = serde_json::to_string(&preflight)?;

        let round_tripped: DownloadPreflight = serde_json::from_str(&raw)?;

        assert!(raw.contains("reserved_output_dirs"));
        assert_eq!(
            round_tripped.output_dir_for_decision(DuplicateDecision::KeepBoth)?,
            output_base.join("Mock video (3)")
        );
        Ok(())
    }

    #[tokio::test]
    async fn archive_preflight_keep_both_rejects_stale_archive_reservations() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let output_base = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let options = DownloadOptions::new(output_base.clone())
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_mux(MuxOptions::Disabled);
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        std::fs::create_dir_all(&planned_output_dir)?;
        let stale_preflight =
            DownloadPreflight::inspect(&plan, &options, Some(&DownloadArchive::default()))?;
        let mut archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: "plan|unrelated".to_owned(),
            title: "Unrelated archived content".to_owned(),
            output_dir: output_base.join("Mock video (2)"),
            completed_at_unix: 42,
            entries: Vec::new(),
        }]);

        let error = match client
            .download_plan_with_archive_preflight_decision(
                &plan,
                options,
                &mut archive,
                &stale_preflight,
                DuplicateDecision::KeepBoth,
            )
            .await
        {
            Ok(report) => anyhow::bail!(
                "stale preflight unexpectedly downloaded to {}",
                report.output_dir.display()
            ),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            crate::Error::InvalidInput(message)
                if message.contains("current download archive")
        ));
        assert!(!output_base.join("Mock video (2)").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn download_preflight_matches_symlink_parent_archive_output() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let plan = test_plan(&server);
        let options = DownloadOptions::new(temp.path().join("downloads"));
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        let output_subdir = planned_output_dir.join("subdir");
        std_fs::create_dir_all(&output_subdir)?;
        let external_parent = temp.path().join("external");
        std_fs::create_dir_all(&external_parent)?;
        let link_to_output_subdir = external_parent.join("link");
        std::os::unix::fs::symlink(&output_subdir, &link_to_output_subdir)?;
        let archived_output_dir = link_to_output_subdir.join("..");
        let archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: "plan|different-content".to_owned(),
            title: "Symlink parent content".to_owned(),
            output_dir: archived_output_dir,
            completed_at_unix: 42,
            entries: Vec::new(),
        }]);

        let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;

        assert!(preflight.requires_decision());
        assert_eq!(preflight.archived_records.len(), 1);
        assert_eq!(
            preflight.archived_records[0].title,
            "Symlink parent content"
        );
        Ok(())
    }

    #[tokio::test]
    async fn archive_decision_keep_both_uses_new_output_root() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        server.mock(|when, then| {
            when.method(GET).path("/subtitle.ass");
            then.status(200).body("[Script Info]");
        });
        server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200).body("<i/>");
        });
        let temp = tempfile::tempdir()?;
        let output_base = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let options = DownloadOptions::new(output_base.clone())
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_mux(MuxOptions::Disabled);
        std::fs::create_dir_all(default_plan_output_dir(&plan, &options))?;
        let mut archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: download_plan_content_key(&plan),
            title: plan.title.clone(),
            output_dir: default_plan_output_dir(&plan, &options),
            completed_at_unix: 42,
            entries: Vec::new(),
        }]);

        let report = client
            .download_plan_with_archive_decision(
                &plan,
                options,
                &mut archive,
                DuplicateDecision::KeepBoth,
            )
            .await?;

        assert_eq!(report.output_dir, output_base.join("Mock video (2)"));
        assert!(report.output_dir.exists());
        assert_eq!(archive.records.len(), 2);
        assert_eq!(
            archive.records[1].output_dir,
            comparable_output_path(&report.output_dir)
        );
        assert_eq!(archive.records[1].entries.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn archive_decision_keep_both_avoids_archive_only_output_root() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let output_base = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let options = DownloadOptions::new(output_base.clone())
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_danmaku(false)
            .with_mux(MuxOptions::Disabled);
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        let planned_file_name = planned_output_dir
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("missing planned output file name"))?;
        let archived_output_dir = planned_output_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing planned output parent"))?
            .join(".")
            .join(planned_file_name);
        let mut archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: download_plan_content_key(&plan),
            title: plan.title.clone(),
            output_dir: archived_output_dir.clone(),
            completed_at_unix: 42,
            entries: Vec::new(),
        }]);

        let report = client
            .download_plan_with_archive_decision(
                &plan,
                options,
                &mut archive,
                DuplicateDecision::KeepBoth,
            )
            .await?;

        assert!(!planned_output_dir.exists());
        assert_eq!(report.output_dir, output_base.join("Mock video (2)"));
        assert_eq!(archive.records.len(), 2);
        assert_eq!(archive.records[0].output_dir, archived_output_dir);
        assert_eq!(
            archive.records[1].output_dir,
            comparable_output_path(&report.output_dir)
        );
        Ok(())
    }

    #[tokio::test]
    async fn archive_decision_keep_both_avoids_unrelated_archive_output_root() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let output_base = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let options = DownloadOptions::new(output_base.clone())
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_danmaku(false)
            .with_mux(MuxOptions::Disabled);
        std::fs::create_dir_all(default_plan_output_dir(&plan, &options))?;
        let unrelated_output_dir = output_base.join("Mock video (2)");
        let mut archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: "plan|unrelated".to_owned(),
            title: "Unrelated archived content".to_owned(),
            output_dir: unrelated_output_dir.clone(),
            completed_at_unix: 42,
            entries: Vec::new(),
        }]);

        let report = client
            .download_plan_with_archive_decision(
                &plan,
                options,
                &mut archive,
                DuplicateDecision::KeepBoth,
            )
            .await?;

        assert_eq!(report.output_dir, output_base.join("Mock video (3)"));
        assert_eq!(archive.records.len(), 2);
        assert_eq!(archive.records[0].output_dir, unrelated_output_dir);
        assert_eq!(
            archive.records[1].output_dir,
            comparable_output_path(&report.output_dir)
        );
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn archive_decision_keep_both_skips_broken_symlink_output_root() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let output_base = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let options = DownloadOptions::new(output_base.clone())
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_danmaku(false)
            .with_mux(MuxOptions::Disabled);
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        let output_parent = planned_output_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing planned output parent"))?;
        std::fs::create_dir_all(output_parent)?;
        std::os::unix::fs::symlink(temp.path().join("missing-target"), &planned_output_dir)?;
        let mut archive = DownloadArchive::default();

        let report = client
            .download_plan_with_archive_decision(
                &plan,
                options,
                &mut archive,
                DuplicateDecision::KeepBoth,
            )
            .await?;

        assert!(!planned_output_dir.exists());
        assert_eq!(report.output_dir, output_base.join("Mock video (2)"));
        assert_eq!(
            archive.records[0].output_dir,
            comparable_output_path(&report.output_dir)
        );
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn archive_decision_replace_removes_broken_symlink_output_root() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let output_base = temp.path().join("downloads");
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let options = DownloadOptions::new(output_base.clone())
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_danmaku(false)
            .with_mux(MuxOptions::Disabled);
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        let output_parent = planned_output_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing planned output parent"))?;
        std::fs::create_dir_all(output_parent)?;
        std::os::unix::fs::symlink(temp.path().join("missing-target"), &planned_output_dir)?;
        let mut archive = DownloadArchive::default();

        let report = client
            .download_plan_with_archive_decision(
                &plan,
                options,
                &mut archive,
                DuplicateDecision::Replace,
            )
            .await?;

        assert_eq!(report.output_dir, planned_output_dir);
        assert!(tokio::fs::metadata(&planned_output_dir).await?.is_dir());
        assert_eq!(
            archive.records[0].output_dir,
            comparable_output_path(&report.output_dir)
        );
        Ok(())
    }

    #[tokio::test]
    async fn archive_decision_replace_forces_fresh_writes() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let options = DownloadOptions::new(temp.path().join("downloads"))
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_danmaku(false)
            .with_mux(MuxOptions::Disabled);
        let entry_dir = test_entry_dir(&options.output_dir, &plan);
        std::fs::create_dir_all(&entry_dir)?;
        let video_path =
            entry_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        std::fs::write(&video_path, "partial")?;
        let stale_sidecar = entry_dir.join("danmaku.xml");
        std::fs::write(&stale_sidecar, "<old/>")?;
        let planned_output_dir = default_plan_output_dir(&plan, &options);
        let planned_file_name = planned_output_dir
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("missing planned output file name"))?;
        let equivalent_output_dir = planned_output_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing planned output parent"))?
            .join(".")
            .join(planned_file_name);
        let stale_content_key = "plan|old-content".to_owned();
        let mut archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: stale_content_key.clone(),
            title: "Old content".to_owned(),
            output_dir: equivalent_output_dir,
            completed_at_unix: 41,
            entries: Vec::new(),
        }]);

        let report = client
            .download_plan_with_archive_decision(
                &plan,
                options,
                &mut archive,
                DuplicateDecision::Replace,
            )
            .await?;

        assert_eq!(tokio::fs::read_to_string(video_path).await?, "video");
        assert!(!stale_sidecar.exists());
        assert_eq!(report.entries[0].files[0].resumed_from, 0);
        assert_eq!(archive.records.len(), 1);
        assert_eq!(
            archive.records[0].content_key,
            download_plan_content_key(&plan)
        );
        assert_ne!(archive.records[0].content_key, stale_content_key);
        Ok(())
    }

    #[tokio::test]
    async fn archive_preflight_cancel_rejects_late_output_conflict() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let options = DownloadOptions::new(temp.path().join("downloads"))
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_danmaku(false)
            .with_mux(MuxOptions::Disabled);
        let mut archive = DownloadArchive::default();
        let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;
        let marker = preflight.planned_output_dir.join("marker.txt");
        std::fs::create_dir_all(&preflight.planned_output_dir)?;
        std::fs::write(&marker, "external")?;

        let result = client
            .download_plan_with_archive_preflight_decision(
                &plan,
                options,
                &mut archive,
                &preflight,
                DuplicateDecision::Cancel,
            )
            .await;
        let error = match result {
            Ok(report) => anyhow::bail!(
                "late output conflict must not be deleted by safe continue, got {report:?}"
            ),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("output directory appeared after duplicate preflight")
        );
        assert_eq!(std::fs::read_to_string(marker)?, "external");
        assert!(archive.records.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn archive_preflight_replace_removes_late_output_conflict() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        server.mock(|when, then| {
            when.method(GET).path("/audio.m4s");
            then.status(200).body("audio");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let options = DownloadOptions::new(temp.path().join("downloads"))
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_danmaku(false)
            .with_mux(MuxOptions::Disabled);
        let entry_dir = test_entry_dir(&options.output_dir, &plan);
        let video_path =
            entry_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        let stale_sidecar = entry_dir.join("stale.txt");
        let mut archive = DownloadArchive::default();
        let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;
        std::fs::create_dir_all(&entry_dir)?;
        std::fs::write(&video_path, "partial")?;
        std::fs::write(&stale_sidecar, "stale")?;

        let report = client
            .download_plan_with_archive_preflight_decision(
                &plan,
                options,
                &mut archive,
                &preflight,
                DuplicateDecision::Replace,
            )
            .await?;

        assert_eq!(tokio::fs::read_to_string(video_path).await?, "video");
        assert!(!stale_sidecar.exists());
        assert_eq!(report.entries[0].files[0].resumed_from, 0);
        assert_eq!(archive.records.len(), 1);
        Ok(())
    }

    #[test]
    fn download_archive_record_stores_absolute_paths() -> anyhow::Result<()> {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let report = DownloadReport {
            title: plan.title.clone(),
            output_dir: Path::new("downloads").join("Mock video"),
            entries: vec![EntryDownloadReport {
                index: 1,
                title: "Main".to_owned(),
                directory: Path::new("downloads").join("Mock video").join("entry"),
                files: vec![DownloadedFile {
                    kind: DownloadFileKind::Video,
                    path: Path::new("downloads")
                        .join("Mock video")
                        .join("entry")
                        .join("video.m4s"),
                    bytes_written: 5,
                    resumed_from: 0,
                }],
                mux: Some(super::MuxReport {
                    output_path: Path::new("downloads")
                        .join("Mock video")
                        .join("entry")
                        .join("main.mp4"),
                    command: vec!["ffmpeg".to_owned()],
                }),
            }],
        };
        let mut archive = DownloadArchive::default();

        archive.record_download(&plan, &DownloadOptions::default(), &report);

        let record = &archive.records[0];
        assert!(record.output_dir.is_absolute());
        assert!(
            record
                .output_dir
                .ends_with(Path::new("downloads").join("Mock video"))
        );
        assert!(record.entries[0].directory.is_absolute());
        assert!(
            record.entries[0]
                .directory
                .ends_with(Path::new("downloads").join("Mock video").join("entry"))
        );
        assert!(record.entries[0].files[0].is_absolute());
        assert!(
            record.entries[0].files[0].ends_with(
                Path::new("downloads")
                    .join("Mock video")
                    .join("entry")
                    .join("video.m4s")
            )
        );
        let mux_output = record.entries[0]
            .mux_output
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing mux output"))?;
        assert!(mux_output.is_absolute());
        assert!(
            mux_output.ends_with(
                Path::new("downloads")
                    .join("Mock video")
                    .join("entry")
                    .join("main.mp4")
            )
        );
        Ok(())
    }

    #[test]
    fn download_archive_records_can_be_queried_by_download_mode() {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let report = DownloadReport {
            title: plan.title.clone(),
            output_dir: Path::new("downloads").join("Mock video"),
            entries: vec![EntryDownloadReport {
                index: 1,
                title: "Main".to_owned(),
                directory: Path::new("downloads").join("Mock video").join("entry"),
                files: vec![DownloadedFile {
                    kind: DownloadFileKind::Cover,
                    path: Path::new("downloads")
                        .join("Mock video")
                        .join("entry")
                        .join("cover.jpg"),
                    bytes_written: 5,
                    resumed_from: 0,
                }],
                mux: None,
            }],
        };
        let options = DownloadOptions::default().with_download_mode(DownloadMode::CoverOnly);
        let mut archive = DownloadArchive::default();

        archive.record_download(&plan, &options, &report);

        assert_eq!(archive.records_for_plan(&plan).len(), 1);
        assert_eq!(
            archive
                .records_for_plan_with_mode(&plan, DownloadMode::CoverOnly)
                .len(),
            1
        );
        assert!(
            archive
                .records_for_plan_with_mode(&plan, DownloadMode::All)
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn download_archive_record_resolves_symlink_parent_output_path() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let plan = test_plan(&server);
        let planned_output_dir = temp.path().join("downloads").join("Mock video");
        let output_subdir = planned_output_dir.join("subdir");
        std_fs::create_dir_all(&output_subdir)?;
        let external_parent = temp.path().join("external");
        std_fs::create_dir_all(&external_parent)?;
        let link_to_output_subdir = external_parent.join("link");
        std::os::unix::fs::symlink(&output_subdir, &link_to_output_subdir)?;
        let report = DownloadReport {
            title: plan.title.clone(),
            output_dir: link_to_output_subdir.join(".."),
            entries: Vec::new(),
        };
        let mut archive = DownloadArchive::default();

        archive.record_download(&plan, &DownloadOptions::default(), &report);

        assert_eq!(
            archive.records[0].output_dir,
            comparable_output_path(&planned_output_dir)
        );
        Ok(())
    }

    #[test]
    fn download_archive_round_trips_without_urls() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let plan = test_plan(&server);
        let report = DownloadReport {
            title: plan.title.clone(),
            output_dir: temp.path().join("downloads").join("Mock video"),
            entries: vec![EntryDownloadReport {
                index: 1,
                title: "Main".to_owned(),
                directory: temp
                    .path()
                    .join("downloads")
                    .join("Mock video")
                    .join("entry"),
                files: vec![DownloadedFile {
                    kind: DownloadFileKind::Video,
                    path: temp
                        .path()
                        .join("downloads")
                        .join("Mock video")
                        .join("entry")
                        .join("video.m4s"),
                    bytes_written: 5,
                    resumed_from: 0,
                }],
                mux: None,
            }],
        };
        let mut archive = DownloadArchive::default();
        archive.record_download(&plan, &DownloadOptions::default(), &report);
        let archive_path = temp.path().join("archive.json");

        archive.save(&archive_path)?;
        let raw = std::fs::read_to_string(&archive_path)?;
        let loaded = DownloadArchive::load(&archive_path)?;

        assert_eq!(loaded, archive);
        assert!(!raw.contains("https://"));
        assert!(!raw.contains("ACCESS"));
        Ok(())
    }

    #[test]
    fn download_archive_save_replaces_existing_file() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let archive_path = temp.path().join("archive.json");
        std::fs::write(&archive_path, "{\"old\":true}")?;

        DownloadArchive::default().save(&archive_path)?;

        let raw = std::fs::read_to_string(&archive_path)?;
        assert!(raw.contains("\"records\""));
        assert!(!raw.contains("\"old\""));
        assert!(!archive_sidecar_path(&archive_path, ".bbdown-archive-backup").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn download_archive_save_preserves_symlink_target() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let shared_dir = temp.path().join("shared");
        std::fs::create_dir_all(&shared_dir)?;
        let archive_target = shared_dir.join("archive.json");
        let archive_link = temp.path().join("archive-link.json");
        std::fs::write(&archive_target, "{\"old\":true}")?;
        std::os::unix::fs::symlink(&archive_target, &archive_link)?;

        DownloadArchive::default().save(&archive_link)?;

        let raw = std::fs::read_to_string(&archive_target)?;
        assert!(
            std::fs::symlink_metadata(&archive_link)?
                .file_type()
                .is_symlink()
        );
        assert!(raw.contains("\"records\""));
        assert!(!raw.contains("\"old\""));
        assert!(!archive_sidecar_path(&archive_target, ".bbdown-archive-backup").exists());
        assert!(!archive_sidecar_path(&archive_link, ".bbdown-archive-backup").exists());
        Ok(())
    }

    #[test]
    fn download_archive_save_rejects_directory_path() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let archive_path = temp.path().join("archive.json");
        std::fs::create_dir_all(&archive_path)?;

        let error = match DownloadArchive::default().save(&archive_path) {
            Ok(()) => anyhow::bail!("directory archive path unexpectedly saved"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            crate::Error::InvalidInput(message) if message.contains("directory")
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn download_archive_load_reports_metadata_errors() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let restricted = temp.path().join("restricted");
        std_fs::create_dir(&restricted)?;
        let original_permissions = std_fs::metadata(&restricted)?.permissions();
        let mut denied_permissions = original_permissions.clone();
        denied_permissions.set_mode(0o000);
        std_fs::set_permissions(&restricted, denied_permissions)?;

        let result = DownloadArchive::load(restricted.join("archive.json"));

        std_fs::set_permissions(&restricted, original_permissions)?;
        match result {
            Err(crate::Error::Io(error))
                if error.kind() == std::io::ErrorKind::PermissionDenied => {}
            Ok(archive) => {
                anyhow::bail!(
                    "expected archive load permission error, got {} records",
                    archive.records.len()
                );
            }
            Err(error) => {
                anyhow::bail!("expected archive load permission error, got {error}");
            }
        }
        Ok(())
    }

    fn media_stream(id: u32, base_url: &str) -> MediaStream {
        MediaStream {
            id,
            base_url: base_url.to_owned(),
            backup_urls: Vec::new(),
            codecs: None,
            bandwidth: None,
            width: None,
            height: None,
            frame_rate: None,
            mime_type: Some("video/mp4".to_owned()),
            size: None,
        }
    }

    fn test_plan(server: &MockServer) -> DownloadPlan {
        DownloadPlan {
            title: "Mock video".to_owned(),
            entries: vec![DownloadEntry {
                index: 1,
                aid: 170_001,
                bvid: Some("BV1xx411c7mD".to_owned()),
                cid: 2,
                epid: None,
                title: "Main".to_owned(),
                cover_url: Some(format!("{}/cover.jpg", server.base_url())),
                source: StreamSource::NormalWeb,
                streams: StreamSet {
                    videos: vec![MediaStream {
                        id: 80,
                        base_url: format!("{}/video.m4s", server.base_url()),
                        backup_urls: Vec::new(),
                        codecs: None,
                        bandwidth: None,
                        width: None,
                        height: None,
                        frame_rate: None,
                        mime_type: Some("video/mp4".to_owned()),
                        size: None,
                    }],
                    audios: vec![MediaStream {
                        id: 30280,
                        base_url: format!("{}/audio.m4s", server.base_url()),
                        backup_urls: Vec::new(),
                        codecs: None,
                        bandwidth: None,
                        width: None,
                        height: None,
                        frame_rate: None,
                        mime_type: Some("audio/mp4".to_owned()),
                        size: None,
                    }],
                    flv_segments: Vec::new(),
                    accept_quality: vec![80],
                    qualities: vec![StreamQuality {
                        id: 80,
                        description: Some("1080P".to_owned()),
                    }],
                    duration_seconds: Some(3),
                },
                diagnostics: StreamDiagnostics::default(),
                subtitles: vec![SubtitleTrack {
                    language: "en".to_owned(),
                    language_doc: Some("English".to_owned()),
                    url: format!("{}/subtitle.ass", server.base_url()),
                    format: SubtitleFormat::Ass,
                }],
                danmaku: DanmakuTrack {
                    cid: 2,
                    xml_url: format!("{}/danmaku.xml", server.base_url()),
                },
            }],
        }
    }

    fn single_video_plan(url: String) -> DownloadPlan {
        let mut plan = test_plan(&MockServer::start());
        plan.entries[0].streams.videos[0].base_url = url;
        plan.entries[0].streams.audios[0].base_url =
            companion_audio_url(&plan.entries[0].streams.videos[0].base_url);
        plan.entries[0].subtitles.clear();
        plan
    }

    fn companion_audio_url(video_url: &str) -> String {
        let Ok(mut url) = url::Url::parse(video_url) else {
            return video_url.to_owned();
        };
        url.set_path("/audio.m4s");
        url.set_query(None);
        url.set_fragment(None);
        url.to_string()
    }

    fn test_entry_dir(base: &Path, plan: &DownloadPlan) -> std::path::PathBuf {
        base.join(safe_file_name(&plan.title))
            .join(entry_dir_name(&plan.entries[0]))
    }

    #[cfg(unix)]
    fn write_fake_ffmpeg(dir: &Path, body: &str) -> anyhow::Result<std::path::PathBuf> {
        let path = dir.join("fake-ffmpeg");
        std_fs::write(&path, format!("#!/bin/sh\n{body}\n"))?;
        let mut permissions = std_fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        std_fs::set_permissions(&path, permissions)?;
        Ok(path)
    }

    #[cfg(unix)]
    fn fake_ffmpeg_creates_output_body() -> &'static str {
        "last=\nfor arg do last=$arg; done\nprintf 'muxed' > \"$last\"\nexit 0"
    }
}

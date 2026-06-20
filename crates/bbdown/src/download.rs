use crate::{
    BiliClient, ChapterTrack, DownloadEntry, DownloadPlan, Error, FlvSegment, Input, MediaStream,
    Result, Selection, SubtitleFormat, SubtitleTrack,
    cancellation::DownloadCancellationToken,
    danmaku::{self, DanmakuFormat, DanmakuFormats},
    progress::{DownloadProgressEvent, DownloadProgressSink, NoopDownloadProgress},
};
use futures_util::StreamExt;
use md5::{Digest, Md5};
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::future::Future;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const MAX_FILE_NAME_BYTES: usize = 80;
const MAX_FILE_COMPONENT_BYTES: usize = 240;
const MAX_SUBTITLE_EXTENSION_BYTES: usize = 16;
const MAX_COVER_EXTENSION_BYTES: usize = 16;
#[cfg(any(unix, windows))]
const MUX_SIGNAL_CANCELLATION_GRACE: Duration = Duration::from_millis(100);
const DEFAULT_UPOS_REPLACEMENT_HOST: &str = "upos-sz-mirrorcoso1.bilivideo.com";
const DEFAULT_OUTPUT_DIR_TEMPLATE: &str = "{title}";
const DEFAULT_ENTRY_DIR_TEMPLATE: &str = "P{index:03}-{content_id}-{entry_title}";
const DEFAULT_MUX_FILE_STEM_TEMPLATE: &str = "{entry_title}";

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
    pub path_templates: DownloadPathTemplates,
    pub resume: bool,
    pub mode: DownloadMode,
    pub include_subtitles: bool,
    pub subtitle_ai_policy: SubtitleAiPolicy,
    pub include_danmaku: bool,
    pub danmaku_formats: DanmakuFormats,
    pub sidecars: SidecarOptions,
    pub media_hosts: MediaHostOptions,
    pub mux: MuxOptions,
    pub download_idle_timeout: Option<Duration>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("."),
            retry: RetryPolicy::default(),
            stream_selection: StreamSelection::default(),
            path_templates: DownloadPathTemplates::default(),
            resume: true,
            mode: DownloadMode::All,
            include_subtitles: true,
            subtitle_ai_policy: SubtitleAiPolicy::default(),
            include_danmaku: true,
            danmaku_formats: DanmakuFormats::default(),
            sidecars: SidecarOptions::default(),
            media_hosts: MediaHostOptions::default(),
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
    pub fn with_path_templates(mut self, path_templates: DownloadPathTemplates) -> Self {
        self.path_templates = path_templates;
        self
    }

    #[must_use]
    pub fn with_output_template(mut self, output_template: impl Into<String>) -> Self {
        self.path_templates.output_dir = output_template.into();
        self
    }

    #[must_use]
    pub fn with_entry_template(mut self, entry_template: impl Into<String>) -> Self {
        self.path_templates.entry_dir = entry_template.into();
        self
    }

    #[must_use]
    pub fn with_mux_template(mut self, mux_template: impl Into<String>) -> Self {
        self.path_templates.mux_file_stem = mux_template.into();
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
    pub fn with_subtitle_ai_policy(mut self, subtitle_ai_policy: SubtitleAiPolicy) -> Self {
        self.subtitle_ai_policy = subtitle_ai_policy;
        self
    }

    #[must_use]
    pub fn with_danmaku(mut self, include_danmaku: bool) -> Self {
        self.include_danmaku = include_danmaku;
        self.sidecars.danmaku = include_danmaku;
        self
    }

    #[must_use]
    pub fn with_danmaku_format(mut self, danmaku_format: DanmakuFormat) -> Self {
        self.danmaku_formats = DanmakuFormats::new([danmaku_format]);
        self
    }

    #[must_use]
    pub fn with_danmaku_formats(
        mut self,
        danmaku_formats: impl IntoIterator<Item = DanmakuFormat>,
    ) -> Self {
        self.danmaku_formats = DanmakuFormats::new(danmaku_formats);
        self
    }

    #[must_use]
    pub fn with_media_hosts(mut self, media_hosts: MediaHostOptions) -> Self {
        self.media_hosts = media_hosts;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SubtitleAiPolicy {
    #[default]
    Include,
    PreferNonAi,
    ExcludeAi,
    OnlyAi,
}

impl SubtitleAiPolicy {
    const fn archive_key_token(self) -> Option<&'static str> {
        match self {
            Self::Include => None,
            Self::PreferNonAi => Some("prefer-non-ai"),
            Self::ExcludeAi => Some("exclude-ai"),
            Self::OnlyAi => Some("only-ai"),
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct DanmakuUpdateOptions {
    pub retry: RetryPolicy,
    pub danmaku_formats: DanmakuFormats,
    pub download_idle_timeout: Option<Duration>,
}

impl Default for DanmakuUpdateOptions {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            danmaku_formats: DanmakuFormats::default(),
            download_idle_timeout: Some(Duration::from_secs(30)),
        }
    }
}

impl DanmakuUpdateOptions {
    #[must_use]
    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    #[must_use]
    pub fn with_danmaku_formats(
        mut self,
        danmaku_formats: impl IntoIterator<Item = DanmakuFormat>,
    ) -> Self {
        self.danmaku_formats = DanmakuFormats::new(danmaku_formats);
        self
    }

    #[must_use]
    pub fn with_download_idle_timeout(mut self, download_idle_timeout: Option<Duration>) -> Self {
        self.download_idle_timeout = download_idle_timeout;
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadPathTemplates {
    pub output_dir: String,
    pub entry_dir: String,
    pub mux_file_stem: String,
}

impl Default for DownloadPathTemplates {
    fn default() -> Self {
        Self {
            output_dir: DEFAULT_OUTPUT_DIR_TEMPLATE.to_owned(),
            entry_dir: DEFAULT_ENTRY_DIR_TEMPLATE.to_owned(),
            mux_file_stem: DEFAULT_MUX_FILE_STEM_TEMPLATE.to_owned(),
        }
    }
}

impl DownloadPathTemplates {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_output_dir(mut self, output_dir: impl Into<String>) -> Self {
        self.output_dir = output_dir.into();
        self
    }

    #[must_use]
    pub fn with_entry_dir(mut self, entry_dir: impl Into<String>) -> Self {
        self.entry_dir = entry_dir.into();
        self
    }

    #[must_use]
    pub fn with_mux_file_stem(mut self, mux_file_stem: impl Into<String>) -> Self {
        self.mux_file_stem = mux_file_stem.into();
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaHostOptions {
    pub upos_host: Option<String>,
    pub force_replace_host: bool,
    pub allow_pcdn: bool,
}

impl Default for MediaHostOptions {
    fn default() -> Self {
        Self {
            upos_host: None,
            force_replace_host: false,
            allow_pcdn: true,
        }
    }
}

impl MediaHostOptions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn bbdown_cli_default() -> Self {
        Self {
            allow_pcdn: false,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_upos_host(mut self, upos_host: impl Into<String>) -> Self {
        let upos_host = upos_host.into();
        self.upos_host = (!upos_host.trim().is_empty()).then_some(upos_host);
        self
    }

    #[must_use]
    pub fn with_force_replace_host(mut self, force_replace_host: bool) -> Self {
        self.force_replace_host = force_replace_host;
        self
    }

    #[must_use]
    pub fn with_allow_pcdn(mut self, allow_pcdn: bool) -> Self {
        self.allow_pcdn = allow_pcdn;
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StreamSelection {
    pub video_quality: Option<u32>,
    pub audio_quality: Option<u32>,
    pub audio_language: Option<String>,
}

impl StreamSelection {
    #[must_use]
    pub const fn new(video_quality: Option<u32>, audio_quality: Option<u32>) -> Self {
        Self {
            video_quality,
            audio_quality,
            audio_language: None,
        }
    }

    #[must_use]
    pub const fn video(video_quality: u32) -> Self {
        Self {
            video_quality: Some(video_quality),
            audio_quality: None,
            audio_language: None,
        }
    }

    #[must_use]
    pub const fn audio(audio_quality: u32) -> Self {
        Self {
            video_quality: None,
            audio_quality: Some(audio_quality),
            audio_language: None,
        }
    }

    #[must_use]
    pub fn audio_language(audio_language: impl Into<String>) -> Self {
        Self::default().with_audio_language(audio_language)
    }

    #[must_use]
    pub fn with_audio_language(mut self, audio_language: impl Into<String>) -> Self {
        let audio_language = audio_language.into();
        self.audio_language = non_empty_trimmed_string(&audio_language);
        self
    }

    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.video_quality.is_some()
            || self.audio_quality.is_some()
            || self.audio_language.is_some()
    }

    #[must_use]
    pub fn has_audio_selection(&self) -> bool {
        self.audio_quality.is_some() || self.audio_language.is_some()
    }
}

fn non_empty_trimmed_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
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
            if comparable_output_path_key(&existing.output_dir) != record_output_key {
                return true;
            }
            existing.content_key != record.content_key
                && archive_record_outputs_are_present(existing)
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
            content_key: download_plan_content_key_for_options(plan, options),
            title: report.title.clone(),
            output_dir: archive_record_path(&report.output_dir),
            completed_at_unix,
            entries: plan
                .entries
                .iter()
                .zip(&report.entries)
                .map(|(plan_entry, report_entry)| DownloadArchiveEntryRecord {
                    content_key: download_entry_content_key_for_options(plan_entry, options),
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
        validate_download_path_templates(plan, options)?;
        let planned_output_dir = default_plan_output_dir(plan, options)?;
        let output_conflict =
            path_is_occupied(&planned_output_dir)?.then_some(DownloadOutputConflict {
                path: planned_output_dir.clone(),
            });
        let archived_records = archive.map_or_else(Vec::new, |archive| {
            archive_records_for_preflight(archive, plan, options, &planned_output_dir)
        });
        let reserved_output_dirs = archive.map_or_else(Vec::new, |archive| {
            archive
                .records
                .iter()
                .map(|record| record.output_dir.clone())
                .collect()
        });
        Ok(Self {
            content_key: download_plan_content_key_for_options(plan, options),
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadReportSummary {
    pub entry_count: usize,
    pub file_count: usize,
    pub media_file_count: usize,
    pub sidecar_file_count: usize,
    pub mux_count: usize,
    pub bytes_written: u64,
    pub resumed_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EntryDownloadReport {
    pub index: u32,
    pub title: String,
    pub directory: PathBuf,
    pub files: Vec<DownloadedFile>,
    pub mux: Option<MuxReport>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EntryDownloadSummary {
    pub file_count: usize,
    pub media_file_count: usize,
    pub sidecar_file_count: usize,
    pub has_mux: bool,
    pub bytes_written: u64,
    pub resumed_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadedFile {
    pub kind: DownloadFileKind,
    pub path: PathBuf,
    pub bytes_written: u64,
    pub resumed_from: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DanmakuUpdateReport {
    pub entries: Vec<EntryDanmakuUpdateReport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EntryDanmakuUpdateReport {
    pub index: u32,
    pub aid: u64,
    pub bvid: Option<String>,
    pub cid: u64,
    pub epid: Option<u64>,
    pub title: String,
    pub directory: PathBuf,
    pub existing_comments: usize,
    pub fetched_comments: usize,
    pub appended_comments: usize,
    pub files: Vec<DownloadedFile>,
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
    DanmakuAss,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MuxReport {
    pub output_path: PathBuf,
    pub command: Vec<String>,
    #[serde(default)]
    pub chapter_count: usize,
}

impl DownloadReport {
    #[must_use]
    pub fn summary(&self) -> DownloadReportSummary {
        self.entries.iter().map(EntryDownloadReport::summary).fold(
            DownloadReportSummary {
                entry_count: self.entries.len(),
                ..DownloadReportSummary::default()
            },
            |mut summary, entry| {
                summary.file_count += entry.file_count;
                summary.media_file_count += entry.media_file_count;
                summary.sidecar_file_count += entry.sidecar_file_count;
                summary.mux_count += usize::from(entry.has_mux);
                summary.bytes_written = summary.bytes_written.saturating_add(entry.bytes_written);
                summary.resumed_bytes = summary.resumed_bytes.saturating_add(entry.resumed_bytes);
                summary.total_bytes = summary.total_bytes.saturating_add(entry.total_bytes);
                summary
            },
        )
    }
}

impl EntryDownloadReport {
    #[must_use]
    pub fn summary(&self) -> EntryDownloadSummary {
        self.files.iter().fold(
            EntryDownloadSummary {
                file_count: self.files.len(),
                has_mux: self.mux.is_some(),
                ..EntryDownloadSummary::default()
            },
            |mut summary, file| {
                if file.kind.is_media() {
                    summary.media_file_count += 1;
                } else {
                    summary.sidecar_file_count += 1;
                }
                summary.bytes_written = summary.bytes_written.saturating_add(file.bytes_written);
                summary.resumed_bytes = summary.resumed_bytes.saturating_add(file.resumed_from);
                summary.total_bytes = summary
                    .total_bytes
                    .saturating_add(file.resumed_from.saturating_add(file.bytes_written));
                summary
            },
        )
    }
}

impl BiliClient {
    pub async fn download_input(
        &self,
        raw: &str,
        selection: Option<Selection>,
        options: DownloadOptions,
    ) -> Result<DownloadReport> {
        let progress = NoopDownloadProgress;
        self.download_input_with_progress(raw, selection, options, &progress)
            .await
    }

    pub async fn download_input_with_cancellation(
        &self,
        raw: &str,
        selection: Option<Selection>,
        options: DownloadOptions,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport> {
        let progress = NoopDownloadProgress;
        self.download_input_with_progress_and_cancellation(
            raw,
            selection,
            options,
            &progress,
            cancellation,
        )
        .await
    }

    pub async fn download_input_with_progress<P>(
        &self,
        raw: &str,
        selection: Option<Selection>,
        options: DownloadOptions,
        progress: &P,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let cancellation = DownloadCancellationToken::new();
        self.download_input_with_progress_and_cancellation(
            raw,
            selection,
            options,
            progress,
            &cancellation,
        )
        .await
    }

    pub async fn download_input_with_progress_and_cancellation<P>(
        &self,
        raw: &str,
        selection: Option<Selection>,
        options: DownloadOptions,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let input = match Input::parse(raw) {
            Ok(input) => input,
            Err(error) => {
                emit_plan_failed_title(progress, raw.to_owned(), &options.output_dir, 0, &error);
                return Err(error);
            }
        };
        if let Err(error) = cancellation.check() {
            emit_plan_cancelled_title(progress, raw.to_owned(), &options.output_dir, 0, &error);
            return Err(error);
        }
        self.download_with_progress_and_cancellation(
            input,
            selection,
            options,
            progress,
            cancellation,
        )
        .await
    }

    pub async fn download(
        &self,
        input: Input,
        selection: Option<Selection>,
        options: DownloadOptions,
    ) -> Result<DownloadReport> {
        let progress = NoopDownloadProgress;
        self.download_with_progress(input, selection, options, &progress)
            .await
    }

    pub async fn download_with_cancellation(
        &self,
        input: Input,
        selection: Option<Selection>,
        options: DownloadOptions,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport> {
        let progress = NoopDownloadProgress;
        self.download_with_progress_and_cancellation(
            input,
            selection,
            options,
            &progress,
            cancellation,
        )
        .await
    }

    pub async fn download_with_progress<P>(
        &self,
        input: Input,
        selection: Option<Selection>,
        options: DownloadOptions,
        progress: &P,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let cancellation = DownloadCancellationToken::new();
        self.download_with_progress_and_cancellation(
            input,
            selection,
            options,
            progress,
            &cancellation,
        )
        .await
    }

    pub async fn download_with_progress_and_cancellation<P>(
        &self,
        input: Input,
        selection: Option<Selection>,
        options: DownloadOptions,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let title = input_progress_title(&input);
        if let Err(error) = cancellation.check() {
            emit_plan_cancelled_title(progress, title, &options.output_dir, 0, &error);
            return Err(error);
        }
        let plan = match await_or_cancel(
            self.plan_for_download(input, selection, options.mode),
            cancellation,
        )
        .await
        {
            Ok(plan) => plan,
            Err(error) => {
                if error.is_cancelled() {
                    emit_plan_cancelled_title(progress, title, &options.output_dir, 0, &error);
                } else {
                    emit_plan_failed_title(progress, title, &options.output_dir, 0, &error);
                }
                return Err(error);
            }
        };
        self.download_plan_with_progress_and_cancellation(&plan, options, progress, cancellation)
            .await
    }

    pub async fn download_plan(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
    ) -> Result<DownloadReport> {
        let progress = NoopDownloadProgress;
        self.download_plan_with_progress(plan, options, &progress)
            .await
    }

    pub async fn download_plan_with_cancellation(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport> {
        let progress = NoopDownloadProgress;
        self.download_plan_with_progress_and_cancellation(plan, options, &progress, cancellation)
            .await
    }

    pub async fn download_plan_with_progress<P>(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        progress: &P,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let cancellation = DownloadCancellationToken::new();
        self.download_plan_with_progress_and_cancellation(plan, options, progress, &cancellation)
            .await
    }

    pub async fn download_plan_with_progress_and_cancellation<P>(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        if let Err(error) = validate_download_plan_options(plan, &options) {
            return plan_failed(progress, plan, &options.output_dir, 0, error);
        }
        let output_dir = match default_plan_output_dir(plan, &options) {
            Ok(output_dir) => output_dir,
            Err(error) => return plan_failed(progress, plan, &options.output_dir, 0, error),
        };
        self.download_plan_to_output_dir(plan, options, output_dir, progress, cancellation)
            .await
    }

    pub async fn download_plan_with_archive_decision(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        archive: &mut DownloadArchive,
        decision: DuplicateDecision,
    ) -> Result<DownloadReport> {
        let progress = NoopDownloadProgress;
        self.download_plan_with_archive_decision_with_progress(
            plan, options, archive, decision, &progress,
        )
        .await
    }

    pub async fn download_plan_with_archive_decision_with_cancellation(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        archive: &mut DownloadArchive,
        decision: DuplicateDecision,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport> {
        let progress = NoopDownloadProgress;
        self.download_plan_with_archive_decision_with_progress_and_cancellation(
            plan,
            options,
            archive,
            decision,
            &progress,
            cancellation,
        )
        .await
    }

    pub async fn download_plan_with_archive_decision_with_progress<P>(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        archive: &mut DownloadArchive,
        decision: DuplicateDecision,
        progress: &P,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let cancellation = DownloadCancellationToken::new();
        self.download_plan_with_archive_decision_with_progress_and_cancellation(
            plan,
            options,
            archive,
            decision,
            progress,
            &cancellation,
        )
        .await
    }

    pub async fn download_plan_with_archive_decision_with_progress_and_cancellation<P>(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        archive: &mut DownloadArchive,
        decision: DuplicateDecision,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let preflight = match DownloadPreflight::inspect(plan, &options, Some(archive)) {
            Ok(preflight) => preflight,
            Err(error) => return plan_failed(progress, plan, &options.output_dir, 0, error),
        };
        self.download_plan_with_archive_preflight_decision_with_progress_and_cancellation(
            plan,
            options,
            archive,
            &preflight,
            decision,
            progress,
            cancellation,
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
        let progress = NoopDownloadProgress;
        self.download_plan_with_archive_preflight_decision_with_progress(
            plan, options, archive, preflight, decision, &progress,
        )
        .await
    }

    pub async fn download_plan_with_archive_preflight_decision_with_cancellation(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        archive: &mut DownloadArchive,
        preflight: &DownloadPreflight,
        decision: DuplicateDecision,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport> {
        let progress = NoopDownloadProgress;
        self.download_plan_with_archive_preflight_decision_with_progress_and_cancellation(
            plan,
            options,
            archive,
            preflight,
            decision,
            &progress,
            cancellation,
        )
        .await
    }

    pub async fn download_plan_with_archive_preflight_decision_with_progress<P>(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        archive: &mut DownloadArchive,
        preflight: &DownloadPreflight,
        decision: DuplicateDecision,
        progress: &P,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let cancellation = DownloadCancellationToken::new();
        self.download_plan_with_archive_preflight_decision_with_progress_and_cancellation(
            plan,
            options,
            archive,
            preflight,
            decision,
            progress,
            &cancellation,
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "archive-aware progress plus cancellation API mirrors the explicit existing archive execution API"
    )]
    pub async fn download_plan_with_archive_preflight_decision_with_progress_and_cancellation<P>(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        archive: &mut DownloadArchive,
        preflight: &DownloadPreflight,
        decision: DuplicateDecision,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        if let Err(error) = validate_download_path_templates(plan, &options) {
            return plan_failed(progress, plan, &preflight.planned_output_dir, 0, error);
        }
        if let Err(error) = validate_archive_preflight(plan, &options, archive, preflight) {
            return plan_failed(progress, plan, &preflight.planned_output_dir, 0, error);
        }
        if let Err(error) = cancellation.check() {
            emit_plan_cancelled(progress, plan, &preflight.planned_output_dir, 0, &error);
            return Err(error);
        }
        let mut effective_options = options;
        let output_dir = archive_output_dir_for_decision(
            plan,
            &mut effective_options,
            preflight,
            decision,
            progress,
            cancellation,
        )
        .await?;
        let report = self
            .download_plan_to_output_dir(
                plan,
                effective_options.clone(),
                output_dir,
                progress,
                cancellation,
            )
            .await?;
        archive.record_download(plan, &effective_options, &report);
        Ok(report)
    }

    pub async fn update_danmaku_for_archive(
        &self,
        plan: &DownloadPlan,
        archive: &mut DownloadArchive,
        options: DanmakuUpdateOptions,
    ) -> Result<DanmakuUpdateReport> {
        let archive_danmaku_formats = danmaku_update_archive_formats(&options.danmaku_formats);
        let mut entries = Vec::new();
        for record in &mut archive.records {
            let record_content_key = record.content_key.clone();
            let mut record_updated = false;
            for archive_entry in &mut record.entries {
                if !archive_entry_allows_danmaku_update(&record_content_key, archive_entry) {
                    continue;
                }
                let Some(plan_entry) = plan
                    .entries
                    .iter()
                    .find(|plan_entry| archive_entry_matches_plan_entry(archive_entry, plan_entry))
                else {
                    continue;
                };
                let report = self
                    .update_danmaku_archive_entry(plan_entry, archive_entry, &options)
                    .await?;
                refresh_archive_entry_danmaku_content_key(archive_entry, &archive_danmaku_formats);
                record_updated = true;
                entries.push(report);
            }
            if record_updated {
                refresh_archive_record_danmaku_content_key(record, &archive_danmaku_formats);
                record.completed_at_unix = current_unix_seconds();
            }
        }
        if entries.is_empty() {
            return Err(Error::InvalidInput(
                "download archive does not contain entries selected by the current plan".to_owned(),
            ));
        }
        Ok(DanmakuUpdateReport { entries })
    }

    async fn update_danmaku_archive_entry(
        &self,
        plan_entry: &DownloadEntry,
        archive_entry: &mut DownloadArchiveEntryRecord,
        options: &DanmakuUpdateOptions,
    ) -> Result<EntryDanmakuUpdateReport> {
        ensure_directory_exists(&archive_entry.directory).await?;
        let xml_path = archive_entry.directory.join("danmaku.xml");
        let source_path = temporary_path_with_suffix(&xml_path, ".bbdown-source");
        remove_file_if_exists(&source_path).await?;
        let download_options = DownloadOptions::default()
            .with_retry_policy(options.retry)
            .with_resume(false)
            .with_download_idle_timeout(options.download_idle_timeout);
        let source_request =
            DownloadFileRequest::new(plan_entry, &source_path, DownloadFileKind::Danmaku, None);
        let cancellation = DownloadCancellationToken::new();
        let source_file = self
            .download_url_to_file(
                &plan_entry.danmaku.xml_url,
                &source_request,
                &download_options,
                &NoopDownloadProgress,
                &cancellation,
            )
            .await?;
        let fetched_xml = fs::read_to_string(&source_file.path).await;
        let _ = remove_file_if_exists(&source_file.path).await;
        let fetched_xml = fetched_xml?;
        let existing_xml = read_optional_text_file(&xml_path).await?;
        let merged = danmaku::merge_xml_append_only(&existing_xml, &fetched_xml);
        let mut files = Vec::new();
        let xml_file =
            write_generated_text_file(&xml_path, &merged.xml, DownloadFileKind::Danmaku).await?;
        remember_archive_entry_file(archive_entry, &xml_file.path);
        files.push(xml_file);
        if options.danmaku_formats.contains(DanmakuFormat::Ass) {
            let ass = danmaku::xml_to_ass(&merged.xml);
            let ass_file = write_generated_text_file(
                &archive_entry.directory.join("danmaku.ass"),
                &ass,
                DownloadFileKind::DanmakuAss,
            )
            .await?;
            remember_archive_entry_file(archive_entry, &ass_file.path);
            files.push(ass_file);
        }
        Ok(EntryDanmakuUpdateReport {
            index: archive_entry.index,
            aid: archive_entry.aid,
            bvid: archive_entry.bvid.clone(),
            cid: archive_entry.cid,
            epid: archive_entry.epid,
            title: archive_entry.title.clone(),
            directory: archive_entry.directory.clone(),
            existing_comments: merged.existing_comments,
            fetched_comments: merged.fetched_comments,
            appended_comments: merged.appended_comments,
            files,
        })
    }

    async fn download_plan_to_output_dir<P>(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
        output_dir: PathBuf,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        if let Err(error) = validate_download_plan_options(plan, &options) {
            return plan_failed(progress, plan, &output_dir, 0, error);
        }
        if let Err(error) = cancellation.check() {
            emit_plan_cancelled(progress, plan, &output_dir, 0, &error);
            return Err(error);
        }
        if let Err(error) = fs::create_dir_all(&output_dir).await {
            return plan_failed(progress, plan, &output_dir, 0, error.into());
        }
        progress.on_download_progress(&DownloadProgressEvent::PlanStarted {
            title: plan.title.clone(),
            output_dir: output_dir.clone(),
            entry_count: plan.entries.len(),
        });
        let mut entries = Vec::new();
        for entry in &plan.entries {
            if let Err(error) = cancellation.check() {
                emit_plan_cancelled(progress, plan, &output_dir, entries.len(), &error);
                return Err(error);
            }
            let context = DownloadExecutionContext {
                options: &options,
                progress,
                cancellation,
            };
            match self.download_entry(plan, entry, &output_dir, context).await {
                Ok(entry_report) => entries.push(entry_report),
                Err(error) => {
                    if error.is_cancelled() {
                        emit_plan_cancelled(progress, plan, &output_dir, entries.len(), &error);
                    } else {
                        emit_plan_failed(progress, plan, &output_dir, entries.len(), &error);
                    }
                    return Err(error);
                }
            }
        }
        progress.on_download_progress(&DownloadProgressEvent::PlanCompleted {
            title: plan.title.clone(),
            output_dir: output_dir.clone(),
            entry_count: entries.len(),
        });
        Ok(DownloadReport {
            title: plan.title.clone(),
            output_dir,
            entries,
        })
    }

    async fn download_entry<P>(
        &self,
        plan: &DownloadPlan,
        entry: &DownloadEntry,
        output_dir: &Path,
        context: DownloadExecutionContext<'_, P>,
    ) -> Result<EntryDownloadReport>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let options = context.options;
        let progress = context.progress;
        let cancellation = context.cancellation;
        let entry_dir = output_dir.join(entry_dir_name(&plan.title, entry, options)?);
        if let Err(error) = cancellation.check() {
            emit_entry_failed(progress, entry, &entry_dir, &error);
            return Err(error);
        }
        if let Err(error) = fs::create_dir_all(&entry_dir).await {
            let error = Error::Io(error);
            emit_entry_failed(progress, entry, &entry_dir, &error);
            return Err(error);
        }
        progress.on_download_progress(&DownloadProgressEvent::EntryStarted {
            index: entry.index,
            title: entry.title.clone(),
            directory: entry_dir.clone(),
        });
        let mut files = Vec::new();
        if let Err(error) = self
            .download_entry_media(entry, &entry_dir, &mut files, context)
            .await
        {
            emit_entry_failed(progress, entry, &entry_dir, &error);
            return Err(error);
        }
        if let Err(error) = cancellation.check() {
            emit_entry_failed(progress, entry, &entry_dir, &error);
            return Err(error);
        }
        if let Err(error) = self
            .download_entry_sidecars(entry, &entry_dir, &mut files, context)
            .await
        {
            emit_entry_failed(progress, entry, &entry_dir, &error);
            return Err(error);
        }
        if let Err(error) = cancellation.check() {
            emit_entry_failed(progress, entry, &entry_dir, &error);
            return Err(error);
        }
        let mux = match self
            .mux_entry(&plan.title, entry, &entry_dir, &files, context)
            .await
        {
            Ok(mux) => mux,
            Err(error) => {
                emit_entry_failed(progress, entry, &entry_dir, &error);
                return Err(error);
            }
        };
        let report = EntryDownloadReport {
            index: entry.index,
            title: entry.title.clone(),
            directory: entry_dir.clone(),
            files,
            mux,
        };
        progress.on_download_progress(&DownloadProgressEvent::EntryCompleted {
            index: report.index,
            title: report.title.clone(),
            directory: report.directory.clone(),
            file_count: report.files.len(),
            mux_output: report.mux.as_ref().map(|mux| mux.output_path.clone()),
        });
        Ok(report)
    }

    async fn download_entry_media<P>(
        &self,
        entry: &DownloadEntry,
        entry_dir: &Path,
        files: &mut Vec<DownloadedFile>,
        context: DownloadExecutionContext<'_, P>,
    ) -> Result<()>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let options = context.options;
        let cancellation = context.cancellation;
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
                    let audio =
                        select_audio_stream(&entry.streams.audios, &options.stream_selection)?;
                    files.push(
                        self.download_media_stream(
                            entry,
                            video,
                            DownloadFileKind::Video,
                            entry_dir,
                            context,
                        )
                        .await?,
                    );
                    cancellation.check()?;
                    files.push(
                        self.download_media_stream(
                            entry,
                            audio,
                            DownloadFileKind::Audio,
                            entry_dir,
                            context,
                        )
                        .await?,
                    );
                } else if use_flv_fallback {
                    if options.stream_selection.has_selection() {
                        return Err(Error::InvalidInput(
                            "stream selection requires DASH media; selected entry only has FLV segments"
                                .to_owned(),
                        ));
                    }
                    for segment in &entry.streams.flv_segments {
                        cancellation.check()?;
                        files.push(
                            self.download_flv_segment(entry, segment, entry_dir, context)
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
                    self.download_media_stream(
                        entry,
                        video,
                        DownloadFileKind::Video,
                        entry_dir,
                        context,
                    )
                    .await?,
                );
            }
            DownloadMode::AudioOnly => {
                let audio = select_audio_stream(&entry.streams.audios, &options.stream_selection)?;
                files.push(
                    self.download_media_stream(
                        entry,
                        audio,
                        DownloadFileKind::Audio,
                        entry_dir,
                        context,
                    )
                    .await?,
                );
            }
            DownloadMode::SubtitleOnly | DownloadMode::DanmakuOnly | DownloadMode::CoverOnly => {}
        }
        Ok(())
    }

    async fn download_entry_sidecars<P>(
        &self,
        entry: &DownloadEntry,
        entry_dir: &Path,
        files: &mut Vec<DownloadedFile>,
        context: DownloadExecutionContext<'_, P>,
    ) -> Result<()>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let options = context.options;
        let progress = context.progress;
        let cancellation = context.cancellation;
        if should_download_cover(options) {
            if let Some(cover_url) = entry.cover_url.as_deref().filter(|url| !url.is_empty()) {
                cancellation.check()?;
                let cover_path = entry_dir.join(cover_file_name(cover_url));
                let request =
                    DownloadFileRequest::new(entry, &cover_path, DownloadFileKind::Cover, None);
                files.push(
                    self.download_url_to_file(cover_url, &request, options, progress, cancellation)
                        .await?,
                );
            } else if matches!(options.mode, DownloadMode::CoverOnly) {
                return Err(Error::MissingField("cover URL"));
            }
        }
        if should_download_subtitles(options) {
            let mut seen_subtitles = HashSet::new();
            let subtitles = selected_subtitles(&entry.subtitles, options.subtitle_ai_policy);
            for (index, subtitle) in subtitles.iter().copied() {
                if !seen_subtitles.insert(subtitle_dedup_key(&subtitle.url)) {
                    continue;
                }
                cancellation.check()?;
                files.push(
                    self.download_subtitle(index, subtitle, entry, entry_dir, context)
                        .await?,
                );
            }
            if matches!(options.mode, DownloadMode::SubtitleOnly) && seen_subtitles.is_empty() {
                return Err(Error::MissingField("subtitle tracks matching AI policy"));
            }
        }
        if should_download_danmaku(options) {
            cancellation.check()?;
            self.download_danmaku(entry, entry_dir, files, context)
                .await?;
        }
        Ok(())
    }

    async fn download_danmaku<P>(
        &self,
        entry: &DownloadEntry,
        entry_dir: &Path,
        files: &mut Vec<DownloadedFile>,
        context: DownloadExecutionContext<'_, P>,
    ) -> Result<()>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let options = context.options;
        let progress = context.progress;
        let cancellation = context.cancellation;
        let xml_path = entry_dir.join("danmaku.xml");
        let writes_xml = options.danmaku_formats.contains(DanmakuFormat::Xml);
        let writes_ass = options.danmaku_formats.contains(DanmakuFormat::Ass);
        let source_file = if writes_xml {
            let request =
                DownloadFileRequest::new(entry, &xml_path, DownloadFileKind::Danmaku, None);
            let file = self
                .download_url_to_file(
                    &entry.danmaku.xml_url,
                    &request,
                    options,
                    progress,
                    cancellation,
                )
                .await?;
            files.push(file.clone());
            file
        } else {
            let source_path = temporary_path_with_suffix(&xml_path, ".bbdown-source");
            remove_file_if_exists(&source_path).await?;
            let request =
                DownloadFileRequest::new(entry, &source_path, DownloadFileKind::Danmaku, None);
            self.download_url_to_file(
                &entry.danmaku.xml_url,
                &request,
                options,
                &NoopDownloadProgress,
                cancellation,
            )
            .await?
        };
        if writes_ass {
            let ass_file = match async {
                cancellation.check()?;
                let xml = fs::read(&source_file.path).await?;
                let xml = String::from_utf8_lossy(&xml);
                let ass = danmaku::xml_to_ass(&xml);
                write_generated_text_file_with_progress(
                    &entry_dir.join("danmaku.ass"),
                    &ass,
                    DownloadFileKind::DanmakuAss,
                    entry,
                    progress,
                    cancellation,
                )
                .await
            }
            .await
            {
                Ok(ass_file) => ass_file,
                Err(error) => {
                    if !writes_xml {
                        let _ = remove_file_if_exists(&source_file.path).await;
                    }
                    return Err(error);
                }
            };
            files.push(ass_file);
        }
        if !writes_xml {
            let _ = remove_file_if_exists(&source_file.path).await;
        }
        Ok(())
    }

    async fn download_media_stream<P>(
        &self,
        entry: &DownloadEntry,
        stream: &MediaStream,
        kind: DownloadFileKind,
        entry_dir: &Path,
        context: DownloadExecutionContext<'_, P>,
    ) -> Result<DownloadedFile>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let options = context.options;
        let label = match kind {
            DownloadFileKind::Video => "video",
            DownloadFileKind::Audio => "audio",
            DownloadFileKind::FlvSegment
            | DownloadFileKind::Cover
            | DownloadFileKind::Subtitle
            | DownloadFileKind::Danmaku
            | DownloadFileKind::DanmakuAss => "media",
        };
        let path = entry_dir.join(media_file_name(label, stream));
        let request = DownloadFileRequest::new(entry, &path, kind, stream.size);
        self.download_candidate_urls_to_file(
            &candidate_urls(&stream.base_url, &stream.backup_urls, &options.media_hosts),
            &request,
            options,
            context.progress,
            context.cancellation,
        )
        .await
    }

    async fn download_flv_segment<P>(
        &self,
        entry: &DownloadEntry,
        segment: &FlvSegment,
        entry_dir: &Path,
        context: DownloadExecutionContext<'_, P>,
    ) -> Result<DownloadedFile>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let options = context.options;
        let path = entry_dir.join(format!("segment-{:03}.flv", segment.order));
        let request =
            DownloadFileRequest::new(entry, &path, DownloadFileKind::FlvSegment, segment.size);
        self.download_candidate_urls_to_file(
            &candidate_urls(&segment.url, &segment.backup_urls, &options.media_hosts),
            &request,
            options,
            context.progress,
            context.cancellation,
        )
        .await
    }

    async fn download_subtitle<P>(
        &self,
        index: usize,
        subtitle: &SubtitleTrack,
        entry: &DownloadEntry,
        entry_dir: &Path,
        context: DownloadExecutionContext<'_, P>,
    ) -> Result<DownloadedFile>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let path = entry_dir.join(subtitle_file_name(index, subtitle));
        let request = DownloadFileRequest::new(entry, &path, DownloadFileKind::Subtitle, None);
        self.download_url_to_file(
            &subtitle.url,
            &request,
            context.options,
            context.progress,
            context.cancellation,
        )
        .await
    }

    async fn download_url_to_file<P>(
        &self,
        url: &str,
        request: &DownloadFileRequest<'_>,
        options: &DownloadOptions,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadedFile>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let result = self
            .download_url_to_file_without_terminal(url, request, options, progress, cancellation)
            .await;
        match result {
            Ok(file) => Ok(file),
            Err(error) => {
                if error.started || !error.error.is_cancelled() {
                    emit_file_failed(progress, request, error.attempt, &error.error);
                }
                Err(error.error)
            }
        }
    }

    async fn download_url_to_file_without_terminal<P>(
        &self,
        url: &str,
        request: &DownloadFileRequest<'_>,
        options: &DownloadOptions,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> std::result::Result<DownloadedFile, DownloadFileAttemptError>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let attempts = options.retry.max_attempts.max(1);
        let mut last_error = None;
        for attempt in 1..=attempts {
            if let Err(error) = cancellation.check() {
                return Err(DownloadFileAttemptError::new(
                    error,
                    DownloadAttempt {
                        current: attempt,
                        max: attempts,
                    },
                    false,
                ));
            }
            let attempt = DownloadAttempt {
                current: attempt,
                max: attempts,
            };
            match self
                .try_download_url_to_file(url, request, options, attempt, progress, cancellation)
                .await
            {
                Ok(file) => return Ok(file),
                Err(error) if error.error.is_cancelled() => return Err(error),
                Err(error) if attempt.current < attempts => {
                    if error.started {
                        emit_file_failed(progress, request, error.attempt, &error.error);
                    }
                    last_error = Some(error);
                    if !options.retry.backoff.is_zero() {
                        sleep_or_cancel(options.retry.backoff, cancellation)
                            .await
                            .map_err(|error| {
                                DownloadFileAttemptError::new(error, attempt, false)
                            })?;
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            DownloadFileAttemptError::new(
                Error::InvalidInput("download retry failed".to_owned()),
                terminal_download_attempt(options),
                false,
            )
        }))
    }

    async fn download_candidate_urls_to_file<P>(
        &self,
        urls: &[String],
        request: &DownloadFileRequest<'_>,
        options: &DownloadOptions,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> Result<DownloadedFile>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let mut last_error = None;
        for (index, url) in urls.iter().enumerate() {
            cancellation.check()?;
            match self
                .download_url_to_file_without_terminal(
                    url,
                    request,
                    options,
                    progress,
                    cancellation,
                )
                .await
            {
                Ok(file) => return Ok(file),
                Err(error) => {
                    if error.started && index + 1 < urls.len() {
                        emit_file_failed(progress, request, error.attempt, &error.error);
                    }
                    last_error = Some(error);
                }
            }
        }
        let error = last_error.unwrap_or_else(|| {
            DownloadFileAttemptError::new(
                Error::InvalidInput("empty download URL list".to_owned()),
                terminal_download_attempt(options),
                false,
            )
        });
        if error.started || !error.error.is_cancelled() {
            emit_file_failed(progress, request, error.attempt, &error.error);
        }
        Err(error.error)
    }

    async fn try_download_url_to_file<P>(
        &self,
        url: &str,
        request: &DownloadFileRequest<'_>,
        options: &DownloadOptions,
        attempt: DownloadAttempt,
        progress: &P,
        cancellation: &DownloadCancellationToken,
    ) -> std::result::Result<DownloadedFile, DownloadFileAttemptError>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let started = false;
        map_attempt_error(cancellation.check(), attempt, started)?;
        if let Some(parent) = request.path.parent() {
            map_attempt_error(
                fs::create_dir_all(parent).await.map_err(Error::from),
                attempt,
                started,
            )?;
        }
        let existing_len =
            map_attempt_error(existing_file_len(request.path).await, attempt, started)?;
        let resume_from = if options.resume { existing_len } else { 0 };
        map_attempt_error(cancellation.check(), attempt, started)?;
        let response = map_attempt_error(
            await_or_cancel(self.send_download_request(url, resume_from), cancellation).await,
            attempt,
            started,
        )?;
        let status = response.status();
        if resume_from > 0 && status == StatusCode::RANGE_NOT_SATISFIABLE {
            return map_attempt_error(
                already_complete_resume_result(&response, request, resume_from, attempt, progress),
                attempt,
                started,
            );
        }
        let response = map_attempt_error(
            response
                .error_for_status()
                .map_err(BiliClient::http_error_without_url),
            attempt,
            started,
        )?;
        let has_content_range = response.headers().contains_key(CONTENT_RANGE);
        let content_range = map_attempt_error(content_range(response.headers()), attempt, started)?;
        let response_content_len = response.content_length();
        let append = map_attempt_error(
            validate_resume_response(status, resume_from, has_content_range, content_range),
            attempt,
            started,
        )?;
        let start_offset = if append { resume_from } else { 0 };
        let full_retry_after_ignored_range = resume_from > 0 && !append;
        let validation_expected_size = validation_size_for_full_retry(
            request.expected_size,
            content_range,
            response_content_len,
            full_retry_after_ignored_range,
        );
        if full_retry_after_ignored_range && validation_expected_size.is_none() {
            return Err(DownloadFileAttemptError::new(
                Error::InvalidInput(
                    "server ignored resume range without a verifiable full response length"
                        .to_owned(),
                ),
                attempt,
                started,
            ));
        }
        emit_file_started(
            progress,
            request,
            start_offset,
            validation_expected_size,
            attempt,
        );
        Self::finish_started_download_attempt(
            request,
            response,
            WriteResponseRequest {
                content_range,
                start_offset,
                expected_size: validation_expected_size,
                idle_timeout: options.download_idle_timeout,
            },
            existing_len,
            append,
            attempt,
            DownloadExecutionContext {
                options,
                progress,
                cancellation,
            },
        )
        .await
    }

    async fn finish_started_download_attempt<P>(
        request: &DownloadFileRequest<'_>,
        response: reqwest::Response,
        write_request: WriteResponseRequest,
        existing_len: u64,
        append: bool,
        attempt: DownloadAttempt,
        context: DownloadExecutionContext<'_, P>,
    ) -> std::result::Result<DownloadedFile, DownloadFileAttemptError>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let replace_existing = existing_len > 0 && !append;
        let write_path = if replace_existing {
            temporary_download_path(request.path)
        } else {
            request.path.to_path_buf()
        };
        let mut file = map_attempt_error(
            OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(&write_path)
                .await
                .map_err(Error::from),
            attempt,
            true,
        )?;
        let write_result = write_response_body_to_file(
            &mut file,
            response,
            write_request,
            context.progress,
            request,
            context.cancellation,
        )
        .await;
        drop(file);
        let bytes_written = match write_result {
            Ok(bytes_written) => bytes_written,
            Err(error) => {
                if replace_existing || (error.is_cancelled() && write_request.start_offset == 0) {
                    let _ = fs::remove_file(&write_path).await;
                }
                return Err(DownloadFileAttemptError::new(error, attempt, true));
            }
        };
        if is_unexpected_empty_response(&request.kind, bytes_written) {
            if !append {
                let _ = fs::remove_file(&write_path).await;
            }
            return Err(DownloadFileAttemptError::new(
                Error::InvalidInput("empty media response".to_owned()),
                attempt,
                true,
            ));
        }
        if replace_existing {
            map_attempt_error(replace_file(&write_path, request.path).await, attempt, true)?;
        }
        emit_file_completed(
            context.progress,
            request,
            bytes_written,
            write_request.start_offset,
        );
        Ok(DownloadedFile {
            kind: request.kind.clone(),
            path: request.path.to_path_buf(),
            bytes_written,
            resumed_from: write_request.start_offset,
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

    async fn mux_entry<P>(
        &self,
        plan_title: &str,
        entry: &DownloadEntry,
        entry_dir: &Path,
        files: &[DownloadedFile],
        context: DownloadExecutionContext<'_, P>,
    ) -> Result<Option<MuxReport>>
    where
        P: DownloadProgressSink + ?Sized,
    {
        let options = context.options;
        let progress = context.progress;
        let cancellation = context.cancellation;
        if !options.mode.allows_mux() {
            return Ok(None);
        }
        let MuxOptions::Ffmpeg { binary } = &options.mux else {
            return Ok(None);
        };
        cancellation.check()?;
        let media_files = files
            .iter()
            .filter(|file| file.kind.is_media())
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        if media_files.is_empty() {
            return Ok(None);
        }
        let output_path = entry_dir.join(format!(
            "{}.mp4",
            mux_file_stem(plan_title, entry, options)?
        ));
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
        let chapter_metadata = write_ffmpeg_chapter_metadata(entry, entry_dir).await?;
        let chapter_count = if let Some(metadata) = &chapter_metadata {
            let input_index = if only_flv_segments(files) {
                1
            } else {
                media_files.len()
            };
            args.push(OsString::from("-i"));
            args.push(metadata.path.as_os_str().to_os_string());
            args.push(OsString::from("-map_chapters"));
            args.push(OsString::from(input_index.to_string()));
            metadata.chapter_count
        } else {
            0
        };
        args.extend([
            OsString::from("-c"),
            OsString::from("copy"),
            mux_output_path.as_os_str().to_os_string(),
        ]);
        let command = command_report(binary, &args);
        progress.on_download_progress(&DownloadProgressEvent::MuxStarted {
            entry_index: entry.index,
            entry_title: entry.title.clone(),
            output_path: output_path.clone(),
            command: command.clone(),
        });
        if let Err(error) = run_ffmpeg_mux(binary, &args, &mux_output_path, cancellation).await {
            let _ = fs::remove_file(&mux_output_path).await;
            remove_optional_chapter_metadata(chapter_metadata.as_ref()).await;
            emit_mux_failed(progress, entry, &output_path, &command, &error);
            return Err(error);
        }
        if let Err(error) = remove_mux_output_if_cancelled(cancellation, &mux_output_path).await {
            remove_optional_chapter_metadata(chapter_metadata.as_ref()).await;
            return Err(error);
        }
        if let Err(error) = replace_file(&mux_output_path, &output_path).await {
            remove_optional_chapter_metadata(chapter_metadata.as_ref()).await;
            emit_mux_failed(progress, entry, &output_path, &command, &error);
            return Err(error);
        }
        remove_optional_chapter_metadata(chapter_metadata.as_ref()).await;
        progress.on_download_progress(&DownloadProgressEvent::MuxCompleted {
            entry_index: entry.index,
            entry_title: entry.title.clone(),
            output_path: output_path.clone(),
        });
        Ok(Some(MuxReport {
            output_path,
            command,
            chapter_count,
        }))
    }
}

struct FfmpegChapterMetadata {
    path: PathBuf,
    chapter_count: usize,
}

async fn write_ffmpeg_chapter_metadata(
    entry: &DownloadEntry,
    entry_dir: &Path,
) -> Result<Option<FfmpegChapterMetadata>> {
    let metadata = ffmpeg_chapter_metadata(&entry.chapters);
    if metadata.chapter_count == 0 {
        return Ok(None);
    }
    let path = entry_dir.join("ffmpeg-chapters.ffmetadata.bbdown-tmp");
    fs::write(&path, metadata.body).await?;
    Ok(Some(FfmpegChapterMetadata {
        path,
        chapter_count: metadata.chapter_count,
    }))
}

struct FfmpegChapterMetadataBody {
    body: String,
    chapter_count: usize,
}

fn ffmpeg_chapter_metadata(chapters: &[ChapterTrack]) -> FfmpegChapterMetadataBody {
    const TIMEBASE: u64 = 1000;
    let mut body = String::from(";FFMETADATA1\n");
    let mut chapter_count = 0;
    for chapter in chapters {
        if chapter.title.trim().is_empty() || chapter.end_seconds <= chapter.start_seconds {
            continue;
        }
        chapter_count += 1;
        body.push_str("[CHAPTER]\n");
        let _ = writeln!(body, "TIMEBASE=1/{TIMEBASE}");
        let _ = writeln!(
            body,
            "START={}",
            chapter.start_seconds.saturating_mul(TIMEBASE)
        );
        let _ = writeln!(body, "END={}", chapter.end_seconds.saturating_mul(TIMEBASE));
        body.push_str("title=");
        body.push_str(&escape_ffmetadata_value(chapter.title.trim()));
        body.push_str("\n\n");
    }
    FfmpegChapterMetadataBody {
        body,
        chapter_count,
    }
}

fn escape_ffmetadata_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '=' | ';' | '#' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '\n' => escaped.push_str("\\n"),
            '\r' => {}
            _ => escaped.push(character),
        }
    }
    escaped
}

async fn remove_optional_chapter_metadata(metadata: Option<&FfmpegChapterMetadata>) {
    if let Some(metadata) = metadata {
        let _ = fs::remove_file(&metadata.path).await;
    }
}

async fn remove_mux_output_if_cancelled(
    cancellation: &DownloadCancellationToken,
    mux_output_path: &Path,
) -> Result<()> {
    if let Err(error) = cancellation.check() {
        let _ = fs::remove_file(mux_output_path).await;
        return Err(error);
    }
    Ok(())
}

async fn archive_output_dir_for_decision<P>(
    plan: &DownloadPlan,
    effective_options: &mut DownloadOptions,
    preflight: &DownloadPreflight,
    decision: DuplicateDecision,
    progress: &P,
    cancellation: &DownloadCancellationToken,
) -> Result<PathBuf>
where
    P: DownloadProgressSink + ?Sized,
{
    let output_dir = match decision {
        DuplicateDecision::Cancel if preflight.requires_decision() => {
            progress.on_download_progress(&DownloadProgressEvent::PlanCancelled {
                title: plan.title.clone(),
                output_dir: preflight.planned_output_dir.clone(),
                completed_entries: 0,
                error: "archive or output conflict requires a decision".to_owned(),
            });
            return Err(Error::InvalidInput(
                "download canceled because archive or output conflict requires a decision"
                    .to_owned(),
            ));
        }
        DuplicateDecision::Cancel => match default_plan_output_dir(plan, effective_options) {
            Ok(output_dir) => output_dir,
            Err(error) => {
                return plan_failed(progress, plan, &preflight.planned_output_dir, 0, error);
            }
        },
        DuplicateDecision::Replace => {
            prepare_archive_replace_output(
                plan,
                effective_options,
                preflight,
                progress,
                cancellation,
            )
            .await?;
            preflight.planned_output_dir.clone()
        }
        DuplicateDecision::KeepBoth => match preflight.output_dir_for_decision(decision) {
            Ok(output_dir) => output_dir,
            Err(error) => {
                return plan_failed(progress, plan, &preflight.planned_output_dir, 0, error);
            }
        },
    };
    if decision != DuplicateDecision::Replace {
        reject_late_archive_output_conflict(plan, &output_dir, progress)?;
    }
    Ok(output_dir)
}

async fn prepare_archive_replace_output<P>(
    plan: &DownloadPlan,
    effective_options: &mut DownloadOptions,
    preflight: &DownloadPreflight,
    progress: &P,
    cancellation: &DownloadCancellationToken,
) -> Result<()>
where
    P: DownloadProgressSink + ?Sized,
{
    match path_is_occupied(&preflight.planned_output_dir) {
        Ok(true) => {
            if let Err(error) = validate_plan_stream_selection(plan, effective_options) {
                return plan_failed(progress, plan, &preflight.planned_output_dir, 0, error);
            }
            if let Err(error) = cancellation.check() {
                emit_plan_cancelled(progress, plan, &preflight.planned_output_dir, 0, &error);
                return Err(error);
            }
            if let Err(error) = remove_output_root_if_exists(&preflight.planned_output_dir).await {
                return plan_failed(progress, plan, &preflight.planned_output_dir, 0, error);
            }
            effective_options.resume = false;
        }
        Ok(false) => {}
        Err(error) => return plan_failed(progress, plan, &preflight.planned_output_dir, 0, error),
    }
    Ok(())
}

fn reject_late_archive_output_conflict<P>(
    plan: &DownloadPlan,
    output_dir: &Path,
    progress: &P,
) -> Result<()>
where
    P: DownloadProgressSink + ?Sized,
{
    match path_is_occupied(output_dir) {
        Ok(true) => plan_failed(
            progress,
            plan,
            output_dir,
            0,
            Error::InvalidInput(format!(
                "output directory appeared after duplicate preflight: {}",
                output_dir.display()
            )),
        ),
        Ok(false) => Ok(()),
        Err(error) => plan_failed(progress, plan, output_dir, 0, error),
    }
}

impl DownloadFileKind {
    #[must_use]
    pub const fn is_media(&self) -> bool {
        matches!(self, Self::Video | Self::Audio | Self::FlvSegment)
    }

    #[must_use]
    pub const fn is_sidecar(&self) -> bool {
        !self.is_media()
    }

    fn expects_non_empty_response(&self) -> bool {
        matches!(
            self,
            Self::Video | Self::Audio | Self::FlvSegment | Self::Cover
        )
    }
}

struct DownloadExecutionContext<'a, P: ?Sized> {
    options: &'a DownloadOptions,
    progress: &'a P,
    cancellation: &'a DownloadCancellationToken,
}

impl<P: ?Sized> Clone for DownloadExecutionContext<'_, P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P: ?Sized> Copy for DownloadExecutionContext<'_, P> {}

struct DownloadFileRequest<'a> {
    entry: &'a DownloadEntry,
    path: &'a Path,
    kind: DownloadFileKind,
    expected_size: Option<u64>,
}

impl<'a> DownloadFileRequest<'a> {
    fn new(
        entry: &'a DownloadEntry,
        path: &'a Path,
        kind: DownloadFileKind,
        expected_size: Option<u64>,
    ) -> Self {
        Self {
            entry,
            path,
            kind,
            expected_size,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DownloadAttempt {
    current: u32,
    max: u32,
}

#[derive(Debug)]
struct DownloadFileAttemptError {
    error: Error,
    attempt: DownloadAttempt,
    started: bool,
}

impl DownloadFileAttemptError {
    fn new(error: Error, attempt: DownloadAttempt, started: bool) -> Self {
        Self {
            error,
            attempt,
            started,
        }
    }
}

fn map_attempt_error<T>(
    result: Result<T>,
    attempt: DownloadAttempt,
    started: bool,
) -> std::result::Result<T, DownloadFileAttemptError> {
    result.map_err(|error| DownloadFileAttemptError::new(error, attempt, started))
}

#[derive(Clone, Copy, Debug)]
struct WriteResponseRequest {
    content_range: Option<ParsedContentRange>,
    start_offset: u64,
    expected_size: Option<u64>,
    idle_timeout: Option<Duration>,
}

fn command_report(binary: &Path, args: &[OsString]) -> Vec<String> {
    let mut command = Vec::with_capacity(args.len() + 1);
    command.push(binary.to_string_lossy().into_owned());
    command.extend(args.iter().map(|arg| arg.to_string_lossy().into_owned()));
    command
}

async fn run_ffmpeg_mux(
    binary: &Path,
    args: &[OsString],
    mux_output_path: &Path,
    cancellation: &DownloadCancellationToken,
) -> Result<()> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let status = tokio::select! {
        status = child.wait() => status?,
        () = cancellation.cancelled() => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(cancellation.cancelled_error());
        }
    };
    if !status.success() {
        return Err(mux_error_for_failed_status(status, cancellation).await);
    }
    let metadata = fs::metadata(mux_output_path)
        .await
        .map_err(|_| Error::MuxFailed {
            status: "missing output file".to_owned(),
        })?;
    if !metadata.is_file() {
        return Err(Error::MuxFailed {
            status: "missing output file".to_owned(),
        });
    }
    if metadata.len() == 0 {
        return Err(Error::MuxFailed {
            status: "empty output file".to_owned(),
        });
    }
    Ok(())
}

async fn mux_error_for_failed_status(
    status: ExitStatus,
    cancellation: &DownloadCancellationToken,
) -> Error {
    if let Some(error) = mux_cancellation_error_for_status(status, cancellation).await {
        return error;
    }
    Error::MuxFailed {
        status: mux_status_message(status),
    }
}

async fn mux_cancellation_error_for_status(
    status: ExitStatus,
    cancellation: &DownloadCancellationToken,
) -> Option<Error> {
    if cancellation.is_cancelled() {
        return Some(cancellation.cancelled_error());
    }
    #[cfg(unix)]
    {
        // ffmpeg often handles SIGINT itself and exits 255 instead of surfacing a signal status.
        const FFMPEG_INTERRUPTED_EXIT_CODE: i32 = 255;
        const SIGINT_SIGNAL: i32 = 2;
        if (status.signal() == Some(SIGINT_SIGNAL)
            || status.code() == Some(FFMPEG_INTERRUPTED_EXIT_CODE))
            && let Some(error) = wait_for_mux_signal_cancellation(cancellation).await
        {
            return Some(error);
        }
    }
    #[cfg(windows)]
    {
        const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;
        if status
            .code()
            .is_some_and(|code| code as u32 == STATUS_CONTROL_C_EXIT)
            && let Some(error) = wait_for_mux_signal_cancellation(cancellation).await
        {
            return Some(error);
        }
    }
    if cancellation.is_cancelled() {
        Some(cancellation.cancelled_error())
    } else {
        None
    }
}

#[cfg(any(unix, windows))]
async fn wait_for_mux_signal_cancellation(
    cancellation: &DownloadCancellationToken,
) -> Option<Error> {
    tokio::select! {
        () = cancellation.cancelled() => Some(cancellation.cancelled_error()),
        () = tokio::time::sleep(MUX_SIGNAL_CANCELLATION_GRACE) => None,
    }
}

fn mux_status_message(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "terminated by signal".to_owned(),
        |code| code.to_string(),
    )
}

fn already_complete_resume_result<P>(
    response: &reqwest::Response,
    request: &DownloadFileRequest<'_>,
    resume_from: u64,
    attempt: DownloadAttempt,
    progress: &P,
) -> Result<DownloadedFile>
where
    P: DownloadProgressSink + ?Sized,
{
    if content_range_complete_len(response.headers()) == Some(resume_from)
        && request.expected_size.is_none_or(|size| size == resume_from)
    {
        emit_file_started(progress, request, resume_from, Some(resume_from), attempt);
        emit_file_completed(progress, request, 0, resume_from);
        return Ok(DownloadedFile {
            kind: request.kind.clone(),
            path: request.path.to_path_buf(),
            bytes_written: 0,
            resumed_from: resume_from,
        });
    }
    Err(Error::InvalidInput(
        "server rejected resume range for a different file length".to_owned(),
    ))
}

async fn write_response_body_to_file<P>(
    file: &mut tokio::fs::File,
    response: reqwest::Response,
    write_request: WriteResponseRequest,
    progress: &P,
    file_request: &DownloadFileRequest<'_>,
    cancellation: &DownloadCancellationToken,
) -> Result<u64>
where
    P: DownloadProgressSink + ?Sized,
{
    let mut bytes_written = 0_u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = match tokio::select! {
        chunk = next_download_chunk(&mut stream, write_request.idle_timeout) => chunk,
        () = cancellation.cancelled() => {
            rollback_download_file(file, write_request.start_offset).await?;
            return Err(cancellation.cancelled_error());
        }
    } {
        Ok(chunk) => chunk,
        Err(error) => {
            rollback_download_file(file, write_request.start_offset).await?;
            return Err(error);
        }
    } {
        if let Err(error) = cancellation.check() {
            rollback_download_file(file, write_request.start_offset).await?;
            return Err(error);
        }
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                rollback_download_file(file, write_request.start_offset).await?;
                return Err(BiliClient::http_error_without_url(error));
            }
        };
        if let Err(error) = file.write_all(&chunk).await {
            rollback_download_file(file, write_request.start_offset).await?;
            return Err(Error::Io(error));
        }
        let bytes_delta = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        bytes_written = bytes_written.saturating_add(bytes_delta);
        emit_file_progress(
            progress,
            file_request,
            bytes_delta,
            bytes_written,
            write_request.start_offset,
            write_request.expected_size,
        );
    }
    if let Err(error) = cancellation.check() {
        rollback_download_file(file, write_request.start_offset).await?;
        return Err(error);
    }
    if let Err(error) = file.flush().await {
        rollback_download_file(file, write_request.start_offset).await?;
        return Err(Error::Io(error));
    }
    if let Err(error) = validate_download_completion(
        write_request.expected_size,
        write_request.content_range,
        write_request.start_offset,
        bytes_written,
    ) {
        rollback_download_file(file, write_request.start_offset).await?;
        return Err(error);
    }
    Ok(bytes_written)
}

async fn await_or_cancel<T, F>(future: F, cancellation: &DownloadCancellationToken) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    tokio::select! {
        result = future => result,
        () = cancellation.cancelled() => Err(cancellation.cancelled_error()),
    }
}

async fn sleep_or_cancel(
    duration: Duration,
    cancellation: &DownloadCancellationToken,
) -> Result<()> {
    tokio::select! {
        () = tokio::time::sleep(duration) => Ok(()),
        () = cancellation.cancelled() => Err(cancellation.cancelled_error()),
    }
}

fn emit_file_started<P>(
    progress: &P,
    request: &DownloadFileRequest<'_>,
    resumed_from: u64,
    expected_size: Option<u64>,
    attempt: DownloadAttempt,
) where
    P: DownloadProgressSink + ?Sized,
{
    progress.on_download_progress(&DownloadProgressEvent::FileStarted {
        entry_index: request.entry.index,
        entry_title: request.entry.title.clone(),
        kind: request.kind.clone(),
        path: request.path.to_path_buf(),
        resumed_from,
        expected_size,
        attempt: attempt.current,
        max_attempts: attempt.max,
    });
}

fn emit_file_progress<P>(
    progress: &P,
    request: &DownloadFileRequest<'_>,
    bytes_delta: u64,
    bytes_written: u64,
    resumed_from: u64,
    expected_size: Option<u64>,
) where
    P: DownloadProgressSink + ?Sized,
{
    progress.on_download_progress(&DownloadProgressEvent::FileProgress {
        entry_index: request.entry.index,
        entry_title: request.entry.title.clone(),
        kind: request.kind.clone(),
        path: request.path.to_path_buf(),
        bytes_delta,
        bytes_written,
        resumed_from,
        expected_size,
    });
}

fn emit_file_completed<P>(
    progress: &P,
    request: &DownloadFileRequest<'_>,
    bytes_written: u64,
    resumed_from: u64,
) where
    P: DownloadProgressSink + ?Sized,
{
    progress.on_download_progress(&DownloadProgressEvent::FileCompleted {
        entry_index: request.entry.index,
        entry_title: request.entry.title.clone(),
        kind: request.kind.clone(),
        path: request.path.to_path_buf(),
        bytes_written,
        resumed_from,
        total_bytes: resumed_from.saturating_add(bytes_written),
    });
}

fn emit_file_failed<P>(
    progress: &P,
    request: &DownloadFileRequest<'_>,
    attempt: DownloadAttempt,
    error: &Error,
) where
    P: DownloadProgressSink + ?Sized,
{
    progress.on_download_progress(&DownloadProgressEvent::FileFailed {
        entry_index: request.entry.index,
        entry_title: request.entry.title.clone(),
        kind: request.kind.clone(),
        path: request.path.to_path_buf(),
        attempt: attempt.current,
        max_attempts: attempt.max,
        error: error.to_string(),
    });
}

fn terminal_download_attempt(options: &DownloadOptions) -> DownloadAttempt {
    let attempts = options.retry.max_attempts.max(1);
    DownloadAttempt {
        current: attempts,
        max: attempts,
    }
}

fn emit_mux_failed<P>(
    progress: &P,
    entry: &DownloadEntry,
    output_path: &Path,
    command: &[String],
    error: &Error,
) where
    P: DownloadProgressSink + ?Sized,
{
    progress.on_download_progress(&DownloadProgressEvent::MuxFailed {
        entry_index: entry.index,
        entry_title: entry.title.clone(),
        output_path: output_path.to_path_buf(),
        command: command.to_vec(),
        error: error.to_string(),
    });
}

fn emit_entry_failed<P>(progress: &P, entry: &DownloadEntry, directory: &Path, error: &Error)
where
    P: DownloadProgressSink + ?Sized,
{
    progress.on_download_progress(&DownloadProgressEvent::EntryFailed {
        index: entry.index,
        title: entry.title.clone(),
        directory: directory.to_path_buf(),
        error: error.to_string(),
    });
}

fn emit_plan_failed<P>(
    progress: &P,
    plan: &DownloadPlan,
    output_dir: &Path,
    completed_entries: usize,
    error: &Error,
) where
    P: DownloadProgressSink + ?Sized,
{
    emit_plan_failed_title(
        progress,
        plan.title.clone(),
        output_dir,
        completed_entries,
        error,
    );
}

fn emit_plan_failed_title<P>(
    progress: &P,
    title: String,
    output_dir: &Path,
    completed_entries: usize,
    error: &Error,
) where
    P: DownloadProgressSink + ?Sized,
{
    progress.on_download_progress(&DownloadProgressEvent::PlanFailed {
        title,
        output_dir: output_dir.to_path_buf(),
        completed_entries,
        error: error.to_string(),
    });
}

fn emit_plan_cancelled<P>(
    progress: &P,
    plan: &DownloadPlan,
    output_dir: &Path,
    completed_entries: usize,
    error: &Error,
) where
    P: DownloadProgressSink + ?Sized,
{
    emit_plan_cancelled_title(
        progress,
        plan.title.clone(),
        output_dir,
        completed_entries,
        error,
    );
}

fn emit_plan_cancelled_title<P>(
    progress: &P,
    title: String,
    output_dir: &Path,
    completed_entries: usize,
    error: &Error,
) where
    P: DownloadProgressSink + ?Sized,
{
    progress.on_download_progress(&DownloadProgressEvent::PlanCancelled {
        title,
        output_dir: output_dir.to_path_buf(),
        completed_entries,
        error: error.to_string(),
    });
}

fn plan_failed<T, P>(
    progress: &P,
    plan: &DownloadPlan,
    output_dir: &Path,
    completed_entries: usize,
    error: Error,
) -> Result<T>
where
    P: DownloadProgressSink + ?Sized,
{
    emit_plan_failed(progress, plan, output_dir, completed_entries, &error);
    Err(error)
}

fn input_progress_title(input: &Input) -> String {
    match input {
        Input::Aid(aid) => format!("av{aid}"),
        Input::Bvid(bvid) => bvid.clone(),
        Input::Episode(epid) => format!("ep{epid}"),
        Input::Season(season_id) => format!("ss{season_id}"),
        Input::Media(media_id) => format!("md{media_id}"),
        Input::CheeseEpisode(epid) => format!("cheese/ep{epid}"),
        Input::CheeseSeason(season_id) => format!("cheese/ss{season_id}"),
        Input::SpaceVideos(mid) => format!("mid{mid}"),
        Input::FavoriteList {
            media_id,
            owner_mid,
        } => match (media_id, owner_mid) {
            (Some(media_id), _) => format!("fav{media_id}"),
            (None, Some(owner_mid)) => format!("fav/mid{owner_mid}"),
            (None, None) => "favorite".to_owned(),
        },
        Input::CollectionList(id) => format!("collection{id}"),
        Input::SeriesList(id) => format!("series{id}"),
        Input::SpaceCollectionList { list_id, owner_mid } => {
            format!("space/{owner_mid}/collection/{list_id}")
        }
        Input::SpaceSeriesList { list_id, owner_mid } => {
            format!("space/{owner_mid}/series/{list_id}")
        }
        Input::RecommendationFeed => "recommendations".to_owned(),
        Input::FollowingFeed => "following".to_owned(),
        Input::SpaceDynamic(mid) => format!("space/{mid}/dynamic"),
        Input::History => "history".to_owned(),
        Input::WatchLater => "watchlater".to_owned(),
        Input::IntlEpisode(epid) => format!("intl/ep{epid}"),
        Input::ShortLink(url) => url.clone(),
    }
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

fn temporary_generated_path(path: &Path) -> PathBuf {
    temporary_path_with_suffix(path, ".bbdown-generated")
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

async fn write_generated_text_file(
    path: &Path,
    contents: &str,
    kind: DownloadFileKind,
) -> Result<DownloadedFile> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let write_path = temporary_generated_path(path);
    remove_file_if_exists(&write_path).await?;
    fs::write(&write_path, contents).await?;
    replace_file(&write_path, path).await?;
    Ok(DownloadedFile {
        kind,
        path: path.to_path_buf(),
        bytes_written: u64::try_from(contents.len()).unwrap_or(u64::MAX),
        resumed_from: 0,
    })
}

async fn write_generated_text_file_with_progress<P>(
    path: &Path,
    contents: &str,
    kind: DownloadFileKind,
    entry: &DownloadEntry,
    progress: &P,
    cancellation: &DownloadCancellationToken,
) -> Result<DownloadedFile>
where
    P: DownloadProgressSink + ?Sized,
{
    let expected_size = u64::try_from(contents.len()).unwrap_or(u64::MAX);
    let request = DownloadFileRequest::new(entry, path, kind, Some(expected_size));
    emit_file_started(
        progress,
        &request,
        0,
        Some(expected_size),
        DownloadAttempt { current: 1, max: 1 },
    );
    if let Err(error) = cancellation.check() {
        emit_file_failed(
            progress,
            &request,
            DownloadAttempt { current: 1, max: 1 },
            &error,
        );
        return Err(error);
    }
    let file = match write_generated_text_file(path, contents, request.kind.clone()).await {
        Ok(file) => file,
        Err(error) => {
            emit_file_failed(
                progress,
                &request,
                DownloadAttempt { current: 1, max: 1 },
                &error,
            );
            return Err(error);
        }
    };
    emit_file_completed(progress, &request, file.bytes_written, 0);
    Ok(file)
}

fn candidate_urls(
    primary: &str,
    backups: &[String],
    media_hosts: &MediaHostOptions,
) -> Vec<String> {
    let mut urls = Vec::with_capacity(backups.len() + 1);
    push_candidate_url(&mut urls, primary, media_hosts);
    for url in backups.iter().filter(|url| !url.is_empty()) {
        push_candidate_url(&mut urls, url, media_hosts);
    }
    urls
}

fn push_candidate_url(urls: &mut Vec<String>, url: &str, media_hosts: &MediaHostOptions) {
    let candidate = rewrite_media_url_host(url, media_hosts).unwrap_or_else(|| url.to_owned());
    if !urls.iter().any(|existing| existing == &candidate) {
        urls.push(candidate);
    }
}

fn rewrite_media_url_host(url: &str, media_hosts: &MediaHostOptions) -> Option<String> {
    let replacement = media_replacement_host(url, media_hosts)?;
    replace_url_host(url, replacement)
}

fn media_replacement_host<'a>(url: &str, media_hosts: &'a MediaHostOptions) -> Option<&'a str> {
    let configured = media_hosts
        .upos_host
        .as_deref()
        .filter(|host| !host.trim().is_empty());
    if let Some(host) = configured {
        return Some(host);
    }
    if media_hosts.force_replace_host
        || (!media_hosts.allow_pcdn && media_url_needs_host_fallback(url))
    {
        return Some(DEFAULT_UPOS_REPLACEMENT_HOST);
    }
    None
}

fn media_url_needs_host_fallback(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    if is_local_or_private_host(host) {
        return false;
    }
    let host = host.to_ascii_lowercase();
    parsed.port().is_some()
        || host.contains("pcdn")
        || host.contains("mcdn")
        || host.ends_with("akamaized.net")
}

fn is_local_or_private_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
        Err(_) => false,
    }
}

fn replace_url_host(url: &str, replacement_host: &str) -> Option<String> {
    let mut parsed = url::Url::parse(url).ok()?;
    let (host, port) = parse_replacement_host(replacement_host)?;
    parsed.set_host(Some(&host)).ok()?;
    parsed.set_port(port).ok()?;
    Some(parsed.to_string())
}

fn parse_replacement_host(replacement_host: &str) -> Option<(String, Option<u16>)> {
    let replacement_host = replacement_host.trim().trim_end_matches('/');
    if replacement_host.is_empty() {
        return None;
    }
    let authority = if let Some((_, rest)) = replacement_host.split_once("://") {
        rest.split(['/', '?', '#']).next().unwrap_or_default()
    } else {
        replacement_host
    };
    parse_replacement_authority(authority)
}

fn parse_replacement_authority(authority: &str) -> Option<(String, Option<u16>)> {
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let explicit_port = explicit_authority_port(authority)?.into_option();
    let parsed = url::Url::parse(&format!("https://{authority}")).ok()?;
    let host = match parsed.host()? {
        url::Host::Domain(host) => host.to_owned(),
        url::Host::Ipv4(host) => host.to_string(),
        url::Host::Ipv6(host) => format!("[{host}]"),
    };
    Some((host, explicit_port))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExplicitPort {
    Absent,
    Present(u16),
}

impl ExplicitPort {
    fn into_option(self) -> Option<u16> {
        match self {
            Self::Absent => None,
            Self::Present(port) => Some(port),
        }
    }
}

fn explicit_authority_port(authority: &str) -> Option<ExplicitPort> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (_, suffix) = rest.split_once(']')?;
        return parse_optional_port_suffix(suffix);
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() {
            return None;
        }
        return Some(ExplicitPort::Present(port.parse().ok()?));
    }
    Some(ExplicitPort::Absent)
}

fn parse_optional_port_suffix(suffix: &str) -> Option<ExplicitPort> {
    if suffix.is_empty() {
        return Some(ExplicitPort::Absent);
    }
    let port = suffix.strip_prefix(':')?;
    Some(ExplicitPort::Present(port.parse().ok()?))
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

async fn ensure_directory_exists(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path).await?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "download archive entry directory is not a directory: {}",
            path.display()
        )))
    }
}

async fn read_optional_text_file(path: &Path) -> Result<String> {
    match fs::read_to_string(path).await {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(Error::Io(error)),
    }
}

fn remember_archive_entry_file(entry: &mut DownloadArchiveEntryRecord, path: &Path) {
    let path = archive_record_path(path);
    let path_key = comparable_output_path_key(&path);
    if !entry
        .files
        .iter()
        .any(|existing| comparable_output_path_key(existing) == path_key)
    {
        entry.files.push(path);
    }
}

fn danmaku_update_archive_formats(formats: &DanmakuFormats) -> DanmakuFormats {
    let mut effective = Vec::with_capacity(formats.as_slice().len() + 1);
    effective.push(DanmakuFormat::Xml);
    effective.extend(formats.as_slice().iter().copied());
    DanmakuFormats::new(effective)
}

fn refresh_archive_entry_danmaku_content_key(
    entry: &mut DownloadArchiveEntryRecord,
    formats: &DanmakuFormats,
) {
    if let Some(key) = refresh_danmaku_archive_content_key(&entry.content_key, formats) {
        entry.content_key = key;
    }
}

fn refresh_archive_record_danmaku_content_key(
    record: &mut DownloadArchiveRecord,
    formats: &DanmakuFormats,
) {
    if !record
        .entries
        .iter()
        .all(|entry| archive_content_key_has_danmaku_formats(&entry.content_key, formats))
    {
        return;
    }
    if let Some(key) = refresh_danmaku_archive_content_key(&record.content_key, formats) {
        record.content_key = key;
    }
}

fn refresh_danmaku_archive_content_key(key: &str, formats: &DanmakuFormats) -> Option<String> {
    let (prefix, base) = split_archive_content_key_prefix(key);
    let mode = archive_content_key_mode(prefix).unwrap_or("all");
    if !matches!(mode, "all" | "danmaku") {
        return None;
    }
    let refreshed_prefix = archive_key_prefix_for_danmaku_update(prefix, mode, formats);
    Some(apply_archive_key_prefix(
        base.to_owned(),
        refreshed_prefix.as_deref(),
    ))
}

fn archive_content_key_has_danmaku_formats(key: &str, formats: &DanmakuFormats) -> bool {
    refresh_danmaku_archive_content_key(key, formats).is_some_and(|refreshed| refreshed == key)
}

#[must_use]
pub fn archive_entry_allows_danmaku_update(
    record_content_key: &str,
    entry: &DownloadArchiveEntryRecord,
) -> bool {
    if !archive_content_key_allows_danmaku_update(record_content_key)
        || !archive_content_key_allows_danmaku_update(&entry.content_key)
    {
        return false;
    }
    archive_content_key_is_explicit_danmaku(record_content_key)
        || archive_content_key_is_explicit_danmaku(&entry.content_key)
        || archive_entry_has_danmaku_sidecar(entry)
}

fn archive_content_key_allows_danmaku_update(key: &str) -> bool {
    let (prefix, _) = split_archive_content_key_prefix(key);
    matches!(
        archive_content_key_mode(prefix),
        None | Some("all" | "danmaku")
    )
}

fn archive_content_key_is_explicit_danmaku(key: &str) -> bool {
    let (prefix, _) = split_archive_content_key_prefix(key);
    archive_content_key_mode(prefix) == Some("danmaku")
        || prefix.is_some_and(|prefix| {
            prefix
                .split(';')
                .any(|token| token.strip_prefix("danmaku=").is_some())
        })
}

fn archive_entry_has_danmaku_sidecar(entry: &DownloadArchiveEntryRecord) -> bool {
    entry.files.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "danmaku.xml" | "danmaku.ass"))
    })
}

fn split_archive_content_key_prefix(key: &str) -> (Option<&str>, &str) {
    if key.starts_with("plan") || key.starts_with("aid=") {
        return (None, key);
    }
    if let Some(index) = key.find(";plan") {
        return (Some(&key[..index]), &key[index + 1..]);
    }
    if let Some(index) = key.find(";aid=") {
        return (Some(&key[..index]), &key[index + 1..]);
    }
    (None, key)
}

fn archive_content_key_mode(prefix: Option<&str>) -> Option<&str> {
    prefix?
        .split(';')
        .find_map(|token| token.strip_prefix("mode="))
}

fn archive_entry_matches_plan_entry(
    archive_entry: &DownloadArchiveEntryRecord,
    plan_entry: &DownloadEntry,
) -> bool {
    archive_entry.aid == plan_entry.aid && archive_entry.cid == plan_entry.cid
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

fn selected_subtitles(
    subtitles: &[SubtitleTrack],
    policy: SubtitleAiPolicy,
) -> Vec<(usize, &SubtitleTrack)> {
    match policy {
        SubtitleAiPolicy::Include => subtitles.iter().enumerate().collect(),
        SubtitleAiPolicy::ExcludeAi => subtitles
            .iter()
            .enumerate()
            .filter(|(_, subtitle)| !subtitle_track_is_ai_generated(subtitle))
            .collect(),
        SubtitleAiPolicy::OnlyAi => subtitles
            .iter()
            .enumerate()
            .filter(|(_, subtitle)| subtitle_track_is_ai_generated(subtitle))
            .collect(),
        SubtitleAiPolicy::PreferNonAi => {
            let non_ai_languages = subtitles
                .iter()
                .filter(|subtitle| !subtitle_track_is_ai_generated(subtitle))
                .map(subtitle_language_preference_key)
                .collect::<HashSet<_>>();
            subtitles
                .iter()
                .enumerate()
                .filter(|(_, subtitle)| {
                    !subtitle_track_is_ai_generated(subtitle)
                        || !non_ai_languages.contains(&subtitle_language_preference_key(subtitle))
                })
                .collect()
        }
    }
}

fn subtitle_track_is_ai_generated(subtitle: &SubtitleTrack) -> bool {
    subtitle.is_ai_generated
        || subtitle.ai_type.is_some_and(|ai_type| ai_type != 0)
        || subtitle_language_without_ai_prefix(&subtitle.language)
            .is_some_and(|without_prefix| without_prefix != subtitle.language.trim())
}

fn subtitle_language_preference_key(subtitle: &SubtitleTrack) -> String {
    let language = subtitle_language_without_ai_prefix(&subtitle.language)
        .unwrap_or_else(|| subtitle.language.trim())
        .to_ascii_lowercase();
    language
        .split(['-', '_'])
        .next()
        .unwrap_or(language.as_str())
        .to_owned()
}

fn subtitle_language_without_ai_prefix(language: &str) -> Option<&str> {
    let trimmed = language.trim();
    if trimmed
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ai-"))
    {
        Some(trimmed[3..].trim())
    } else {
        None
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

fn default_plan_output_dir(plan: &DownloadPlan, options: &DownloadOptions) -> Result<PathBuf> {
    Ok(options
        .output_dir
        .join(render_template_component_with_budget(
            &options.path_templates.output_dir,
            &TemplateContext::plan(plan),
            MAX_FILE_NAME_BYTES,
        )?))
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
    if preflight.content_key != download_plan_content_key_for_options(plan, options) {
        return Err(Error::InvalidInput(
            "download preflight does not match the download plan".to_owned(),
        ));
    }
    let planned_output_dir = default_plan_output_dir(plan, options)?;
    if comparable_output_path_key(&preflight.planned_output_dir)
        != comparable_output_path_key(&planned_output_dir)
    {
        return Err(Error::InvalidInput(
            "download preflight does not match the download output options".to_owned(),
        ));
    }
    let current_archived_records =
        archive_records_for_preflight(archive, plan, options, &planned_output_dir);
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
    options: &DownloadOptions,
    planned_output_dir: &Path,
) -> Vec<DownloadArchiveRecord> {
    let plan_match = ArchivePlanMatch::from_options(plan, options);
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
    content: HashSet<String>,
    entries: HashSet<String>,
    base_content: String,
    base_entries: HashSet<String>,
    mode_token: Option<&'static str>,
    accepts_legacy_keys: bool,
    accepts_same_mode_prefixed_keys: bool,
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
        let prefixes = archive_key_prefixes_for_mode(mode);
        Self {
            content: prefixes
                .iter()
                .map(|prefix| download_plan_content_key_with_prefix(plan, prefix.as_deref()))
                .collect(),
            entries: plan
                .entries
                .iter()
                .flat_map(|entry| {
                    prefixes.iter().map(|prefix| {
                        download_entry_content_key_with_prefix(entry, prefix.as_deref())
                    })
                })
                .collect(),
            base_content: download_plan_content_key(plan),
            base_entries: plan
                .entries
                .iter()
                .map(download_entry_content_key)
                .collect(),
            mode_token: Some(mode.archive_key_token()),
            accepts_legacy_keys: matches!(mode, DownloadMode::All),
            accepts_same_mode_prefixed_keys: true,
        }
    }

    fn from_options(plan: &DownloadPlan, options: &DownloadOptions) -> Self {
        Self {
            content: [download_plan_content_key_for_options(plan, options)]
                .into_iter()
                .collect(),
            entries: plan
                .entries
                .iter()
                .map(|entry| download_entry_content_key_for_options(entry, options))
                .collect(),
            base_content: download_plan_content_key(plan),
            base_entries: plan
                .entries
                .iter()
                .map(download_entry_content_key)
                .collect(),
            mode_token: Some(options.mode.archive_key_token()),
            accepts_legacy_keys: archive_key_prefix_for_options(options).is_none()
                && matches!(options.mode, DownloadMode::All),
            accepts_same_mode_prefixed_keys: false,
        }
    }

    fn matches_record(&self, record: &DownloadArchiveRecord) -> bool {
        self.content.contains(&record.content_key)
            || self.matches_same_mode_prefixed_key(&record.content_key, ArchiveKeyScope::Content)
            || record.entries.iter().any(|entry| {
                self.entries.contains(&entry.content_key)
                    || (self.accepts_legacy_keys
                        && !archive_content_key_has_mode(&record.content_key)
                        && !archive_content_key_has_mode(&entry.content_key)
                        && self.entries.contains(&archive_entry_content_key(entry)))
                    || self
                        .matches_same_mode_prefixed_key(&entry.content_key, ArchiveKeyScope::Entry)
            })
    }

    fn matches_same_mode_prefixed_key(&self, key: &str, scope: ArchiveKeyScope) -> bool {
        if !self.accepts_same_mode_prefixed_keys {
            return false;
        }
        let (prefix, base) = split_archive_content_key_prefix(key);
        if prefix.is_none() || archive_content_key_mode(prefix) != self.mode_token {
            return false;
        }
        match scope {
            ArchiveKeyScope::Content => base == self.base_content,
            ArchiveKeyScope::Entry => self.base_entries.contains(base),
        }
    }
}

#[derive(Clone, Copy)]
enum ArchiveKeyScope {
    Content,
    Entry,
}

fn archive_content_key_has_mode(key: &str) -> bool {
    key.starts_with("mode=")
}

fn archive_record_path(path: &Path) -> PathBuf {
    comparable_output_path(path)
}

fn archive_record_outputs_are_present(record: &DownloadArchiveRecord) -> bool {
    let mut output_count = 0usize;
    for entry in &record.entries {
        for path in &entry.files {
            output_count += 1;
            if !path.is_file() {
                return false;
            }
        }
        if let Some(mux_output) = &entry.mux_output {
            output_count += 1;
            if !mux_output.is_file() {
                return false;
            }
        }
    }
    output_count > 0
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
    file_component_collision_key(&value)
}

fn file_component_collision_key(value: &str) -> String {
    if cfg!(windows) || cfg!(target_os = "macos") {
        value.to_lowercase()
    } else {
        value.to_owned()
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

fn download_plan_content_key_for_options(plan: &DownloadPlan, options: &DownloadOptions) -> String {
    download_plan_content_key_with_prefix(
        plan,
        archive_key_prefix_for_plan_options(plan, options).as_deref(),
    )
}

fn download_plan_content_key_with_prefix(plan: &DownloadPlan, prefix: Option<&str>) -> String {
    apply_archive_key_prefix(download_plan_content_key(plan), prefix)
}

fn download_entry_content_key(entry: &DownloadEntry) -> String {
    format!("aid={};cid={}", entry.aid, entry.cid)
}

fn download_entry_content_key_for_options(
    entry: &DownloadEntry,
    options: &DownloadOptions,
) -> String {
    download_entry_content_key_with_prefix(
        entry,
        archive_key_prefix_for_entry_options(entry, options).as_deref(),
    )
}

fn download_entry_content_key_with_prefix(entry: &DownloadEntry, prefix: Option<&str>) -> String {
    apply_archive_key_prefix(download_entry_content_key(entry), prefix)
}

fn apply_archive_key_prefix(key: String, prefix: Option<&str>) -> String {
    match prefix {
        Some(prefix) => format!("{prefix};{key}"),
        None => key,
    }
}

fn archive_key_prefix_for_options(options: &DownloadOptions) -> Option<String> {
    archive_key_prefix_with_stream_token(
        options,
        archive_stream_selection_token(&options.stream_selection),
    )
}

fn archive_key_prefix_for_plan_options(
    plan: &DownloadPlan,
    options: &DownloadOptions,
) -> Option<String> {
    archive_key_prefix_with_stream_token(
        options,
        archive_stream_selection_token_for_plan(plan, &options.stream_selection),
    )
}

fn archive_key_prefix_for_entry_options(
    entry: &DownloadEntry,
    options: &DownloadOptions,
) -> Option<String> {
    archive_key_prefix_with_stream_token(
        options,
        archive_stream_selection_token_for_entry(entry, &options.stream_selection),
    )
}

fn archive_key_prefix_with_stream_token(
    options: &DownloadOptions,
    stream_token: Option<String>,
) -> Option<String> {
    let mut tokens = Vec::new();
    if !matches!(options.mode, DownloadMode::All)
        || (should_download_danmaku(options) && !options.danmaku_formats.is_default())
        || (should_download_subtitles(options)
            && options.subtitle_ai_policy.archive_key_token().is_some())
        || options.stream_selection.has_selection()
    {
        tokens.push(format!("mode={}", options.mode.archive_key_token()));
    }
    if should_download_subtitles(options)
        && let Some(policy_token) = options.subtitle_ai_policy.archive_key_token()
    {
        tokens.push(format!("subtitle_ai={policy_token}"));
    }
    if should_download_danmaku(options) && !options.danmaku_formats.is_default() {
        tokens.push(format!(
            "danmaku={}",
            options.danmaku_formats.archive_key_token()
        ));
    }
    if let Some(stream_token) =
        stream_token.or_else(|| archive_stream_selection_token(&options.stream_selection))
    {
        tokens.push(stream_token);
    }
    (!tokens.is_empty()).then(|| tokens.join(";"))
}

fn archive_stream_selection_token_for_plan(
    plan: &DownloadPlan,
    selection: &StreamSelection,
) -> Option<String> {
    if !selection.has_selection() {
        return None;
    }
    let mut entry_tokens = Vec::new();
    for entry in &plan.entries {
        entry_tokens.push(archive_stream_selection_token_for_entry(entry, selection)?);
    }
    if entry_tokens.is_empty() || entry_tokens.iter().all(|token| token == &entry_tokens[0]) {
        return entry_tokens
            .into_iter()
            .next()
            .or_else(|| archive_stream_selection_token(selection));
    }
    let mut identity = String::new();
    for (entry, token) in plan.entries.iter().zip(&entry_tokens) {
        if !identity.is_empty() {
            identity.push('|');
        }
        identity.push_str(&download_entry_content_key(entry));
        identity.push('=');
        identity.push_str(token);
    }
    Some(format!("stream=set{}", short_identity_hash(&identity)))
}

fn archive_stream_selection_token_for_entry(
    entry: &DownloadEntry,
    selection: &StreamSelection,
) -> Option<String> {
    if !selection.has_selection() {
        return None;
    }
    let mut parts = Vec::new();
    if selection.video_quality.is_some() {
        let stream =
            select_media_stream(&entry.streams.videos, selection.video_quality, "video").ok()?;
        parts.push(format!("video{}", stream.id));
    }
    if selection.has_audio_selection() {
        let stream = select_audio_stream(&entry.streams.audios, selection).ok()?;
        parts.push(archive_selected_audio_stream_token(stream, selection));
    }
    (!parts.is_empty()).then(|| format!("stream={}", parts.join(",")))
}

fn archive_selected_audio_stream_token(
    stream: &MediaStream,
    selection: &StreamSelection,
) -> String {
    if selection.audio_language.is_none() {
        return format!("audio{}", stream.id);
    }
    format!(
        "audio{}-{}",
        stream.id,
        archive_stream_selection_text_token(&media_stream_identity(stream))
    )
}

fn archive_stream_selection_token(selection: &StreamSelection) -> Option<String> {
    if !selection.has_selection() {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(video_quality) = selection.video_quality {
        parts.push(format!("video{video_quality}"));
    }
    if let Some(audio_quality) = selection.audio_quality {
        parts.push(format!("audio{audio_quality}"));
    }
    if let Some(audio_language) = selection.audio_language.as_deref() {
        parts.push(format!(
            "alang{}",
            archive_stream_selection_text_token(audio_language)
        ));
    }
    Some(format!("stream={}", parts.join(",")))
}

fn archive_stream_selection_text_token(value: &str) -> String {
    let value = canonical_archive_audio_language(value);
    let token = file_name_token(&value);
    if token.is_empty() {
        short_identity_hash(&value)
    } else {
        format!("{token}-{}", short_identity_hash(&value))
    }
}

fn canonical_archive_audio_language(value: &str) -> String {
    let mut value = value.trim().to_owned();
    value.make_ascii_lowercase();
    value
}

fn archive_key_prefixes_for_mode(mode: DownloadMode) -> Vec<Option<String>> {
    let mut prefixes = vec![archive_key_prefix_for_mode(mode)];
    if matches!(mode, DownloadMode::All | DownloadMode::DanmakuOnly) {
        prefixes.extend(
            DanmakuFormats::non_default_combinations()
                .into_iter()
                .map(|formats| Some(archive_key_prefix_for_danmaku_formats(mode, &formats))),
        );
    }
    prefixes
}

fn archive_key_prefix_for_danmaku_formats(
    mode: DownloadMode,
    danmaku_formats: &DanmakuFormats,
) -> String {
    archive_key_prefix_for_danmaku_formats_token(mode.archive_key_token(), danmaku_formats)
}

fn archive_key_prefix_for_danmaku_update(
    prefix: Option<&str>,
    mode: &str,
    danmaku_formats: &DanmakuFormats,
) -> Option<String> {
    let preserved_tokens = prefix
        .into_iter()
        .flat_map(|prefix| prefix.split(';'))
        .filter(|token| {
            !token.is_empty() && !token.starts_with("mode=") && !token.starts_with("danmaku=")
        });
    let mut tokens = Vec::new();
    let mut subtitle_tokens = Vec::new();
    let mut trailing_tokens = Vec::new();
    for token in preserved_tokens {
        if token.starts_with("subtitle_ai=") {
            subtitle_tokens.push(token.to_owned());
        } else {
            trailing_tokens.push(token.to_owned());
        }
    }
    let has_preserved_tokens = !subtitle_tokens.is_empty() || !trailing_tokens.is_empty();
    if mode != "all" || !danmaku_formats.is_default() || has_preserved_tokens {
        tokens.push(format!("mode={mode}"));
    }
    tokens.extend(subtitle_tokens);
    if !danmaku_formats.is_default() {
        tokens.push(format!("danmaku={}", danmaku_formats.archive_key_token()));
    }
    tokens.extend(trailing_tokens);
    (!tokens.is_empty()).then(|| tokens.join(";"))
}

fn archive_key_prefix_for_danmaku_formats_token(
    mode: &str,
    danmaku_formats: &DanmakuFormats,
) -> String {
    let mode_prefix = format!("mode={mode}");
    format!(
        "{mode_prefix};danmaku={}",
        danmaku_formats.archive_key_token()
    )
}

fn archive_key_prefix_for_mode(mode: DownloadMode) -> Option<String> {
    if matches!(mode, DownloadMode::All) {
        None
    } else {
        Some(format!("mode={}", mode.archive_key_token()))
    }
}

fn archive_entry_content_key(entry: &DownloadArchiveEntryRecord) -> String {
    format!("aid={};cid={}", entry.aid, entry.cid)
}

fn entry_dir_name(
    plan_title: &str,
    entry: &DownloadEntry,
    options: &DownloadOptions,
) -> Result<String> {
    if options.path_templates.entry_dir == DEFAULT_ENTRY_DIR_TEMPLATE {
        let prefix = format!("P{:03}-{}-", entry.index, entry_content_identity(entry));
        return Ok(format_file_component(&prefix, &entry.title, ""));
    }
    render_template_component_with_budget(
        &options.path_templates.entry_dir,
        &TemplateContext::entry(plan_title, entry),
        MAX_FILE_COMPONENT_BYTES,
    )
}

fn mux_file_stem(
    plan_title: &str,
    entry: &DownloadEntry,
    options: &DownloadOptions,
) -> Result<String> {
    render_template_component_with_budget(
        &options.path_templates.mux_file_stem,
        &TemplateContext::entry(plan_title, entry),
        MAX_FILE_NAME_BYTES,
    )
}

struct TemplateContext<'a> {
    plan_title: &'a str,
    entry_count: Option<usize>,
    entry: Option<&'a DownloadEntry>,
}

impl<'a> TemplateContext<'a> {
    fn plan(plan: &'a DownloadPlan) -> Self {
        Self {
            plan_title: &plan.title,
            entry_count: Some(plan.entries.len()),
            entry: None,
        }
    }

    fn entry(plan_title: &'a str, entry: &'a DownloadEntry) -> Self {
        Self {
            plan_title,
            entry_count: None,
            entry: Some(entry),
        }
    }
}

#[cfg(test)]
fn render_template_component(template: &str, context: &TemplateContext<'_>) -> Result<String> {
    render_template_component_with_budget(template, context, MAX_FILE_COMPONENT_BYTES)
}

fn render_template_component_with_budget(
    template: &str,
    context: &TemplateContext<'_>,
    max_bytes: usize,
) -> Result<String> {
    if template.trim().is_empty() {
        return Err(Error::InvalidInput(
            "download path template must not be empty".to_owned(),
        ));
    }
    let mut output = String::new();
    let mut chars = template.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        match character {
            '{' if chars.peek().is_some_and(|(_, next)| *next == '{') => {
                let _ = chars.next();
                output.push('{');
            }
            '{' => {
                let placeholder_start = index + character.len_utf8();
                let mut placeholder_end = None;
                for (next_index, next_character) in chars.by_ref() {
                    if next_character == '}' {
                        placeholder_end = Some(next_index);
                        break;
                    }
                }
                let Some(placeholder_end) = placeholder_end else {
                    return Err(Error::InvalidInput(format!(
                        "download path template has an unclosed placeholder: {template}"
                    )));
                };
                let placeholder = &template[placeholder_start..placeholder_end];
                output.push_str(&render_template_placeholder(placeholder, context)?);
            }
            '}' if chars.peek().is_some_and(|(_, next)| *next == '}') => {
                let _ = chars.next();
                output.push('}');
            }
            '}' => {
                return Err(Error::InvalidInput(format!(
                    "download path template has an unmatched closing brace: {template}"
                )));
            }
            _ => output.push(character),
        }
    }
    Ok(safe_file_name_with_budget(&output, max_bytes))
}

fn render_template_placeholder(placeholder: &str, context: &TemplateContext<'_>) -> Result<String> {
    if placeholder.is_empty() {
        return Err(Error::InvalidInput(
            "download path template contains an empty placeholder".to_owned(),
        ));
    }
    let (name, format) = placeholder
        .split_once(':')
        .map_or((placeholder, None), |(name, format)| (name, Some(format)));
    if name.is_empty() {
        return Err(Error::InvalidInput(
            "download path template contains an empty placeholder".to_owned(),
        ));
    }
    match name {
        "title" => {
            validate_no_placeholder_format(name, format)?;
            Ok(context.plan_title.to_owned())
        }
        "entry_count" => context
            .entry_count
            .map(|value| format_template_number(value as u64, format))
            .transpose()?
            .ok_or_else(|| {
                Error::InvalidInput(
                    "download path template placeholder {entry_count} is only available for output templates"
                        .to_owned(),
                )
            }),
        "entry_title" | "page_title" => entry_template_value(context, name, |entry| {
            validate_no_placeholder_format(name, format)?;
            Ok(entry.title.clone())
        }),
        "index" | "page" => entry_template_value(context, name, |entry| {
            format_template_number(u64::from(entry.index), format)
        }),
        "aid" => entry_template_value(context, name, |entry| {
            format_template_number(entry.aid, format)
        }),
        "bvid" => entry_template_value(context, name, |entry| {
            validate_no_placeholder_format(name, format)?;
            Ok(entry.bvid.clone().unwrap_or_default())
        }),
        "cid" => entry_template_value(context, name, |entry| {
            format_template_number(entry.cid, format)
        }),
        "epid" => entry_template_value(context, name, |entry| {
            entry.epid.map_or_else(
                || {
                    validate_no_placeholder_format(name, format)?;
                    Ok(String::new())
                },
                |epid| format_template_number(epid, format),
            )
        }),
        "content_id" => entry_template_value(context, name, |entry| {
            validate_no_placeholder_format(name, format)?;
            Ok(entry_content_identity(entry))
        }),
        _ => Err(Error::InvalidInput(format!(
            "unknown download path template placeholder {{{name}}}"
        ))),
    }
}

fn entry_template_value(
    context: &TemplateContext<'_>,
    name: &str,
    render: impl FnOnce(&DownloadEntry) -> Result<String>,
) -> Result<String> {
    let entry = context.entry.ok_or_else(|| {
        Error::InvalidInput(format!(
            "download path template placeholder {{{name}}} is only available for entry templates"
        ))
    })?;
    render(entry)
}

fn format_template_number(value: u64, format: Option<&str>) -> Result<String> {
    match format {
        None | Some("") => Ok(value.to_string()),
        Some(format) => {
            let width = format.strip_prefix('0').ok_or_else(|| {
                Error::InvalidInput(format!("unsupported template number format :{format}"))
            })?;
            if width.is_empty() || !width.chars().all(|character| character.is_ascii_digit()) {
                return Err(Error::InvalidInput(format!(
                    "unsupported template number format :{format}"
                )));
            }
            let width: usize = width.parse().map_err(|_| {
                Error::InvalidInput(format!("unsupported template number format :{format}"))
            })?;
            if width > MAX_FILE_COMPONENT_BYTES {
                return Err(Error::InvalidInput(format!(
                    "template number format width must be at most {MAX_FILE_COMPONENT_BYTES}: :{format}"
                )));
            }
            Ok(format!("{value:0width$}"))
        }
    }
}

fn validate_no_placeholder_format(name: &str, format: Option<&str>) -> Result<()> {
    if let Some(format) = format.filter(|format| !format.is_empty()) {
        return Err(Error::InvalidInput(format!(
            "placeholder {{{name}}} does not support format :{format}"
        )));
    }
    Ok(())
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

fn select_audio_stream<'a>(
    streams: &'a [MediaStream],
    selection: &StreamSelection,
) -> Result<&'a MediaStream> {
    streams
        .iter()
        .find(|stream| {
            selection.audio_quality.is_none_or(|id| stream.id == id)
                && selection
                    .audio_language
                    .as_deref()
                    .is_none_or(|language| audio_language_matches(stream, language))
        })
        .ok_or_else(|| audio_selection_error(streams, selection))
}

fn audio_selection_error(streams: &[MediaStream], selection: &StreamSelection) -> Error {
    match (selection.audio_quality, selection.audio_language.as_deref()) {
        (Some(id), Some(language)) => Error::InvalidInput(format!(
            "requested audio quality {id} with language {language} is not available; available audio streams: {}",
            available_audio_streams(streams)
        )),
        (Some(id), None) => Error::InvalidInput(format!(
            "requested audio quality {id} is not available; available audio qualities: {}",
            available_stream_ids(streams)
        )),
        (None, Some(language)) => Error::InvalidInput(format!(
            "requested audio language {language} is not available; available audio languages: {}",
            available_audio_languages(streams)
        )),
        (None, None) => Error::MissingField("selected media stream"),
    }
}

fn audio_language_matches(stream: &MediaStream, requested: &str) -> bool {
    let requested = requested.trim();
    [stream.language.as_deref(), stream.language_doc.as_deref()]
        .into_iter()
        .flatten()
        .any(|candidate| string_matches_audio_language(candidate, requested))
}

fn string_matches_audio_language(candidate: &str, requested: &str) -> bool {
    let candidate = candidate.trim();
    candidate == requested || candidate.eq_ignore_ascii_case(requested)
}

fn validate_download_plan_options(plan: &DownloadPlan, options: &DownloadOptions) -> Result<()> {
    validate_plan_stream_selection(plan, options)?;
    validate_download_path_templates(plan, options)
}

fn validate_plan_stream_selection(plan: &DownloadPlan, options: &DownloadOptions) -> Result<()> {
    let selection = &options.stream_selection;
    if !selection.has_selection() {
        return Ok(());
    }
    validate_stream_selection_mode(selection, options.mode)?;
    for entry in &plan.entries {
        validate_entry_stream_selection(entry, selection, options.mode)?;
    }
    Ok(())
}

fn validate_download_path_templates(plan: &DownloadPlan, options: &DownloadOptions) -> Result<()> {
    let _ = default_plan_output_dir(plan, options)?;
    let mut rendered_entry_dirs = HashSet::new();
    for entry in &plan.entries {
        let entry_dir = entry_dir_name(&plan.title, entry, options)?;
        if !rendered_entry_dirs.insert(file_component_collision_key(&entry_dir)) {
            return Err(Error::InvalidInput(format!(
                "download entry template renders duplicate directory name: {entry_dir}"
            )));
        }
        if options.mode.allows_mux() && matches!(options.mux, MuxOptions::Ffmpeg { .. }) {
            let _ = mux_file_stem(&plan.title, entry, options)?;
        }
    }
    Ok(())
}

fn validate_stream_selection_mode(selection: &StreamSelection, mode: DownloadMode) -> Result<()> {
    match mode {
        DownloadMode::VideoOnly if selection.has_audio_selection() => Err(Error::InvalidInput(
            "audio selection requires all or audio-only download mode".to_owned(),
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
    selection: &StreamSelection,
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
                let _ = select_audio_stream(&entry.streams.audios, selection)?;
            } else if use_flv_fallback {
                return Err(Error::InvalidInput(
                    "stream selection requires DASH media; selected entry only has FLV segments"
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
            let _ = select_audio_stream(&entry.streams.audios, selection)?;
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

fn available_audio_languages(streams: &[MediaStream]) -> String {
    if streams.is_empty() {
        return "none".to_owned();
    }
    let mut labels = Vec::new();
    for stream in streams {
        let label = audio_language_label(stream);
        if !labels.iter().any(|existing| existing == &label) {
            labels.push(label);
        }
    }
    labels.join(", ")
}

fn available_audio_streams(streams: &[MediaStream]) -> String {
    if streams.is_empty() {
        return "none".to_owned();
    }
    streams
        .iter()
        .map(|stream| format!("{} ({})", stream.id, audio_language_label(stream)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn audio_language_label(stream: &MediaStream) -> String {
    match (stream.language.as_deref(), stream.language_doc.as_deref()) {
        (Some(language), Some(language_doc)) => format!("{language} {language_doc}"),
        (Some(language), None) => language.to_owned(),
        (None, Some(language_doc)) => language_doc.to_owned(),
        (None, None) => "und".to_owned(),
    }
}

fn media_stream_identity(stream: &MediaStream) -> String {
    let mut parts = Vec::new();
    push_identity_part(&mut parts, stream.language.as_deref());
    push_identity_part(&mut parts, stream.language_doc.as_deref());
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
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        let token = file_name_token(value);
        if token.is_empty() {
            parts.push(short_identity_hash(value));
        } else {
            parts.push(token);
        }
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
        DEFAULT_UPOS_REPLACEMENT_HOST, DanmakuUpdateOptions, DownloadArchive,
        DownloadArchiveEntryRecord, DownloadArchiveRecord, DownloadFileRequest, DownloadMode,
        DownloadOptions, DownloadPathTemplates, DownloadPreflight, DownloadReport,
        DownloadReportSummary, DownloadedFile, DuplicateDecision, EntryDownloadReport,
        EntryDownloadSummary, MAX_FILE_COMPONENT_BYTES, MAX_FILE_NAME_BYTES,
        MAX_SUBTITLE_EXTENSION_BYTES, MediaHostOptions, MuxOptions, MuxReport, RetryPolicy,
        SidecarOptions, StreamSelection, SubtitleAiPolicy, TemplateContext, archive_sidecar_path,
        candidate_urls, comparable_output_path, cover_file_name, default_plan_output_dir,
        download_entry_content_key, download_entry_content_key_for_options,
        download_plan_content_key, download_plan_content_key_for_options, entry_dir_name,
        media_file_name, mux_file_stem, path_is_occupied, remove_mux_output_if_cancelled,
        render_template_component, safe_file_name, safe_file_name_with_budget, select_audio_stream,
        select_media_stream, selected_subtitles, subtitle_dedup_key, subtitle_extension,
        subtitle_file_name, temporary_download_path, temporary_generated_path, temporary_mux_path,
        temporary_replace_path, write_generated_text_file,
    };
    use crate::models::{
        ChapterTrack, DanmakuTrack, DownloadEntry, DownloadPlan, FlvSegment, MediaStream,
        StreamDiagnostics, StreamQuality, StreamSet, StreamSource, SubtitleFormat, SubtitleTrack,
    };
    use crate::{
        BiliClient, ClientConfig, Credentials, DanmakuFormat, DanmakuFormats,
        DownloadCancellationToken, DownloadFileKind, DownloadProgressEvent, NoopDownloadProgress,
    };
    use httpmock::MockServer;
    use httpmock::prelude::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    #[cfg(unix)]
    use std::{fs as std_fs, os::unix::fs::PermissionsExt};

    #[test]
    fn media_file_name_distinguishes_stream_identity_without_query() {
        let mut stream = MediaStream {
            id: 80,
            base_url: "https://cdn.example/path/video.m4s?token=first".to_owned(),
            backup_urls: Vec::new(),
            language: None,
            language_doc: None,
            codecs: Some("avc1.640028".to_owned()),
            codec_family: None,
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
    fn media_file_name_distinguishes_non_ascii_language_doc_only_streams() {
        let mut first = media_stream(30280, "https://cdn.example/audio-ja.m4s");
        first.language_doc = Some("日本語".to_owned());
        first.codecs = Some("mp4a.40.2".to_owned());
        first.mime_type = Some("audio/mp4".to_owned());
        let mut second = first.clone();
        second.base_url = "https://cdn.example/audio-zh.m4s".to_owned();
        second.language_doc = Some("中文".to_owned());

        let first_name = media_file_name("audio", &first);
        let second_name = media_file_name("audio", &second);

        assert_ne!(first_name, second_name);
        assert!(first_name.contains(&super::short_identity_hash("日本語")));
        assert!(second_name.contains(&super::short_identity_hash("中文")));
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
    fn select_audio_stream_supports_language_metadata() -> crate::Result<()> {
        let mut ja = media_stream(30280, "https://cdn.example/audio-ja.m4s");
        ja.language = Some("ja-JP".to_owned());
        ja.language_doc = Some("Japanese".to_owned());
        let mut en = media_stream(30280, "https://cdn.example/audio-en.m4s");
        en.language = Some("en-US".to_owned());
        en.language_doc = Some("English".to_owned());
        let streams = vec![ja, en];

        let selected = select_audio_stream(&streams, &StreamSelection::audio_language("english"))?;
        assert_eq!(selected.base_url, "https://cdn.example/audio-en.m4s");

        let selected = select_audio_stream(&streams, &StreamSelection::audio_language("JA-jp"))?;
        assert_eq!(selected.base_url, "https://cdn.example/audio-ja.m4s");
        Ok(())
    }

    #[test]
    fn select_audio_stream_reports_available_languages() -> crate::Result<()> {
        let mut stream = media_stream(30280, "https://cdn.example/audio-ja.m4s");
        stream.language = Some("ja-JP".to_owned());
        stream.language_doc = Some("Japanese".to_owned());
        let streams = vec![stream];

        let selection = StreamSelection::audio(30216).with_audio_language("en-US");
        let Err(error) = select_audio_stream(&streams, &selection) else {
            return Err(crate::Error::InvalidInput(
                "unexpectedly selected missing audio stream".to_owned(),
            ));
        };
        assert_eq!(
            error.to_string(),
            "invalid input: requested audio quality 30216 with language en-US is not available; available audio streams: 30280 (ja-JP Japanese)"
        );
        Ok(())
    }

    #[test]
    fn download_options_builders_configure_embedding_controls() -> anyhow::Result<()> {
        let options = DownloadOptions::new("downloads")
            .with_retry_policy(RetryPolicy::new(5, Duration::from_secs(2)))
            .with_stream_selection(super::StreamSelection::video(80).with_audio_language("ja-JP"))
            .with_download_idle_timeout(None)
            .with_resume(false)
            .with_cover(true)
            .with_subtitles(false)
            .with_danmaku(false)
            .with_danmaku_formats([DanmakuFormat::Xml, DanmakuFormat::Ass])
            .with_media_hosts(
                MediaHostOptions::new()
                    .with_upos_host("upos.example")
                    .with_force_replace_host(true)
                    .with_allow_pcdn(false),
            )
            .with_mux(MuxOptions::ffmpeg("ffmpeg-custom"));

        assert_eq!(options.output_dir.as_path(), Path::new("downloads"));
        assert_eq!(options.retry.max_attempts, 5);
        assert_eq!(options.retry.backoff, Duration::from_secs(2));
        assert_eq!(options.stream_selection.video_quality, Some(80));
        assert_eq!(options.stream_selection.audio_quality, None);
        assert_eq!(
            options.stream_selection.audio_language.as_deref(),
            Some("ja-JP")
        );
        assert_eq!(options.download_idle_timeout, None);
        assert!(!options.resume);
        assert!(!options.include_subtitles);
        assert!(!options.include_danmaku);
        assert_eq!(
            options.danmaku_formats,
            DanmakuFormats::new([DanmakuFormat::Xml, DanmakuFormat::Ass])
        );
        assert_eq!(
            options.media_hosts,
            MediaHostOptions {
                upos_host: Some("upos.example".to_owned()),
                force_replace_host: true,
                allow_pcdn: false,
            }
        );
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
    fn candidate_urls_preserve_media_hosts_by_default() {
        let options = MediaHostOptions::default();
        let urls = candidate_urls(
            "https://video.example:448/video.m4s?token=1",
            &["https://backup.example/video.m4s".to_owned()],
            &options,
        );

        assert_eq!(
            urls,
            vec![
                "https://video.example:448/video.m4s?token=1",
                "https://backup.example/video.m4s"
            ]
        );
    }

    #[test]
    fn candidate_urls_rewrite_remote_pcdn_hosts_for_cli_default() {
        let options = MediaHostOptions::bbdown_cli_default();
        let urls = candidate_urls("https://pcdn.example:448/video.m4s?token=1", &[], &options);

        assert_eq!(
            urls,
            vec![format!(
                "https://{DEFAULT_UPOS_REPLACEMENT_HOST}/video.m4s?token=1"
            )]
        );
    }

    #[test]
    fn candidate_urls_do_not_rewrite_local_ports_for_cli_default() {
        let options = MediaHostOptions::bbdown_cli_default();
        let urls = candidate_urls("http://127.0.0.1:3100/video.m4s", &[], &options);

        assert_eq!(urls, vec!["http://127.0.0.1:3100/video.m4s"]);
    }

    #[test]
    fn candidate_urls_rewrite_custom_upos_hosts_and_dedupe() {
        let options = MediaHostOptions::new().with_upos_host("upos.example:8443");
        let urls = candidate_urls(
            "https://primary.example/video.m4s?token=1",
            &[
                "https://backup.example/video.m4s?token=1".to_owned(),
                "https://backup.example/audio.m4s".to_owned(),
            ],
            &options,
        );

        assert_eq!(
            urls,
            vec![
                "https://upos.example:8443/video.m4s?token=1",
                "https://upos.example:8443/audio.m4s"
            ]
        );
    }

    #[test]
    fn candidate_urls_preserve_explicit_replacement_default_port() {
        let options = MediaHostOptions::new().with_upos_host("upos.example:443");
        let urls = candidate_urls("http://primary.example/video.m4s", &[], &options);

        assert_eq!(urls, vec!["http://upos.example:443/video.m4s"]);
    }

    #[test]
    fn candidate_urls_preserve_explicit_replacement_default_port_from_url_like_input() {
        let options = MediaHostOptions::new().with_upos_host("https://upos.example:443");
        let urls = candidate_urls("http://primary.example/video.m4s", &[], &options);

        assert_eq!(urls, vec!["http://upos.example:443/video.m4s"]);
    }

    #[test]
    fn candidate_urls_rewrite_custom_ipv6_upos_host() {
        let options = MediaHostOptions::new().with_upos_host("[::1]:8080");
        let urls = candidate_urls("http://primary.example/video.m4s?token=1", &[], &options);

        assert_eq!(urls, vec!["http://[::1]:8080/video.m4s?token=1"]);
    }

    #[test]
    fn candidate_urls_force_replace_ordinary_hosts() {
        let options = MediaHostOptions::new().with_force_replace_host(true);
        let urls = candidate_urls("https://cdn.example/video.m4s", &[], &options);

        assert_eq!(
            urls,
            vec![format!("https://{DEFAULT_UPOS_REPLACEMENT_HOST}/video.m4s")]
        );
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
    fn entry_dir_name_limits_final_component_bytes() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mut plan = test_plan(&server);
        plan.entries[0].title = "界".repeat(200);

        assert!(
            entry_dir_name(&plan.title, &plan.entries[0], &DownloadOptions::default())?.len()
                <= MAX_FILE_COMPONENT_BYTES
        );
        Ok(())
    }

    #[test]
    fn entry_dir_name_distinguishes_content_identity() -> anyhow::Result<()> {
        let server = MockServer::start();
        let first = test_plan(&server);
        let mut second = test_plan(&server);
        second.entries[0].aid = 170_002;
        second.entries[0].bvid = Some("BV1yy411c7mD".to_owned());
        second.entries[0].cid = 3;

        assert_ne!(
            entry_dir_name(&first.title, &first.entries[0], &DownloadOptions::default())?,
            entry_dir_name(
                &second.title,
                &second.entries[0],
                &DownloadOptions::default()
            )?
        );
        Ok(())
    }

    #[test]
    fn default_path_templates_preserve_existing_names() -> anyhow::Result<()> {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let options = DownloadOptions::new("downloads");

        assert_eq!(
            default_plan_output_dir(&plan, &options)?,
            Path::new("downloads").join("Mock video")
        );
        assert_eq!(
            entry_dir_name(&plan.title, &plan.entries[0], &options)?,
            "P001-BV1xx411c7mD-cid2-Main"
        );
        assert_eq!(
            mux_file_stem(&plan.title, &plan.entries[0], &options)?,
            "Main"
        );
        Ok(())
    }

    #[test]
    fn default_path_templates_preserve_existing_truncation_budgets() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mut plan = test_plan(&server);
        plan.title = "A".repeat(MAX_FILE_NAME_BYTES + 10);
        plan.entries[0].title = "B".repeat(MAX_FILE_NAME_BYTES + 10);
        let options = DownloadOptions::new("downloads");

        let output_file_name = default_plan_output_dir(&plan, &options)?
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| anyhow::anyhow!("missing output file name"))?
            .to_owned();
        let mux_file_name = mux_file_stem(&plan.title, &plan.entries[0], &options)?;

        assert_eq!(output_file_name.len(), MAX_FILE_NAME_BYTES);
        assert_eq!(mux_file_name.len(), MAX_FILE_NAME_BYTES);
        assert!(
            entry_dir_name(&plan.title, &plan.entries[0], &options)?.len()
                <= MAX_FILE_COMPONENT_BYTES
        );
        Ok(())
    }

    #[test]
    fn default_entry_template_preserves_existing_title_sanitization() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mut plan = test_plan(&server);
        let options = DownloadOptions::new("downloads");

        plan.entries[0].title = " .Part 1".to_owned();
        assert_eq!(
            entry_dir_name(&plan.title, &plan.entries[0], &options)?,
            "P001-BV1xx411c7mD-cid2-Part 1"
        );

        plan.entries[0].title = ".".to_owned();
        assert_eq!(
            entry_dir_name(&plan.title, &plan.entries[0], &options)?,
            "P001-BV1xx411c7mD-cid2-untitled"
        );
        Ok(())
    }

    #[test]
    fn path_templates_render_known_placeholders() -> anyhow::Result<()> {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let options = DownloadOptions::new("downloads").with_path_templates(
            DownloadPathTemplates::new()
                .with_output_dir("{title}-{entry_count:02}")
                .with_entry_dir("{index:02}-{entry_title}-{aid}-{cid}-{content_id}")
                .with_mux_file_stem("{index:03}-{entry_title}-{bvid}"),
        );

        assert_eq!(
            default_plan_output_dir(&plan, &options)?,
            Path::new("downloads").join("Mock video-01")
        );
        assert_eq!(
            entry_dir_name(&plan.title, &plan.entries[0], &options)?,
            "01-Main-170001-2-BV1xx411c7mD-cid2"
        );
        assert_eq!(
            mux_file_stem(&plan.title, &plan.entries[0], &options)?,
            "001-Main-BV1xx411c7mD"
        );
        Ok(())
    }

    #[test]
    fn path_templates_reject_unknown_placeholders() {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let error = render_template_component(
            "{unknown}",
            &TemplateContext::entry(&plan.title, &plan.entries[0]),
        )
        .err();

        assert!(
            matches!(error, Some(crate::Error::InvalidInput(message)) if message.contains("unknown download path template placeholder"))
        );
    }

    #[test]
    fn path_templates_reject_excessive_number_width() {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let error = render_template_component(
            "{index:0241}",
            &TemplateContext::entry(&plan.title, &plan.entries[0]),
        )
        .err();

        assert!(
            matches!(error, Some(crate::Error::InvalidInput(message)) if message.contains("template number format width must be at most"))
        );
    }

    #[test]
    fn path_templates_reject_string_placeholder_formats() {
        let server = MockServer::start();
        let plan = test_plan(&server);

        let title_error =
            render_template_component("{title:03}", &TemplateContext::plan(&plan)).err();
        let entry_title_error = render_template_component(
            "{entry_title:03}",
            &TemplateContext::entry(&plan.title, &plan.entries[0]),
        )
        .err();

        assert!(
            matches!(title_error, Some(crate::Error::InvalidInput(message)) if message.contains("does not support format"))
        );
        assert!(
            matches!(entry_title_error, Some(crate::Error::InvalidInput(message)) if message.contains("does not support format"))
        );
    }

    #[test]
    fn path_templates_reject_duplicate_entry_directories() {
        let server = MockServer::start();
        let mut plan = test_plan(&server);
        let mut second = plan.entries[0].clone();
        second.index = 2;
        second.aid = 170_002;
        second.cid = 3;
        plan.entries.push(second);
        let options = DownloadOptions::new("downloads").with_entry_template("{entry_title}");
        let error = DownloadPreflight::inspect(&plan, &options, None).err();

        assert!(
            matches!(error, Some(crate::Error::InvalidInput(message)) if message.contains("duplicate directory name"))
        );
    }

    #[test]
    fn path_templates_escape_literal_braces_and_sanitize_paths() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mut plan = test_plan(&server);
        plan.entries[0].title = "A/B:C".to_owned();
        let component = render_template_component(
            "{{{entry_title}}}",
            &TemplateContext::entry(&plan.title, &plan.entries[0]),
        )?;

        assert_eq!(component, "{A-B-C}");
        Ok(())
    }

    #[test]
    fn subtitle_file_name_distinguishes_duplicate_languages() {
        let first = SubtitleTrack {
            language: "en".to_owned(),
            language_doc: Some("English".to_owned()),
            is_ai_generated: false,
            ai_type: None,
            ai_status: None,
            url: "https://subtitle.example/first.ass".to_owned(),
            format: SubtitleFormat::Ass,
        };
        let second = SubtitleTrack {
            language: "en".to_owned(),
            language_doc: Some("English".to_owned()),
            is_ai_generated: false,
            ai_type: None,
            ai_status: None,
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
            is_ai_generated: false,
            ai_type: None,
            ai_status: None,
            url: format!("https://subtitle.example/file.{}", "x".repeat(200)),
            format: SubtitleFormat::Unknown,
        };

        assert!(subtitle_extension(&subtitle).len() <= MAX_SUBTITLE_EXTENSION_BYTES);
        assert!(subtitle_file_name(0, &subtitle).len() <= MAX_FILE_COMPONENT_BYTES);
    }

    #[test]
    fn subtitle_ai_policy_filters_tracks() {
        let subtitles = vec![
            SubtitleTrack {
                language: "en".to_owned(),
                language_doc: Some("English".to_owned()),
                is_ai_generated: false,
                ai_type: None,
                ai_status: None,
                url: "https://subtitle.example/en.ass".to_owned(),
                format: SubtitleFormat::Ass,
            },
            SubtitleTrack {
                language: "ai-en".to_owned(),
                language_doc: Some("English (AI)".to_owned()),
                is_ai_generated: true,
                ai_type: Some(1),
                ai_status: Some(2),
                url: "https://subtitle.example/ai-en.ass".to_owned(),
                format: SubtitleFormat::Ass,
            },
            SubtitleTrack {
                language: "ai-ja".to_owned(),
                language_doc: Some("Japanese (AI)".to_owned()),
                is_ai_generated: false,
                ai_type: Some(1),
                ai_status: None,
                url: "https://subtitle.example/ai-ja.ass".to_owned(),
                format: SubtitleFormat::Ass,
            },
        ];

        let languages = |policy| {
            selected_subtitles(&subtitles, policy)
                .into_iter()
                .map(|(_, subtitle)| subtitle.language.as_str())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            languages(SubtitleAiPolicy::Include),
            vec!["en", "ai-en", "ai-ja"]
        );
        assert_eq!(languages(SubtitleAiPolicy::ExcludeAi), vec!["en"]);
        assert_eq!(languages(SubtitleAiPolicy::OnlyAi), vec!["ai-en", "ai-ja"]);
        assert_eq!(
            languages(SubtitleAiPolicy::PreferNonAi),
            vec!["en", "ai-ja"]
        );
    }

    #[test]
    fn subtitle_ai_policy_prefers_regioned_manual_track_over_generic_ai_track() {
        let subtitles = vec![
            SubtitleTrack {
                language: "zh-CN".to_owned(),
                language_doc: Some("Chinese".to_owned()),
                is_ai_generated: false,
                ai_type: None,
                ai_status: None,
                url: "https://subtitle.example/zh-cn.ass".to_owned(),
                format: SubtitleFormat::Ass,
            },
            SubtitleTrack {
                language: "ai-zh".to_owned(),
                language_doc: Some("Chinese (AI)".to_owned()),
                is_ai_generated: true,
                ai_type: Some(1),
                ai_status: Some(2),
                url: "https://subtitle.example/ai-zh.ass".to_owned(),
                format: SubtitleFormat::Ass,
            },
        ];

        let languages = selected_subtitles(&subtitles, SubtitleAiPolicy::PreferNonAi)
            .into_iter()
            .map(|(_, subtitle)| subtitle.language.as_str())
            .collect::<Vec<_>>();

        assert_eq!(languages, vec!["zh-CN"]);
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
    fn download_content_key_distinguishes_subtitle_ai_policy() {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let default_key = download_plan_content_key_for_options(&plan, &DownloadOptions::default());
        let exclude_ai_key = download_plan_content_key_for_options(
            &plan,
            &DownloadOptions::default().with_subtitle_ai_policy(SubtitleAiPolicy::ExcludeAi),
        );
        let prefer_non_ai_key = download_plan_content_key_for_options(
            &plan,
            &DownloadOptions::default().with_subtitle_ai_policy(SubtitleAiPolicy::PreferNonAi),
        );

        assert_ne!(default_key, exclude_ai_key);
        assert!(exclude_ai_key.contains("subtitle_ai=exclude-ai"));
        assert!(prefer_non_ai_key.contains("subtitle_ai=prefer-non-ai"));
    }

    #[test]
    fn download_content_key_distinguishes_explicit_audio_language_selection() {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let default_key = download_plan_content_key_for_options(&plan, &DownloadOptions::default());
        let language_options = DownloadOptions::default()
            .with_stream_selection(StreamSelection::audio_language("ja-JP"));
        let language_key = download_plan_content_key_for_options(&plan, &language_options);
        let non_ascii_language_options = DownloadOptions::default()
            .with_stream_selection(StreamSelection::audio_language("日本語"));
        let non_ascii_language_key =
            download_plan_content_key_for_options(&plan, &non_ascii_language_options);
        let hyphenated_language_key = download_plan_content_key_for_options(
            &plan,
            &DownloadOptions::default()
                .with_stream_selection(StreamSelection::audio_language("en-US")),
        );
        let spaced_language_key = download_plan_content_key_for_options(
            &plan,
            &DownloadOptions::default()
                .with_stream_selection(StreamSelection::audio_language("en US")),
        );
        let uppercase_language_key = download_plan_content_key_for_options(
            &plan,
            &DownloadOptions::default()
                .with_stream_selection(StreamSelection::audio_language("English")),
        );
        let lowercase_language_key = download_plan_content_key_for_options(
            &plan,
            &DownloadOptions::default()
                .with_stream_selection(StreamSelection::audio_language("english")),
        );

        assert_eq!(default_key, "plan|aid=170001;cid=2");
        assert_eq!(
            language_key,
            format!(
                "mode=all;stream=alangja-jp-{};plan|aid=170001;cid=2",
                super::short_identity_hash("ja-jp")
            )
        );
        assert_ne!(non_ascii_language_key, language_key);
        assert!(non_ascii_language_key.starts_with("mode=all;stream=alangh"));
        assert_ne!(hyphenated_language_key, spaced_language_key);
        assert_eq!(uppercase_language_key, lowercase_language_key);
    }

    #[test]
    fn danmaku_archive_key_refresh_preserves_stream_selection_prefix() -> anyhow::Result<()> {
        let stream_token = format!(
            "stream=alang{}",
            super::archive_stream_selection_text_token("ja-JP")
        );
        let stream_key = format!("mode=all;{stream_token};plan|aid=170001;cid=2");
        let ass_stream_key = format!("mode=all;danmaku=ass;{stream_token};plan|aid=170001;cid=2");

        let refreshed = super::refresh_danmaku_archive_content_key(
            &stream_key,
            &DanmakuFormats::new([DanmakuFormat::Xml, DanmakuFormat::Ass]),
        )
        .ok_or_else(|| anyhow::anyhow!("stream key did not refresh"))?;
        assert_eq!(
            refreshed,
            format!("mode=all;danmaku=xml+ass;{stream_token};plan|aid=170001;cid=2")
        );

        let refreshed_default =
            super::refresh_danmaku_archive_content_key(&ass_stream_key, &DanmakuFormats::default())
                .ok_or_else(|| anyhow::anyhow!("stream key did not refresh to default"))?;
        assert_eq!(refreshed_default, stream_key);
        Ok(())
    }

    #[test]
    fn danmaku_archive_key_refresh_preserves_subtitle_ai_token_order() -> anyhow::Result<()> {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let subtitle_options =
            DownloadOptions::default().with_subtitle_ai_policy(SubtitleAiPolicy::ExcludeAi);
        let subtitle_ass_options = subtitle_options
            .clone()
            .with_danmaku_formats([DanmakuFormat::Xml, DanmakuFormat::Ass]);
        let subtitle_key = download_plan_content_key_for_options(&plan, &subtitle_options);
        let subtitle_ass_key = download_plan_content_key_for_options(&plan, &subtitle_ass_options);

        let refreshed = super::refresh_danmaku_archive_content_key(
            &subtitle_key,
            &DanmakuFormats::new([DanmakuFormat::Xml, DanmakuFormat::Ass]),
        )
        .ok_or_else(|| anyhow::anyhow!("subtitle AI key did not refresh"))?;
        assert_eq!(refreshed, subtitle_ass_key);

        let refreshed_default = super::refresh_danmaku_archive_content_key(
            &subtitle_ass_key,
            &DanmakuFormats::default(),
        )
        .ok_or_else(|| anyhow::anyhow!("subtitle AI key did not refresh to default"))?;
        assert_eq!(refreshed_default, subtitle_key);
        Ok(())
    }

    #[test]
    fn temporary_file_names_reserve_suffix_budget() {
        let path = std::path::PathBuf::from("a".repeat(MAX_FILE_COMPONENT_BYTES));

        for temporary in [
            temporary_download_path(&path),
            temporary_replace_path(&path),
            temporary_generated_path(&path),
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

    #[tokio::test]
    async fn write_generated_text_file_replaces_existing_target() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("danmaku.xml");
        tokio::fs::write(&path, "old").await?;

        let file = write_generated_text_file(&path, "new", DownloadFileKind::Danmaku).await?;

        assert_ne!(
            temporary_generated_path(&path),
            temporary_replace_path(&path)
        );
        assert_eq!(file.path, path);
        assert_eq!(file.bytes_written, 3);
        assert_eq!(tokio::fs::read_to_string(&path).await?, "new");
        assert!(!temporary_generated_path(&path).exists());
        assert!(!temporary_replace_path(&path).exists());
        Ok(())
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
    async fn download_plan_reports_progress_events() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("video");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].streams.videos[0].size = Some(5);
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let report = client
            .download_plan_with_progress(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    mode: DownloadMode::VideoOnly,
                    sidecars: SidecarOptions {
                        subtitles: false,
                        danmaku: false,
                        ..SidecarOptions::default()
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
                &progress,
            )
            .await?;

        assert_eq!(report.entries[0].files.len(), 1);
        let events = progress_events_snapshot(&events);
        assert!(matches!(
            events.first(),
            Some(DownloadProgressEvent::PlanStarted {
                title,
                entry_count: 1,
                ..
            }) if title == "Mock video"
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::EntryStarted {
                index: 1,
                title,
                ..
            } if title == "Main"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileStarted {
                entry_index: 1,
                kind: DownloadFileKind::Video,
                resumed_from: 0,
                expected_size: Some(5),
                attempt: 1,
                max_attempts: 1,
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileProgress {
                entry_index: 1,
                kind: DownloadFileKind::Video,
                bytes_delta: 5,
                bytes_written: 5,
                resumed_from: 0,
                expected_size: Some(5),
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileCompleted {
                entry_index: 1,
                kind: DownloadFileKind::Video,
                bytes_written: 5,
                resumed_from: 0,
                total_bytes: 5,
                ..
            }
        )));
        assert!(matches!(
            events.last(),
            Some(DownloadProgressEvent::PlanCompleted {
                title,
                entry_count: 1,
                ..
            }) if title == "Mock video"
        ));
        Ok(())
    }

    #[test]
    fn download_report_summary_counts_files_mux_and_bytes() {
        let report = DownloadReport {
            title: "Summary".to_owned(),
            output_dir: PathBuf::from("downloads"),
            entries: vec![
                EntryDownloadReport {
                    index: 1,
                    title: "First".to_owned(),
                    directory: PathBuf::from("downloads/first"),
                    files: vec![
                        DownloadedFile {
                            kind: DownloadFileKind::Video,
                            path: PathBuf::from("downloads/first/video.m4s"),
                            bytes_written: 10,
                            resumed_from: 5,
                        },
                        DownloadedFile {
                            kind: DownloadFileKind::Danmaku,
                            path: PathBuf::from("downloads/first/danmaku.xml"),
                            bytes_written: 3,
                            resumed_from: 0,
                        },
                    ],
                    mux: Some(MuxReport {
                        output_path: PathBuf::from("downloads/first/first.mp4"),
                        command: vec!["ffmpeg".to_owned()],
                        chapter_count: 0,
                    }),
                },
                EntryDownloadReport {
                    index: 2,
                    title: "Second".to_owned(),
                    directory: PathBuf::from("downloads/second"),
                    files: vec![DownloadedFile {
                        kind: DownloadFileKind::Audio,
                        path: PathBuf::from("downloads/second/audio.m4s"),
                        bytes_written: 7,
                        resumed_from: 1,
                    }],
                    mux: None,
                },
            ],
        };

        assert_eq!(
            report.summary(),
            DownloadReportSummary {
                entry_count: 2,
                file_count: 3,
                media_file_count: 2,
                sidecar_file_count: 1,
                mux_count: 1,
                bytes_written: 20,
                resumed_bytes: 6,
                total_bytes: 26,
            }
        );
        assert_eq!(
            report.entries[0].summary(),
            EntryDownloadSummary {
                file_count: 2,
                media_file_count: 1,
                sidecar_file_count: 1,
                has_mux: true,
                bytes_written: 13,
                resumed_bytes: 5,
                total_bytes: 18,
            }
        );
    }

    #[tokio::test]
    async fn download_plan_reports_failure_terminal_events() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(500).body("fail");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_progress(
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
                &progress,
            )
            .await
        else {
            return Err(anyhow::anyhow!("media failure should fail the download"));
        };

        assert!(error.to_string().contains("HTTP error"));
        let events = progress_events_snapshot(&events);
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileFailed {
                kind: DownloadFileKind::Video,
                attempt: 1,
                max_attempts: 1,
                error,
                ..
            } if error.contains("HTTP error")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::EntryFailed {
                index: 1,
                error,
                ..
            } if error.contains("HTTP error")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::PlanFailed {
                completed_entries: 0,
                error,
                ..
            } if error.contains("HTTP error")
        )));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::PlanCompleted { .. }))
        );
        Ok(())
    }

    #[tokio::test]
    async fn download_plan_reports_setup_failure_terminal_event() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_progress(
                &plan,
                DownloadOptions::new(temp.path()).with_output_template("{unknown}"),
                &progress,
            )
            .await
        else {
            return Err(anyhow::anyhow!("invalid output template should fail"));
        };

        assert!(
            error
                .to_string()
                .contains("unknown download path template placeholder")
        );
        assert_eq!(
            progress_events_snapshot(&events),
            vec![DownloadProgressEvent::PlanFailed {
                title: plan.title.clone(),
                output_dir: temp.path().to_path_buf(),
                completed_entries: 0,
                error: error.to_string(),
            }]
        );
        Ok(())
    }

    #[tokio::test]
    async fn download_plan_reports_entry_setup_failure_terminal_event() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let entry_dir = test_entry_dir(temp.path(), &plan)?;
        if let Some(parent) = entry_dir.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&entry_dir, b"not a directory").await?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_progress(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
                &progress,
            )
            .await
        else {
            return Err(anyhow::anyhow!("entry setup conflict should fail"));
        };

        let events = progress_events_snapshot(&events);
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::EntryFailed {
                index: 1,
                directory,
                error: event_error,
                ..
            } if directory == &entry_dir && event_error == &error.to_string()
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::PlanFailed {
                completed_entries: 0,
                error: event_error,
                ..
            } if event_error == &error.to_string()
        )));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::EntryStarted { .. }))
        );
        Ok(())
    }

    #[tokio::test]
    async fn download_input_reports_parse_failure_terminal_event() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_input_with_progress(
                "not-a-valid-input",
                None,
                DownloadOptions::new(temp.path()),
                &progress,
            )
            .await
        else {
            return Err(anyhow::anyhow!("invalid raw input should fail"));
        };

        assert_eq!(
            progress_events_snapshot(&events),
            vec![DownloadProgressEvent::PlanFailed {
                title: "not-a-valid-input".to_owned(),
                output_dir: temp.path().to_path_buf(),
                completed_entries: 0,
                error: error.to_string(),
            }]
        );
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_download_plan_reports_plan_cancelled_before_start() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let options = DownloadOptions::new(temp.path()).with_mux(MuxOptions::Disabled);
        let output_dir = default_plan_output_dir(&plan, &options)?;
        let cancellation = DownloadCancellationToken::new();
        cancellation.cancel_with_reason("test cancellation before start");
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_progress_and_cancellation(&plan, options, &progress, &cancellation)
            .await
        else {
            return Err(anyhow::anyhow!("pre-cancelled download should fail"));
        };

        assert!(error.is_cancelled());
        assert_eq!(
            progress_events_snapshot(&events),
            vec![DownloadProgressEvent::PlanCancelled {
                title: plan.title.clone(),
                output_dir,
                completed_entries: 0,
                error: error.to_string(),
            }]
        );
        assert!(!temp.path().join("Mock video").exists());
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_started_file_rolls_back_and_reports_cancelled() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let media_url = format!("http://{}/video.m4s", listener.local_addr()?);
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n");
                std::thread::sleep(Duration::from_millis(200));
                let _ = stream.write_all(b"video");
            }
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(media_url);
        let cancellation = DownloadCancellationToken::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress_cancellation = cancellation.clone();
        let progress = move |event: &DownloadProgressEvent| {
            if matches!(event, DownloadProgressEvent::FileStarted { .. }) {
                progress_cancellation.cancel_with_reason("test cancellation after file start");
            }
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_progress_and_cancellation(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    mode: DownloadMode::VideoOnly,
                    sidecars: SidecarOptions {
                        danmaku: false,
                        cover: false,
                        subtitles: false,
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
                &progress,
                &cancellation,
            )
            .await
        else {
            return Err(anyhow::anyhow!("cancelled file download should fail"));
        };

        assert!(error.is_cancelled());
        let events = progress_events_snapshot(&events);
        let started_path = events.iter().find_map(|event| match event {
            DownloadProgressEvent::FileStarted { path, .. } => Some(path.clone()),
            _ => None,
        });
        let Some(started_path) = started_path else {
            return Err(anyhow::anyhow!("file start event should be emitted"));
        };
        assert!(!started_path.exists());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileFailed {
                kind: DownloadFileKind::Video,
                error,
                ..
            } if error.contains("test cancellation after file start")
        )));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::EntryFailed { .. }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::PlanCancelled {
                completed_entries: 0,
                error,
                ..
            } if error.contains("test cancellation after file start")
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileCompleted { .. }
                | DownloadProgressEvent::EntryCompleted { .. }
                | DownloadProgressEvent::PlanCompleted { .. }
        )));
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_started_file_does_not_retry_or_duplicate_file_failed() -> anyhow::Result<()>
    {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let media_url = format!("http://{}/video.m4s", listener.local_addr()?);
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n");
                std::thread::sleep(Duration::from_millis(200));
                let _ = stream.write_all(b"video");
            }
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(media_url);
        let cancellation = DownloadCancellationToken::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress_cancellation = cancellation.clone();
        let progress = move |event: &DownloadProgressEvent| {
            if matches!(event, DownloadProgressEvent::FileStarted { .. }) {
                progress_cancellation.cancel_with_reason("test cancellation with default retry");
            }
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_progress_and_cancellation(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    mode: DownloadMode::VideoOnly,
                    sidecars: SidecarOptions {
                        danmaku: false,
                        cover: false,
                        subtitles: false,
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
                &progress,
                &cancellation,
            )
            .await
        else {
            return Err(anyhow::anyhow!(
                "cancelled default-retry download should fail"
            ));
        };

        assert!(error.is_cancelled());
        let events = progress_events_snapshot(&events);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, DownloadProgressEvent::FileStarted { .. }))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    DownloadProgressEvent::FileFailed {
                        kind: DownloadFileKind::Video,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::PlanCancelled {
                error,
                ..
            } if error.contains("test cancellation with default retry")
        )));
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_retry_backoff_does_not_duplicate_file_failed() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let media_url = format!("http://{}/video.m4s", listener.local_addr()?);
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nbad");
            }
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(media_url);
        let cancellation = DownloadCancellationToken::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress_cancellation = cancellation.clone();
        let progress = move |event: &DownloadProgressEvent| {
            if matches!(
                event,
                DownloadProgressEvent::FileFailed {
                    kind: DownloadFileKind::Video,
                    attempt: 1,
                    ..
                }
            ) {
                progress_cancellation.cancel_with_reason("test cancellation during retry backoff");
            }
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_progress_and_cancellation(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy {
                        max_attempts: 2,
                        backoff: Duration::from_millis(200),
                    },
                    mode: DownloadMode::VideoOnly,
                    sidecars: SidecarOptions {
                        danmaku: false,
                        cover: false,
                        subtitles: false,
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
                &progress,
                &cancellation,
            )
            .await
        else {
            return Err(anyhow::anyhow!(
                "cancelled retry-backoff download should fail"
            ));
        };

        assert!(error.is_cancelled());
        let events = progress_events_snapshot(&events);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    DownloadProgressEvent::FileFailed {
                        kind: DownloadFileKind::Video,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::PlanCancelled {
                error,
                ..
            } if error.contains("test cancellation during retry backoff")
        )));
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_single_url_retry_backoff_does_not_duplicate_file_failed()
    -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let cover_url = format!("http://{}/cover.jpg", listener.local_addr()?);
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            }
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(cover_url.clone());
        let path = temp.path().join("cover.jpg");
        let request =
            DownloadFileRequest::new(&plan.entries[0], &path, DownloadFileKind::Cover, None);
        let cancellation = DownloadCancellationToken::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress_cancellation = cancellation.clone();
        let progress = move |event: &DownloadProgressEvent| {
            if matches!(
                event,
                DownloadProgressEvent::FileFailed {
                    kind: DownloadFileKind::Cover,
                    attempt: 1,
                    ..
                }
            ) {
                progress_cancellation
                    .cancel_with_reason("test sidecar cancellation during retry backoff");
            }
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_url_to_file(
                &cover_url,
                &request,
                &DownloadOptions {
                    retry: RetryPolicy {
                        max_attempts: 2,
                        backoff: Duration::from_millis(200),
                    },
                    ..DownloadOptions::default()
                },
                &progress,
                &cancellation,
            )
            .await
        else {
            return Err(anyhow::anyhow!(
                "cancelled single-url retry-backoff download should fail"
            ));
        };

        assert!(error.is_cancelled());
        let events = progress_events_snapshot(&events);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    DownloadProgressEvent::FileFailed {
                        kind: DownloadFileKind::Cover,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert!(!path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn cancellation_after_entry_completion_reports_completed_entries() -> anyhow::Result<()> {
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
        let mut plan = test_plan(&server);
        let mut second_entry = plan.entries[0].clone();
        second_entry.index = 2;
        second_entry.cid = 3;
        second_entry.title = "Second".to_owned();
        plan.entries.push(second_entry);
        let cancellation = DownloadCancellationToken::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress_cancellation = cancellation.clone();
        let progress = move |event: &DownloadProgressEvent| {
            if matches!(
                event,
                DownloadProgressEvent::EntryCompleted { index: 1, .. }
            ) {
                progress_cancellation.cancel_with_reason("test cancellation after first entry");
            }
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_progress_and_cancellation(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    sidecars: SidecarOptions {
                        danmaku: false,
                        cover: false,
                        subtitles: false,
                    },
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
                &progress,
                &cancellation,
            )
            .await
        else {
            return Err(anyhow::anyhow!(
                "cancelled multi-entry download should fail"
            ));
        };

        assert!(error.is_cancelled());
        let events = progress_events_snapshot(&events);
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::EntryCompleted { index: 1, .. }
        )));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::EntryStarted { index: 2, .. }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::PlanCancelled {
                completed_entries: 1,
                error,
                ..
            } if error.contains("test cancellation after first entry")
        )));
        Ok(())
    }

    #[tokio::test]
    async fn archive_cancellation_after_plan_completed_keeps_archive_record() -> anyhow::Result<()>
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
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let options = DownloadOptions {
            output_dir: temp.path().to_path_buf(),
            retry: RetryPolicy::single_attempt(),
            sidecars: SidecarOptions {
                danmaku: false,
                cover: false,
                subtitles: false,
            },
            mux: MuxOptions::Disabled,
            ..DownloadOptions::default()
        };
        let cancellation = DownloadCancellationToken::new();
        let mut archive = DownloadArchive::default();
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress_cancellation = cancellation.clone();
        let progress = move |event: &DownloadProgressEvent| {
            if matches!(event, DownloadProgressEvent::PlanCompleted { .. }) {
                progress_cancellation.cancel_with_reason("late cancellation after completion");
            }
            push_progress_event(&progress_events, event.clone());
        };

        let report = client
            .download_plan_with_archive_decision_with_progress_and_cancellation(
                &plan,
                options,
                &mut archive,
                DuplicateDecision::Replace,
                &progress,
                &cancellation,
            )
            .await?;

        assert!(cancellation.is_cancelled());
        assert_eq!(report.entries.len(), 1);
        assert_eq!(archive.records.len(), 1);
        assert_eq!(archive.records[0].entries.len(), 1);
        let events = progress_events_snapshot(&events);
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::PlanCompleted { entry_count: 1, .. }
        )));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::PlanCancelled { .. }))
        );
        Ok(())
    }

    #[tokio::test]
    async fn duplicate_cancel_reports_plan_cancelled_progress() -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let options = DownloadOptions {
            output_dir: temp.path().to_path_buf(),
            retry: RetryPolicy::single_attempt(),
            sidecars: SidecarOptions {
                danmaku: false,
                ..SidecarOptions::default()
            },
            mux: MuxOptions::Disabled,
            ..DownloadOptions::default()
        };
        let mut archive = DownloadArchive::default();
        let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;
        std::fs::create_dir_all(&preflight.planned_output_dir)?;
        let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;
        assert!(preflight.requires_decision());
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_archive_preflight_decision_with_progress(
                &plan,
                options,
                &mut archive,
                &preflight,
                DuplicateDecision::Cancel,
                &progress,
            )
            .await
        else {
            return Err(anyhow::anyhow!("duplicate cancel should fail the download"));
        };

        assert!(error.to_string().contains("download canceled"));
        assert_eq!(
            progress_events_snapshot(&events),
            vec![DownloadProgressEvent::PlanCancelled {
                title: plan.title.clone(),
                output_dir: preflight.planned_output_dir.clone(),
                completed_entries: 0,
                error: "archive or output conflict requires a decision".to_owned(),
            }]
        );
        Ok(())
    }

    #[tokio::test]
    async fn ass_only_danmaku_progress_excludes_internal_source_file() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200)
                .body(r#"<i><d p="1,1,25,16777215,1,0,0,0">hello</d></i>"#);
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let report = client
            .download_plan_with_progress(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    mode: DownloadMode::DanmakuOnly,
                    danmaku_formats: DanmakuFormats::new([DanmakuFormat::Ass]),
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
                &progress,
            )
            .await?;

        assert_eq!(report.entries[0].files.len(), 1);
        assert_eq!(
            report.entries[0].files[0].kind,
            DownloadFileKind::DanmakuAss
        );
        let events = progress_events_snapshot(&events);
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileCompleted {
                kind: DownloadFileKind::DanmakuAss,
                ..
            }
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileCompleted {
                kind: DownloadFileKind::Danmaku,
                ..
            }
        )));
        assert!(!events.iter().any(|event| match event {
            DownloadProgressEvent::FileStarted { path, .. }
            | DownloadProgressEvent::FileProgress { path, .. }
            | DownloadProgressEvent::FileCompleted { path, .. } =>
                path.to_string_lossy().contains(".bbdown-source"),
            _ => false,
        }));
        Ok(())
    }

    #[tokio::test]
    async fn generated_danmaku_failure_reports_file_failed_progress() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200)
                .body(r#"<i><d p="1,1,25,16777215,1,0,0,0">hello</d></i>"#);
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = test_plan(&server);
        let entry_dir = test_entry_dir(temp.path(), &plan)?;
        tokio::fs::create_dir_all(temporary_generated_path(&entry_dir.join("danmaku.ass"))).await?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let Err(error) = client
            .download_plan_with_progress(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    mode: DownloadMode::DanmakuOnly,
                    danmaku_formats: DanmakuFormats::new([DanmakuFormat::Ass]),
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
                &progress,
            )
            .await
        else {
            return Err(anyhow::anyhow!("generated ASS sidecar write should fail"));
        };

        let events = progress_events_snapshot(&events);
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileStarted {
                kind: DownloadFileKind::DanmakuAss,
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileFailed {
                kind: DownloadFileKind::DanmakuAss,
                error: event_error,
                ..
            } if event_error == &error.to_string()
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileCompleted {
                kind: DownloadFileKind::DanmakuAss,
                ..
            }
        )));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::EntryFailed { .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::PlanFailed { .. }))
        );
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
    async fn download_danmaku_only_can_generate_ass_sidecar() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200)
                .body(r#"<i><d p="1.25,1,25,16711680,0,0,0,0">hello &amp; world</d></i>"#);
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].streams.videos.clear();
        plan.entries[0].streams.audios.clear();

        let report = client
            .download_plan(
                &plan,
                DownloadOptions::new(temp.path())
                    .with_retry_policy(RetryPolicy::single_attempt())
                    .with_download_mode(DownloadMode::DanmakuOnly)
                    .with_danmaku_format(DanmakuFormat::Ass),
            )
            .await?;

        let entry = &report.entries[0];
        assert_eq!(entry.files.len(), 1);
        assert_eq!(entry.files[0].kind, DownloadFileKind::DanmakuAss);
        assert_eq!(
            entry.files[0]
                .path
                .file_name()
                .and_then(std::ffi::OsStr::to_str),
            Some("danmaku.ass")
        );
        let ass = tokio::fs::read_to_string(&entry.files[0].path).await?;
        assert!(ass.contains("[Script Info]"));
        assert!(ass.contains("Dialogue:"));
        assert!(ass.contains("hello & world"));
        assert!(
            !test_entry_dir(temp.path(), &plan)?
                .join("danmaku.xml")
                .exists()
        );
        Ok(())
    }

    #[tokio::test]
    async fn download_danmaku_multiple_formats_writes_xml_and_ass_sidecars() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200)
                .body(r#"<i><d p="0,5,25,255,0,0,0,0">top</d></i>"#);
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].streams.videos.clear();
        plan.entries[0].streams.audios.clear();

        let report = client
            .download_plan(
                &plan,
                DownloadOptions::new(temp.path())
                    .with_retry_policy(RetryPolicy::single_attempt())
                    .with_download_mode(DownloadMode::DanmakuOnly)
                    .with_danmaku_formats([DanmakuFormat::Xml, DanmakuFormat::Ass]),
            )
            .await?;

        let entry = &report.entries[0];
        assert_eq!(entry.files.len(), 2);
        let xml = entry
            .files
            .iter()
            .find(|file| file.kind == DownloadFileKind::Danmaku)
            .ok_or_else(|| anyhow::anyhow!("missing XML danmaku"))?;
        let ass = entry
            .files
            .iter()
            .find(|file| file.kind == DownloadFileKind::DanmakuAss)
            .ok_or_else(|| anyhow::anyhow!("missing ASS danmaku"))?;
        assert_eq!(
            tokio::fs::read_to_string(&xml.path).await?,
            r#"<i><d p="0,5,25,255,0,0,0,0">top</d></i>"#
        );
        assert!(
            tokio::fs::read_to_string(&ass.path)
                .await?
                .contains("\\an8")
        );
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
            .join(entry_dir_name(
                &plan.title,
                &plan.entries[0],
                &DownloadOptions::default(),
            )?)
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
                        audio_language: None,
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
                        audio_language: None,
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
            is_ai_generated: false,
            ai_type: None,
            ai_status: None,
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
    async fn subtitle_ai_policy_prefers_non_ai_tracks() -> anyhow::Result<()> {
        let server = MockServer::start();
        let english_mock = server.mock(|when, then| {
            when.method(GET).path("/subtitle.ass");
            then.status(200).body("manual-en");
        });
        let ai_english_mock = server.mock(|when, then| {
            when.method(GET).path("/ai-en.ass");
            then.status(200).body("ai-en");
        });
        let ai_japanese_mock = server.mock(|when, then| {
            when.method(GET).path("/ai-ja.ass");
            then.status(200).body("ai-ja");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].subtitles.push(SubtitleTrack {
            language: "ai-en".to_owned(),
            language_doc: Some("English (AI)".to_owned()),
            is_ai_generated: true,
            ai_type: Some(1),
            ai_status: Some(2),
            url: format!("{}/ai-en.ass", server.base_url()),
            format: SubtitleFormat::Ass,
        });
        plan.entries[0].subtitles.push(SubtitleTrack {
            language: "ai-ja".to_owned(),
            language_doc: Some("Japanese (AI)".to_owned()),
            is_ai_generated: false,
            ai_type: Some(1),
            ai_status: Some(2),
            url: format!("{}/ai-ja.ass", server.base_url()),
            format: SubtitleFormat::Ass,
        });

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    mode: DownloadMode::SubtitleOnly,
                    subtitle_ai_policy: SubtitleAiPolicy::PreferNonAi,
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        assert_eq!(english_mock.calls(), 1);
        assert_eq!(ai_english_mock.calls(), 0);
        assert_eq!(ai_japanese_mock.calls(), 1);
        let subtitle_files = report.entries[0]
            .files
            .iter()
            .filter(|file| file.kind == DownloadFileKind::Subtitle)
            .collect::<Vec<_>>();
        assert_eq!(subtitle_files.len(), 2);
        let mut bodies = Vec::new();
        for file in subtitle_files {
            bodies.push(tokio::fs::read_to_string(&file.path).await?);
        }
        bodies.sort();
        assert_eq!(bodies, vec!["ai-ja", "manual-en"]);
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
    async fn started_primary_candidate_failure_is_closed_before_backup() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/primary.m4s");
            then.status(200).body("bad");
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
        plan.entries[0].streams.videos[0].size = Some(6);
        plan.entries[0].streams.videos[0]
            .backup_urls
            .push(format!("{}/backup.m4s", server.base_url()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let report = client
            .download_plan_with_progress(
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
                &progress,
            )
            .await?;

        assert_eq!(
            tokio::fs::read_to_string(&report.entries[0].files[0].path).await?,
            "backup"
        );
        let events = progress_events_snapshot(&events);
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileFailed {
                kind: DownloadFileKind::Video,
                attempt: 1,
                max_attempts: 1,
                error,
                ..
            } if error.contains("downloaded file length did not match expected media size")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileCompleted {
                kind: DownloadFileKind::Video,
                total_bytes: 6,
                ..
            }
        )));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::PlanCompleted { .. }))
        );
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
        let events = Arc::new(Mutex::new(Vec::new()));
        let progress_events = Arc::clone(&events);
        let progress = move |event: &DownloadProgressEvent| {
            push_progress_event(&progress_events, event.clone());
        };

        let report = client
            .download_plan_with_progress(
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
                &progress,
            )
            .await?;

        assert_eq!(
            tokio::fs::read_to_string(&report.entries[0].files[0].path).await?,
            "backup"
        );
        let events = progress_events_snapshot(&events);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::FileFailed { .. }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadProgressEvent::FileCompleted {
                kind: DownloadFileKind::Video,
                ..
            }
        )));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DownloadProgressEvent::PlanCompleted { .. }))
        );
        Ok(())
    }

    #[tokio::test]
    async fn download_media_rewrites_custom_upos_host() -> anyhow::Result<()> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/video.m4s");
            then.status(200).body("rewritten-video");
        });
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan("http://pcdn.example:12000/video.m4s".to_owned());

        let report = client
            .download_plan(
                &plan,
                DownloadOptions::new(temp.path())
                    .with_retry_policy(RetryPolicy::single_attempt())
                    .with_download_mode(DownloadMode::VideoOnly)
                    .with_media_hosts(
                        MediaHostOptions::new().with_upos_host(server_authority(&server)?),
                    )
                    .with_mux(MuxOptions::Disabled),
            )
            .await?;

        assert_eq!(
            tokio::fs::read_to_string(&report.entries[0].files[0].path).await?,
            "rewritten-video"
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let entry = single_video_plan(format!("http://{address}/video.m4s"))
            .entries
            .remove(0);
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
        let request = DownloadFileRequest::new(&entry, &path, DownloadFileKind::Video, None);
        let cancellation = DownloadCancellationToken::new();

        let Err(error) = client
            .download_url_to_file(
                &format!("http://{address}/video.m4s"),
                &request,
                &options,
                &NoopDownloadProgress,
                &cancellation,
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
        let output_dir = test_entry_dir(temp.path(), &plan)?;
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
            .join(entry_dir_name(
                &plan.title,
                &plan.entries[0],
                &DownloadOptions::default(),
            )?)
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
            .join(entry_dir_name(
                &plan.title,
                &plan.entries[0],
                &DownloadOptions::default(),
            )?)
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
        let entry = single_video_plan(format!("{}/video.m4s", server.base_url()))
            .entries
            .remove(0);
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
        let request = DownloadFileRequest::new(&entry, &path, DownloadFileKind::Video, Some(0));
        let cancellation = DownloadCancellationToken::new();

        let Err(error) = client
            .download_url_to_file(
                &format!("{}/video.m4s", server.base_url()),
                &request,
                &options,
                &NoopDownloadProgress,
                &cancellation,
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
        let entry = single_video_plan(format!("http://{address}/video.m4s"))
            .entries
            .remove(0);
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
        let request = DownloadFileRequest::new(&entry, &path, DownloadFileKind::Video, Some(2));
        let cancellation = DownloadCancellationToken::new();

        let file = client
            .download_url_to_file(
                &format!("http://{address}/video.m4s"),
                &request,
                &options,
                &NoopDownloadProgress,
                &cancellation,
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
        let entry = single_video_plan(format!("http://{address}/video.m4s"))
            .entries
            .remove(0);
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
        let request = DownloadFileRequest::new(&entry, &path, DownloadFileKind::Video, None);
        let cancellation = DownloadCancellationToken::new();

        let Err(error) = client
            .download_url_to_file(
                &format!("http://{address}/video.m4s"),
                &request,
                &options,
                &NoopDownloadProgress,
                &cancellation,
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
        let entry = single_video_plan(format!("http://{address}/video.m4s"))
            .entries
            .remove(0);
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
        let request = DownloadFileRequest::new(&entry, &path, DownloadFileKind::Video, None);
        let cancellation = DownloadCancellationToken::new();

        let file = client
            .download_url_to_file(
                &format!("http://{address}/video.m4s"),
                &request,
                &options,
                &NoopDownloadProgress,
                &cancellation,
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
        let entry_dir = test_entry_dir(&output_dir, &plan)?;
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
    async fn ffmpeg_mux_includes_chapter_metadata() -> anyhow::Result<()> {
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
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].chapters = vec![
            ChapterTrack {
                title: "Opening".to_owned(),
                start_seconds: 0,
                end_seconds: 90,
            },
            ChapterTrack {
                title: "Main".to_owned(),
                start_seconds: 90,
                end_seconds: 180,
            },
        ];
        let output_dir = temp.path().join("downloads");
        let entry_dir = test_entry_dir(&output_dir, &plan)?;

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
        assert_eq!(mux.chapter_count, 2);
        assert!(mux.command.iter().any(|arg| arg == "-map_chapters"));
        assert!(mux.command.iter().any(|arg| arg == "2"));
        assert!(
            !entry_dir
                .join("ffmpeg-chapters.ffmetadata.bbdown-tmp")
                .exists()
        );
        Ok(())
    }

    #[test]
    fn ffmpeg_chapter_metadata_escapes_values() {
        let metadata = super::ffmpeg_chapter_metadata(&[
            ChapterTrack {
                title: "Intro = #1; back\\slash".to_owned(),
                start_seconds: 1,
                end_seconds: 2,
            },
            ChapterTrack {
                title: "invalid".to_owned(),
                start_seconds: 2,
                end_seconds: 2,
            },
        ]);

        assert_eq!(metadata.chapter_count, 1);
        assert!(metadata.body.starts_with(";FFMETADATA1\n"));
        assert!(metadata.body.contains("START=1000\n"));
        assert!(metadata.body.contains("END=2000\n"));
        assert!(
            metadata
                .body
                .contains("title=Intro \\= \\#1\\; back\\\\slash")
        );
    }

    #[tokio::test]
    async fn ffmpeg_mux_cancel_after_process_removes_temporary_output() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let output_path = temp.path().join("Main.mp4");
        let mux_output_path = temporary_mux_path(&output_path);
        tokio::fs::write(&mux_output_path, "muxed").await?;
        let cancellation = DownloadCancellationToken::new();
        cancellation.cancel_with_reason("cancel after mux process");

        let Err(error) = remove_mux_output_if_cancelled(&cancellation, &mux_output_path).await
        else {
            return Err(anyhow::anyhow!("cancelled mux output should fail"));
        };

        assert!(error.is_cancelled());
        assert!(!mux_output_path.exists());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_mux_sigint_status_maps_to_delayed_cancellation() -> anyhow::Result<()> {
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg("kill -INT $$")
            .status()?;
        let cancellation = DownloadCancellationToken::new();
        let delayed_cancellation = cancellation.clone();
        let cancel_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            delayed_cancellation.cancel_with_reason("test mux SIGINT cancellation");
        });

        let error = super::mux_error_for_failed_status(status, &cancellation).await;
        cancel_task.await?;

        assert!(error.is_cancelled());
        assert!(error.to_string().contains("test mux SIGINT cancellation"));
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_mux_sigint_exit_code_maps_to_delayed_cancellation() -> anyhow::Result<()> {
        use std::os::unix::process::ExitStatusExt as _;

        let status = std::process::ExitStatus::from_raw(255 << 8);
        let cancellation = DownloadCancellationToken::new();
        let delayed_cancellation = cancellation.clone();
        let cancel_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            delayed_cancellation.cancel_with_reason("test mux SIGINT exit-code cancellation");
        });

        let error = super::mux_error_for_failed_status(status, &cancellation).await;
        cancel_task.await?;

        assert!(error.is_cancelled());
        assert!(
            error
                .to_string()
                .contains("test mux SIGINT exit-code cancellation")
        );
        Ok(())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn ffmpeg_mux_windows_ctrl_c_status_maps_to_delayed_cancellation() -> anyhow::Result<()> {
        use std::os::windows::process::ExitStatusExt as _;

        const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;
        let status = std::process::ExitStatus::from_raw(STATUS_CONTROL_C_EXIT);
        let cancellation = DownloadCancellationToken::new();
        let delayed_cancellation = cancellation.clone();
        let cancel_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            delayed_cancellation.cancel_with_reason("test mux Ctrl-C cancellation");
        });

        let error = super::mux_error_for_failed_status(status, &cancellation).await;
        cancel_task.await?;

        assert!(error.is_cancelled());
        assert!(error.to_string().contains("test mux Ctrl-C cancellation"));
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
        let entry_dir = test_entry_dir(&temp.path().join("downloads"), &plan)?;
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
        let entry_dir = test_entry_dir(&output_dir, &plan)?;
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
            .join(entry_dir_name(
                &plan.title,
                &plan.entries[0],
                &DownloadOptions::default(),
            )?)
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
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
        std::fs::create_dir_all(default_plan_output_dir(&plan, &options)?)?;
        let mut archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: download_plan_content_key(&plan),
            title: plan.title.clone(),
            output_dir: default_plan_output_dir(&plan, &options)?,
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
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
        std::fs::create_dir_all(default_plan_output_dir(&plan, &options)?)?;
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
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
        let entry_dir = test_entry_dir(&options.output_dir, &plan)?;
        std::fs::create_dir_all(&entry_dir)?;
        let video_path =
            entry_dir.join(media_file_name("video", &plan.entries[0].streams.videos[0]));
        std::fs::write(&video_path, "partial")?;
        let stale_sidecar = entry_dir.join("danmaku.xml");
        std::fs::write(&stale_sidecar, "<old/>")?;
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
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
    async fn archive_decision_replace_validates_templates_before_removing_output()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        let options = DownloadOptions::new(temp.path().join("downloads"))
            .with_retry_policy(RetryPolicy::single_attempt())
            .with_entry_template("{entry_count}")
            .with_danmaku(false)
            .with_mux(MuxOptions::Disabled);
        let planned_output_dir = default_plan_output_dir(&plan, &options)?;
        let marker = planned_output_dir.join("marker.txt");
        std::fs::create_dir_all(&planned_output_dir)?;
        std::fs::write(&marker, "keep")?;
        let mut archive = DownloadArchive::default();

        let error = match client
            .download_plan_with_archive_decision(
                &plan,
                options,
                &mut archive,
                DuplicateDecision::Replace,
            )
            .await
        {
            Ok(report) => anyhow::bail!(
                "invalid template unexpectedly downloaded to {}",
                report.output_dir.display()
            ),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            crate::Error::InvalidInput(message)
                if message.contains("placeholder {entry_count} is only available for output templates")
        ));
        assert_eq!(std::fs::read_to_string(marker)?, "keep");
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
        let entry_dir = test_entry_dir(&options.output_dir, &plan)?;
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
                    chapter_count: 0,
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

    #[test]
    fn download_archive_records_can_be_queried_by_stream_selection_variant() -> anyhow::Result<()> {
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
                    kind: DownloadFileKind::Audio,
                    path: Path::new("downloads")
                        .join("Mock video")
                        .join("entry")
                        .join("audio-ja-JP.m4a"),
                    bytes_written: 5,
                    resumed_from: 0,
                }],
                mux: None,
            }],
        };
        let language_options = DownloadOptions::default()
            .with_stream_selection(StreamSelection::audio_language("ja-JP"));
        let mut archive = DownloadArchive::default();

        archive.record_download(&plan, &language_options, &report);

        assert_eq!(archive.records_for_plan(&plan).len(), 1);
        assert_eq!(
            archive
                .records_for_plan_with_mode(&plan, DownloadMode::All)
                .len(),
            1
        );
        assert!(
            archive
                .records_for_plan_with_mode(&plan, DownloadMode::AudioOnly)
                .is_empty()
        );
        let temp = tempfile::tempdir()?;
        let default_preflight =
            DownloadPreflight::inspect(&plan, &DownloadOptions::new(temp.path()), Some(&archive))?;
        assert!(default_preflight.archived_records.is_empty());
        let language_preflight = DownloadPreflight::inspect(
            &plan,
            &DownloadOptions::new(temp.path())
                .with_stream_selection(StreamSelection::audio_language("ja-JP")),
            Some(&archive),
        )?;
        assert_eq!(language_preflight.archived_records.len(), 1);

        Ok(())
    }

    #[test]
    fn download_archive_matches_audio_language_aliases_by_selected_stream() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mut plan = test_plan(&server);
        let audio = &mut plan.entries[0].streams.audios[0];
        audio.language = Some("en-US".to_owned());
        audio.language_doc = Some("English".to_owned());
        audio.codecs = Some("mp4a.40.2".to_owned());
        let output_dir = Path::new("downloads").join("Mock video");
        let audio_file = output_dir.join("entry").join("audio-en-US.m4a");
        let language_options = DownloadOptions::default()
            .with_stream_selection(StreamSelection::audio_language("en-US"));
        let doc_options = DownloadOptions::default()
            .with_stream_selection(StreamSelection::audio_language("English"));
        let language_key = download_plan_content_key_for_options(&plan, &language_options);
        let doc_key = download_plan_content_key_for_options(&plan, &doc_options);
        let language_entry_key =
            download_entry_content_key_for_options(&plan.entries[0], &language_options);
        let doc_entry_key = download_entry_content_key_for_options(&plan.entries[0], &doc_options);

        assert_eq!(language_key, doc_key);
        assert_eq!(language_entry_key, doc_entry_key);
        assert!(language_key.contains("stream=audio30280-"));

        let mut archive = DownloadArchive::default();
        archive.record_download(
            &plan,
            &language_options,
            &archive_audio_report(&plan, &output_dir, &audio_file),
        );
        let temp = tempfile::tempdir()?;
        let doc_preflight = DownloadPreflight::inspect(
            &plan,
            &DownloadOptions::new(temp.path())
                .with_stream_selection(StreamSelection::audio_language("English")),
            Some(&archive),
        )?;

        assert_eq!(doc_preflight.archived_records.len(), 1);
        Ok(())
    }

    #[test]
    fn download_archive_keeps_same_output_stream_variants_when_files_remain() -> anyhow::Result<()>
    {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let temp = tempfile::tempdir()?;
        let output_dir = temp.path().join("downloads").join("Mock video");
        let entry_dir = output_dir.join("entry");
        std::fs::create_dir_all(&entry_dir)?;
        let ja_file = entry_dir.join("audio-ja-JP.m4a");
        let en_file = entry_dir.join("audio-en-US.m4a");
        let fr_file = entry_dir.join("audio-fr-FR.m4a");
        std::fs::write(&ja_file, "ja")?;
        std::fs::write(&en_file, "en")?;
        let ja_options = DownloadOptions::default()
            .with_stream_selection(StreamSelection::audio_language("ja-JP"));
        let en_options = DownloadOptions::default()
            .with_stream_selection(StreamSelection::audio_language("en-US"));
        let fr_options = DownloadOptions::default()
            .with_stream_selection(StreamSelection::audio_language("fr-FR"));
        let mut archive = DownloadArchive::default();

        archive.record_download(
            &plan,
            &ja_options,
            &archive_audio_report(&plan, &output_dir, &ja_file),
        );
        archive.record_download(
            &plan,
            &en_options,
            &archive_audio_report(&plan, &output_dir, &en_file),
        );
        assert_eq!(archive.records.len(), 2);

        std::fs::remove_file(&ja_file)?;
        std::fs::remove_file(&en_file)?;
        std::fs::write(&fr_file, "fr")?;
        archive.record_download(
            &plan,
            &fr_options,
            &archive_audio_report(&plan, &output_dir, &fr_file),
        );

        assert_eq!(archive.records.len(), 1);
        assert_eq!(
            archive.records[0].content_key,
            format!(
                "mode=all;stream=alangfr-fr-{};plan|aid=170001;cid=2",
                super::short_identity_hash("fr-fr")
            )
        );
        Ok(())
    }

    fn archive_audio_report(
        plan: &DownloadPlan,
        output_dir: &Path,
        file_path: &Path,
    ) -> DownloadReport {
        DownloadReport {
            title: plan.title.clone(),
            output_dir: output_dir.to_path_buf(),
            entries: vec![EntryDownloadReport {
                index: 1,
                title: "Main".to_owned(),
                directory: file_path.parent().unwrap_or(output_dir).to_path_buf(),
                files: vec![DownloadedFile {
                    kind: DownloadFileKind::Audio,
                    path: file_path.to_path_buf(),
                    bytes_written: 2,
                    resumed_from: 0,
                }],
                mux: None,
            }],
        }
    }

    #[test]
    fn download_archive_distinguishes_danmaku_format_outputs() -> anyhow::Result<()> {
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
                    kind: DownloadFileKind::DanmakuAss,
                    path: Path::new("downloads")
                        .join("Mock video")
                        .join("entry")
                        .join("danmaku.ass"),
                    bytes_written: 5,
                    resumed_from: 0,
                }],
                mux: None,
            }],
        };
        let ass_options = DownloadOptions::default()
            .with_download_mode(DownloadMode::DanmakuOnly)
            .with_danmaku_format(DanmakuFormat::Ass);
        let mut archive = DownloadArchive::default();

        archive.record_download(&plan, &ass_options, &report);

        assert!(archive.records[0].content_key.contains("danmaku=ass"));
        assert_eq!(
            archive
                .records_for_plan_with_mode(&plan, DownloadMode::DanmakuOnly)
                .len(),
            1
        );
        let temp = tempfile::tempdir()?;
        let xml_preflight = DownloadPreflight::inspect(
            &plan,
            &DownloadOptions::new(temp.path()).with_download_mode(DownloadMode::DanmakuOnly),
            Some(&archive),
        )?;
        assert!(xml_preflight.archived_records.is_empty());
        let ass_preflight = DownloadPreflight::inspect(
            &plan,
            &DownloadOptions::new(temp.path())
                .with_download_mode(DownloadMode::DanmakuOnly)
                .with_danmaku_format(DanmakuFormat::Ass),
            Some(&archive),
        )?;
        assert_eq!(ass_preflight.archived_records.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn danmaku_update_archive_appends_xml_and_regenerates_ass() -> anyhow::Result<()> {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let temp = tempfile::tempdir()?;
        let output_dir = temp.path().join("downloads").join("Mock video");
        let entry_dir = output_dir.join("P001-BV1xx411c7mD-Main");
        std::fs::create_dir_all(&entry_dir)?;
        let xml_path = entry_dir.join("danmaku.xml");
        std::fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
        let danmaku_mock = server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200)
                .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
        });
        let mut archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: download_plan_content_key(&plan),
            title: plan.title.clone(),
            output_dir: output_dir.clone(),
            completed_at_unix: 1,
            entries: vec![DownloadArchiveEntryRecord {
                content_key: download_entry_content_key(&plan.entries[0]),
                index: 1,
                aid: plan.entries[0].aid,
                bvid: plan.entries[0].bvid.clone(),
                cid: plan.entries[0].cid,
                epid: plan.entries[0].epid,
                title: plan.entries[0].title.clone(),
                directory: entry_dir.clone(),
                files: vec![xml_path.clone()],
                mux_output: None,
            }],
        }]);
        let client = BiliClient::new(ClientConfig::default());

        let report = client
            .update_danmaku_for_archive(
                &plan,
                &mut archive,
                DanmakuUpdateOptions::default().with_danmaku_formats([DanmakuFormat::Ass]),
            )
            .await?;

        danmaku_mock.assert_calls(1);
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].existing_comments, 1);
        assert_eq!(report.entries[0].fetched_comments, 2);
        assert_eq!(report.entries[0].appended_comments, 1);
        assert_eq!(report.entries[0].files.len(), 2);
        let merged_xml = tokio::fs::read_to_string(&xml_path).await?;
        assert_eq!(merged_xml.matches("<d ").count(), 2);
        let ass_path = entry_dir.join("danmaku.ass");
        assert!(
            tokio::fs::read_to_string(&ass_path)
                .await?
                .contains("Dialogue:")
        );
        assert_eq!(archive.records[0].entries[0].files.len(), 2);
        assert!(archive.records[0].entries[0].files.iter().any(|path| path
            .ends_with(Path::new("P001-BV1xx411c7mD-Main").join("danmaku.xml"))));
        assert!(archive.records[0].entries[0].files.iter().any(|path| path
            .ends_with(Path::new("P001-BV1xx411c7mD-Main").join("danmaku.ass"))));
        assert!(
            archive.records[0]
                .content_key
                .starts_with("mode=all;danmaku=xml+ass;plan|")
        );
        assert!(
            archive.records[0].entries[0]
                .content_key
                .starts_with("mode=all;danmaku=xml+ass;aid=170001;cid=2")
        );
        let ass_preflight = DownloadPreflight::inspect(
            &plan,
            &DownloadOptions::new(temp.path().join("next"))
                .with_download_mode(DownloadMode::All)
                .with_danmaku_formats([DanmakuFormat::Xml, DanmakuFormat::Ass]),
            Some(&archive),
        )?;
        assert_eq!(ass_preflight.archived_records.len(), 1);
        let xml_preflight = DownloadPreflight::inspect(
            &plan,
            &DownloadOptions::new(temp.path().join("xml-only")),
            Some(&archive),
        )?;
        assert!(xml_preflight.archived_records.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn danmaku_update_partial_selection_keeps_record_content_key_conservative()
    -> anyhow::Result<()> {
        let server = MockServer::start();
        let mut full_plan = test_plan(&server);
        let mut second_entry = full_plan.entries[0].clone();
        second_entry.index = 2;
        second_entry.cid = 3;
        second_entry.danmaku.cid = 3;
        second_entry.title = "Second".to_owned();
        second_entry.danmaku.xml_url = format!("{}/danmaku-2.xml", server.base_url());
        full_plan.entries.push(second_entry);
        let selected_plan = DownloadPlan {
            title: full_plan.title.clone(),
            entries: vec![full_plan.entries[0].clone()],
        };
        let temp = tempfile::tempdir()?;
        let output_dir = temp.path().join("downloads").join("Mock video");
        let first_dir = output_dir.join("P001-BV1xx411c7mD-Main");
        let second_dir = output_dir.join("P002-BV1xx411c7mD-Second");
        std::fs::create_dir_all(&first_dir)?;
        std::fs::create_dir_all(&second_dir)?;
        let first_xml = first_dir.join("danmaku.xml");
        let second_xml = second_dir.join("danmaku.xml");
        std::fs::write(&first_xml, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
        std::fs::write(&second_xml, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
        server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200)
                .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
        });
        let original_record_key = download_plan_content_key(&full_plan);
        let original_second_entry_key = download_entry_content_key(&full_plan.entries[1]);
        let mut archive = DownloadArchive::new(vec![DownloadArchiveRecord {
            content_key: original_record_key.clone(),
            title: full_plan.title.clone(),
            output_dir: output_dir.clone(),
            completed_at_unix: 1,
            entries: vec![
                DownloadArchiveEntryRecord {
                    content_key: download_entry_content_key(&full_plan.entries[0]),
                    index: 1,
                    aid: full_plan.entries[0].aid,
                    bvid: full_plan.entries[0].bvid.clone(),
                    cid: full_plan.entries[0].cid,
                    epid: full_plan.entries[0].epid,
                    title: full_plan.entries[0].title.clone(),
                    directory: first_dir,
                    files: vec![first_xml],
                    mux_output: None,
                },
                DownloadArchiveEntryRecord {
                    content_key: original_second_entry_key.clone(),
                    index: 2,
                    aid: full_plan.entries[1].aid,
                    bvid: full_plan.entries[1].bvid.clone(),
                    cid: full_plan.entries[1].cid,
                    epid: full_plan.entries[1].epid,
                    title: full_plan.entries[1].title.clone(),
                    directory: second_dir,
                    files: vec![second_xml],
                    mux_output: None,
                },
            ],
        }]);
        let client = BiliClient::new(ClientConfig::default());

        let report = client
            .update_danmaku_for_archive(
                &selected_plan,
                &mut archive,
                DanmakuUpdateOptions::default().with_danmaku_formats([DanmakuFormat::Ass]),
            )
            .await?;

        assert_eq!(report.entries.len(), 1);
        assert_eq!(archive.records[0].content_key, original_record_key);
        assert!(
            archive.records[0].entries[0]
                .content_key
                .starts_with("mode=all;danmaku=xml+ass;aid=170001;cid=2")
        );
        assert_eq!(
            archive.records[0].entries[1].content_key,
            original_second_entry_key
        );
        Ok(())
    }

    #[tokio::test]
    async fn danmaku_update_skips_archive_entries_without_danmaku_sidecars() -> anyhow::Result<()> {
        let server = MockServer::start();
        let plan = test_plan(&server);
        let temp = tempfile::tempdir()?;
        let no_danmaku_root = temp.path().join("downloads").join("No danmaku");
        let danmaku_root = temp.path().join("downloads").join("With danmaku");
        let no_danmaku_dir = no_danmaku_root.join("P001-BV1xx411c7mD-Main");
        let danmaku_dir = danmaku_root.join("P001-BV1xx411c7mD-Main");
        std::fs::create_dir_all(&no_danmaku_dir)?;
        std::fs::create_dir_all(&danmaku_dir)?;
        let cover_path = no_danmaku_dir.join("cover.jpg");
        let xml_path = danmaku_dir.join("danmaku.xml");
        std::fs::write(&cover_path, "cover")?;
        std::fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
        let danmaku_mock = server.mock(|when, then| {
            when.method(GET).path("/danmaku.xml");
            then.status(200)
                .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
        });
        let no_danmaku_options = DownloadOptions::default().with_danmaku(false);
        let no_danmaku_record_key =
            download_plan_content_key_for_options(&plan, &no_danmaku_options);
        let no_danmaku_entry_key =
            download_entry_content_key_for_options(&plan.entries[0], &no_danmaku_options);
        let mut archive = DownloadArchive::new(vec![
            DownloadArchiveRecord {
                content_key: no_danmaku_record_key.clone(),
                title: plan.title.clone(),
                output_dir: no_danmaku_root,
                completed_at_unix: 1,
                entries: vec![DownloadArchiveEntryRecord {
                    content_key: no_danmaku_entry_key.clone(),
                    index: 1,
                    aid: plan.entries[0].aid,
                    bvid: plan.entries[0].bvid.clone(),
                    cid: plan.entries[0].cid,
                    epid: plan.entries[0].epid,
                    title: plan.entries[0].title.clone(),
                    directory: no_danmaku_dir.clone(),
                    files: vec![cover_path.clone()],
                    mux_output: None,
                }],
            },
            DownloadArchiveRecord {
                content_key: download_plan_content_key(&plan),
                title: plan.title.clone(),
                output_dir: danmaku_root,
                completed_at_unix: 1,
                entries: vec![DownloadArchiveEntryRecord {
                    content_key: download_entry_content_key(&plan.entries[0]),
                    index: 1,
                    aid: plan.entries[0].aid,
                    bvid: plan.entries[0].bvid.clone(),
                    cid: plan.entries[0].cid,
                    epid: plan.entries[0].epid,
                    title: plan.entries[0].title.clone(),
                    directory: danmaku_dir.clone(),
                    files: vec![xml_path.clone()],
                    mux_output: None,
                }],
            },
        ]);
        let client = BiliClient::new(ClientConfig::default());

        let report = client
            .update_danmaku_for_archive(
                &plan,
                &mut archive,
                DanmakuUpdateOptions::default().with_danmaku_formats([DanmakuFormat::Ass]),
            )
            .await?;

        danmaku_mock.assert_calls(1);
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].directory, danmaku_dir);
        assert_eq!(archive.records[0].content_key, no_danmaku_record_key);
        assert_eq!(
            archive.records[0].entries[0].content_key,
            no_danmaku_entry_key
        );
        assert_eq!(archive.records[0].entries[0].files, vec![cover_path]);
        assert!(!no_danmaku_dir.join("danmaku.xml").exists());
        assert!(
            archive.records[1]
                .entries
                .iter()
                .flat_map(|entry| &entry.files)
                .any(|path| path.ends_with(Path::new("P001-BV1xx411c7mD-Main").join("danmaku.ass")))
        );
        Ok(())
    }

    #[test]
    fn archive_entry_danmaku_update_target_requires_danmaku_eligible_archive_mode() {
        let mut entry = DownloadArchiveEntryRecord {
            content_key: "aid=170001;cid=2".to_owned(),
            index: 1,
            aid: 170_001,
            bvid: Some("BV1xx411c7mD".to_owned()),
            cid: 2,
            epid: None,
            title: "Main".to_owned(),
            directory: Path::new("downloads").join("entry"),
            files: Vec::new(),
            mux_output: None,
        };

        assert!(!super::archive_entry_allows_danmaku_update(
            "plan|aid=170001;cid=2",
            &entry
        ));

        entry
            .files
            .push(Path::new("downloads").join("entry").join("danmaku.xml"));
        assert!(super::archive_entry_allows_danmaku_update(
            "plan|aid=170001;cid=2",
            &entry
        ));

        entry.content_key = "mode=cover;aid=170001;cid=2".to_owned();
        assert!(!super::archive_entry_allows_danmaku_update(
            "mode=cover;plan|aid=170001;cid=2",
            &entry
        ));

        entry.content_key = "mode=danmaku;aid=170001;cid=2".to_owned();
        entry.files.clear();
        assert!(super::archive_entry_allows_danmaku_update(
            "mode=danmaku;plan|aid=170001;cid=2",
            &entry
        ));
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
            language: None,
            language_doc: None,
            codecs: None,
            codec_family: None,
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
                        language: None,
                        language_doc: None,
                        codecs: None,
                        codec_family: None,
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
                        language: None,
                        language_doc: None,
                        codecs: None,
                        codec_family: None,
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
                    is_ai_generated: false,
                    ai_type: None,
                    ai_status: None,
                    url: format!("{}/subtitle.ass", server.base_url()),
                    format: SubtitleFormat::Ass,
                }],
                chapters: Vec::new(),
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

    fn push_progress_event(
        events: &Arc<Mutex<Vec<DownloadProgressEvent>>>,
        event: DownloadProgressEvent,
    ) {
        match events.lock() {
            Ok(mut events) => events.push(event),
            Err(poisoned) => poisoned.into_inner().push(event),
        }
    }

    fn progress_events_snapshot(
        events: &Arc<Mutex<Vec<DownloadProgressEvent>>>,
    ) -> Vec<DownloadProgressEvent> {
        match events.lock() {
            Ok(events) => events.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn server_authority(server: &MockServer) -> anyhow::Result<String> {
        let parsed = url::Url::parse(&server.base_url())?;
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("mock server base URL has no host"))?;
        Ok(parsed
            .port()
            .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}")))
    }

    fn test_entry_dir(base: &Path, plan: &DownloadPlan) -> anyhow::Result<std::path::PathBuf> {
        Ok(
            default_plan_output_dir(plan, &DownloadOptions::new(base))?.join(entry_dir_name(
                &plan.title,
                &plan.entries[0],
                &DownloadOptions::default(),
            )?),
        )
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

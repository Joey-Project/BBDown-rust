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
use std::time::Duration;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const MAX_FILE_NAME_BYTES: usize = 80;
const MAX_FILE_COMPONENT_BYTES: usize = 240;
const MAX_SUBTITLE_EXTENSION_BYTES: usize = 16;

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct DownloadOptions {
    pub output_dir: PathBuf,
    pub retry: RetryPolicy,
    pub stream_selection: StreamSelection,
    pub resume: bool,
    pub include_subtitles: bool,
    pub include_danmaku: bool,
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
            include_subtitles: true,
            include_danmaku: true,
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
    pub fn with_subtitles(mut self, include_subtitles: bool) -> Self {
        self.include_subtitles = include_subtitles;
        self
    }

    #[must_use]
    pub fn with_danmaku(mut self, include_danmaku: bool) -> Self {
        self.include_danmaku = include_danmaku;
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadFileKind {
    Video,
    Audio,
    FlvSegment,
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
        let plan = self.plan(input, selection).await?;
        self.download_plan(&plan, options).await
    }

    pub async fn download_plan(
        &self,
        plan: &DownloadPlan,
        options: DownloadOptions,
    ) -> Result<DownloadReport> {
        validate_plan_stream_selection(plan, options.stream_selection)?;
        let output_dir = options.output_dir.join(safe_file_name(&plan.title));
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
        let has_dash_pair = !entry.streams.videos.is_empty() && !entry.streams.audios.is_empty();
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
                self.download_media_stream(video, DownloadFileKind::Video, &entry_dir, options)
                    .await?,
            );
            files.push(
                self.download_media_stream(audio, DownloadFileKind::Audio, &entry_dir, options)
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
                    self.download_flv_segment(segment, &entry_dir, options)
                        .await?,
                );
            }
        } else {
            return Err(Error::MissingField("complete DASH media or FLV segments"));
        }
        if options.include_subtitles {
            let mut seen_subtitles = HashSet::new();
            for (index, subtitle) in entry.subtitles.iter().enumerate() {
                if !seen_subtitles.insert(subtitle_dedup_key(&subtitle.url)) {
                    continue;
                }
                files.push(
                    self.download_subtitle(index, subtitle, &entry_dir, options)
                        .await?,
                );
            }
        }
        if options.include_danmaku {
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
        let mux = self.mux_entry(entry, &entry_dir, &files, options).await?;
        Ok(EntryDownloadReport {
            index: entry.index,
            title: entry.title.clone(),
            directory: entry_dir,
            files,
            mux,
        })
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
        if is_empty_unexpected_media_response(&kind, bytes_written) {
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

fn is_empty_unexpected_media_response(kind: &DownloadFileKind, bytes_written: u64) -> bool {
    kind.is_media() && bytes_written == 0
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

fn concat_file_list(paths: &[PathBuf], base: &Path) -> String {
    paths.iter().fold(String::new(), |mut output, path| {
        let list_path = path.strip_prefix(base).unwrap_or(path);
        let escaped = list_path.to_string_lossy().replace('\'', "'\\''");
        let _ = writeln!(output, "file '{escaped}'");
        output
    })
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

fn validate_plan_stream_selection(plan: &DownloadPlan, selection: StreamSelection) -> Result<()> {
    if !selection.has_selection() {
        return Ok(());
    }
    for entry in &plan.entries {
        validate_entry_stream_selection(entry, selection)?;
    }
    Ok(())
}

fn validate_entry_stream_selection(
    entry: &DownloadEntry,
    selection: StreamSelection,
) -> Result<()> {
    let has_dash_pair = !entry.streams.videos.is_empty() && !entry.streams.audios.is_empty();
    let use_flv_fallback = !has_dash_pair && !entry.streams.flv_segments.is_empty();
    if has_dash_pair {
        let _ = select_media_stream(&entry.streams.videos, selection.video_quality, "video")?;
        let _ = select_media_stream(&entry.streams.audios, selection.audio_quality, "audio")?;
    } else if use_flv_fallback {
        return Err(Error::InvalidInput(
            "stream quality selection requires DASH media; selected entry only has FLV segments"
                .to_owned(),
        ));
    } else {
        return Err(Error::MissingField("complete DASH media or FLV segments"));
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
        DownloadOptions, MAX_FILE_COMPONENT_BYTES, MAX_FILE_NAME_BYTES,
        MAX_SUBTITLE_EXTENSION_BYTES, MuxOptions, RetryPolicy, entry_dir_name, media_file_name,
        safe_file_name, safe_file_name_with_budget, select_media_stream, subtitle_dedup_key,
        subtitle_extension, subtitle_file_name, temporary_download_path, temporary_mux_path,
        temporary_replace_path,
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
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        let entry = &report.entries[0];
        assert_eq!(entry.files.len(), 4);
        let video = entry
            .files
            .iter()
            .find(|file| file.kind == DownloadFileKind::Video)
            .ok_or_else(|| anyhow::anyhow!("missing video"))?;
        assert_eq!(tokio::fs::read_to_string(&video.path).await?, "video");
        assert!(entry.mux.is_none());
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
                    include_subtitles: false,
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
            include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
            include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
            include_danmaku: false,
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
            include_danmaku: false,
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
            include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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
                    include_danmaku: false,
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

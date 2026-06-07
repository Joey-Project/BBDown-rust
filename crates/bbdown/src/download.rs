use crate::{
    BiliClient, DownloadEntry, DownloadPlan, Error, FlvSegment, Input, MediaStream, Result,
    Selection, SubtitleFormat, SubtitleTrack,
};
use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Clone, Debug)]
pub struct DownloadOptions {
    pub output_dir: PathBuf,
    pub retry: RetryPolicy,
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
            resume: true,
            include_subtitles: true,
            include_danmaku: true,
            mux: MuxOptions::Disabled,
            download_idle_timeout: Some(Duration::from_secs(30)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff: Duration,
}

impl RetryPolicy {
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
        if entry.streams.videos.is_empty() && entry.streams.audios.is_empty() {
            for segment in &entry.streams.flv_segments {
                files.push(
                    self.download_flv_segment(segment, &entry_dir, options)
                        .await?,
                );
            }
        } else {
            if let Some(video) = entry.streams.videos.first() {
                files.push(
                    self.download_media_stream(video, DownloadFileKind::Video, &entry_dir, options)
                        .await?,
                );
            }
            if let Some(audio) = entry.streams.audios.first() {
                files.push(
                    self.download_media_stream(audio, DownloadFileKind::Audio, &entry_dir, options)
                        .await?,
                );
            }
        }
        if options.include_subtitles {
            for subtitle in &entry.subtitles {
                files.push(
                    self.download_subtitle(subtitle, &entry_dir, options)
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
        let path = entry_dir.join(format!(
            "{label}-{}.{}",
            stream.id,
            media_extension(&stream.base_url, stream.mime_type.as_deref())
        ));
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
        subtitle: &SubtitleTrack,
        entry_dir: &Path,
        options: &DownloadOptions,
    ) -> Result<DownloadedFile> {
        let path = entry_dir.join(format!(
            "subtitle-{}.{}",
            safe_file_name(&subtitle.language),
            subtitle_extension(subtitle)
        ));
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
        let resume_from = if options.resume {
            existing_file_len(path).await?
        } else {
            0
        };
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
        let content_range = content_range(response.headers())?;
        let append = resume_from > 0 && status == StatusCode::PARTIAL_CONTENT;
        if status == StatusCode::PARTIAL_CONTENT {
            let range = content_range.ok_or_else(|| {
                Error::InvalidInput(
                    "server returned partial content without Content-Range".to_owned(),
                )
            })?;
            let expected_start = if append { resume_from } else { 0 };
            if range.start != expected_start {
                return Err(Error::InvalidInput(
                    "server returned an unexpected Content-Range for resume".to_owned(),
                ));
            }
        }
        let start_offset = if append { resume_from } else { 0 };
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path)
            .await?;
        let mut bytes_written = 0;
        let mut stream = response.bytes_stream();
        while let Some(chunk) =
            match next_download_chunk(&mut stream, options.download_idle_timeout).await {
                Ok(chunk) => chunk,
                Err(error) => {
                    rollback_download_file(&file, start_offset).await?;
                    return Err(error);
                }
            }
        {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    rollback_download_file(&file, start_offset).await?;
                    return Err(BiliClient::http_error_without_url(error));
                }
            };
            file.write_all(&chunk).await?;
            bytes_written += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        }
        file.flush().await?;
        if let Err(error) =
            validate_download_completion(expected_size, content_range, start_offset, bytes_written)
        {
            rollback_download_file(&file, start_offset).await?;
            return Err(error);
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
        let mut args = Vec::new();
        args.push("-y".to_owned());
        if only_flv_segments(files) {
            let list_path = entry_dir.join("ffmpeg-concat.txt");
            fs::write(&list_path, concat_file_list(&media_files)).await?;
            args.extend([
                "-f".to_owned(),
                "concat".to_owned(),
                "-safe".to_owned(),
                "0".to_owned(),
                "-i".to_owned(),
                list_path.to_string_lossy().into_owned(),
            ]);
        } else {
            for media_file in &media_files {
                args.push("-i".to_owned());
                args.push(media_file.to_string_lossy().into_owned());
            }
        }
        args.extend([
            "-c".to_owned(),
            "copy".to_owned(),
            output_path.to_string_lossy().into_owned(),
        ]);
        let status = Command::new(binary)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await?;
        if !status.success() {
            return Err(Error::MuxFailed {
                status: status.code().map_or_else(
                    || "terminated by signal".to_owned(),
                    |code| code.to_string(),
                ),
            });
        }
        let mut command = Vec::with_capacity(args.len() + 1);
        command.push(binary.to_string_lossy().into_owned());
        command.extend(args);
        Ok(Some(MuxReport {
            output_path,
            command,
        }))
    }
}

impl DownloadFileKind {
    fn is_media(&self) -> bool {
        matches!(self, Self::Video | Self::Audio | Self::FlvSegment)
    }
}

async fn existing_file_len(path: &Path) -> Result<u64> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(Error::Io(error)),
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

fn concat_file_list(paths: &[PathBuf]) -> String {
    paths.iter().fold(String::new(), |mut output, path| {
        let escaped = path.to_string_lossy().replace('\'', "'\\''");
        let _ = writeln!(output, "file '{escaped}'");
        output
    })
}

fn entry_dir_name(entry: &DownloadEntry) -> String {
    format!("P{:03}-{}", entry.index, safe_file_name(&entry.title))
}

fn safe_file_name(raw: &str) -> String {
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
    value.chars().take(80).collect()
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

fn subtitle_extension(subtitle: &SubtitleTrack) -> String {
    match subtitle.format {
        SubtitleFormat::Json => "json".to_owned(),
        SubtitleFormat::Ass => "ass".to_owned(),
        SubtitleFormat::Unknown => {
            url_path_extension(&subtitle.url).unwrap_or_else(|| "subtitle".to_owned())
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{DownloadOptions, MuxOptions, RetryPolicy};
    use crate::models::{
        DanmakuTrack, DownloadEntry, DownloadPlan, MediaStream, StreamSet, StreamSource,
        SubtitleFormat, SubtitleTrack,
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
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = test_plan(&server);
        plan.entries[0].streams.videos[0].size = Some(6);
        plan.entries[0].streams.audios.clear();
        plan.entries[0].subtitles.clear();
        plan.entries[0].danmaku.xml_url = format!("{}/danmaku.xml", server.base_url());
        let output_dir = temp.path().join("Mock video").join("P001-Main");
        tokio::fs::create_dir_all(&output_dir).await?;
        tokio::fs::write(output_dir.join("video-80.m4s"), "old").await?;

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
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig {
            credentials: Credentials {
                cookie: Some("SESSDATA=secret".to_owned()),
                access_key: None,
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
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(3);
        let output_dir = temp.path().join("Mock video").join("P001-Main");
        tokio::fs::create_dir_all(&output_dir).await?;
        tokio::fs::write(output_dir.join("video-80.m4s"), "old").await?;

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
        let output_dir = temp.path().join("Mock video").join("P001-Main");
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join("video-80.m4s");
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
        let output_dir = temp.path().join("Mock video").join("P001-Main");
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join("video-80.m4s");
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
        let output_dir = temp.path().join("Mock video").join("P001-Main");
        tokio::fs::create_dir_all(&output_dir).await?;
        let path = output_dir.join("video-80.m4s");
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
        let temp = tempfile::tempdir()?;
        let client = BiliClient::new(ClientConfig::default());
        let mut plan = single_video_plan(format!("{}/video.m4s", server.base_url()));
        plan.entries[0].streams.videos[0].size = Some(6);
        let path = temp
            .path()
            .join("Mock video")
            .join("P001-Main")
            .join("video-80.m4s");

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
        let mut plan = single_video_plan(format!("http://{address}/video.m4s"));
        plan.entries[0].streams.videos[0].size = Some(2);

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    include_danmaku: false,
                    mux: MuxOptions::Disabled,
                    download_idle_timeout: Some(Duration::from_secs(1)),
                    ..DownloadOptions::default()
                },
            )
            .await?;

        handle
            .join()
            .map_err(|_| anyhow::anyhow!("server thread panicked"))??;
        assert_eq!(
            tokio::fs::read_to_string(&report.entries[0].files[0].path).await?,
            "ok"
        );
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
        let plan = single_video_plan(format!("http://{address}/video.m4s"));

        let Err(error) = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy::single_attempt(),
                    include_danmaku: false,
                    mux: MuxOptions::Disabled,
                    download_idle_timeout: Some(Duration::from_secs(1)),
                    ..DownloadOptions::default()
                },
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
        let plan = single_video_plan(format!("http://{address}/video.m4s"));

        let report = client
            .download_plan(
                &plan,
                DownloadOptions {
                    output_dir: temp.path().to_path_buf(),
                    retry: RetryPolicy {
                        max_attempts: 2,
                        backoff: Duration::from_millis(1),
                    },
                    include_danmaku: false,
                    mux: MuxOptions::Disabled,
                    ..DownloadOptions::default()
                },
            )
            .await?;

        handle
            .join()
            .map_err(|_| anyhow::anyhow!("server thread panicked"))??;
        assert_eq!(
            tokio::fs::read_to_string(&report.entries[0].files[0].path).await?,
            "retry-ok"
        );
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
        let temp = tempfile::tempdir()?;
        let ffmpeg = write_fake_ffmpeg(temp.path(), "exit 0")?;
        let client = BiliClient::new(ClientConfig::default());
        let plan = single_video_plan(format!("{}/video.m4s", server.base_url()));

        let report = client
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
            .await?;

        let mux = report.entries[0]
            .mux
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing mux report"))?;
        assert_eq!(mux.command[1], "-y");
        assert!(mux.command.iter().any(|arg| arg == "-c"));
        assert!(mux.output_path.ends_with("Main.mp4"));
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
                    duration_seconds: Some(3),
                },
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
        plan.entries[0].streams.audios.clear();
        plan.entries[0].subtitles.clear();
        plan
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
}

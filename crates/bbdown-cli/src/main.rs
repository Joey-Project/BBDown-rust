#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

use anyhow::{Context, bail, ensure};
use bbdown_core::{
    BiliClient, ClientConfig, CredentialStore, Credentials, DanmakuFormat, DownloadArchive,
    DownloadMode, DownloadOptions, DownloadPreflight, DownloadReport, DuplicateDecision,
    EndpointConfig, MediaHostOptions, MediaStream, MuxOptions, QrLoginKind, QrLoginState,
    QrLoginTicket, ResolvedContent, RestrictedArea, RestrictedAreaConfig, RestrictedAreaProxy,
    RestrictedAreaProxyKind, RetryPolicy, Selection, StreamQuality, StreamSelection, StreamSet,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Parser)]
#[command(name = "bbdown")]
#[command(version)]
#[command(about = "Rust-native Bilibili metadata and download tooling")]
struct Cli {
    #[arg(
        long,
        env = "BBDOWN_API_BASE",
        default_value = "https://api.bilibili.com"
    )]
    api_base: String,
    #[arg(
        long,
        env = "BBDOWN_PGC_BASE",
        default_value = "https://api.bilibili.com"
    )]
    pgc_base: String,
    #[arg(
        long,
        env = "BBDOWN_INTL_BASE",
        default_value = "https://api.bilibili.tv"
    )]
    intl_base: String,
    #[arg(
        long,
        env = "BBDOWN_COMMENT_BASE",
        default_value = "https://comment.bilibili.com"
    )]
    comment_base: String,
    #[arg(
        long,
        env = "BBDOWN_PASSPORT_BASE",
        default_value = "https://passport.bilibili.com"
    )]
    passport_base: String,
    #[arg(long, env = "BBDOWN_TV_PASSPORT_BASE")]
    tv_passport_base: Option<String>,
    #[arg(long, env = "BBDOWN_TV_PASSPORT_POLL_BASE")]
    tv_passport_poll_base: Option<String>,
    #[arg(long, env = "BBDOWN_RESTRICTED_AREA")]
    restricted_area: Option<String>,
    #[arg(
        long,
        env = "BBDOWN_RESTRICTED_AREA_PROXY",
        value_delimiter = ',',
        value_name = "[AREA=]URL"
    )]
    restricted_area_proxy: Vec<String>,
    #[arg(
        long,
        env = "BBDOWN_RESTRICTED_API_PROXY",
        value_delimiter = ',',
        value_name = "[AREA=]URL"
    )]
    restricted_api_proxy: Vec<String>,
    #[arg(long, env = "BBDOWN_CREDENTIAL_FILE")]
    credential_file: Option<PathBuf>,
    #[arg(long, env = "BBDOWN_REQUEST_TIMEOUT_SECONDS", default_value_t = 30)]
    request_timeout_seconds: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Info {
        url: String,
        #[arg(long)]
        select: Option<Selection>,
        #[arg(long)]
        json: bool,
    },
    Plan {
        url: String,
        #[arg(long)]
        select: Option<Selection>,
        #[arg(long)]
        json: bool,
    },
    Download {
        url: String,
        #[arg(long)]
        select: Option<Selection>,
        #[arg(long, default_value = ".")]
        output_dir: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 3)]
        retry_attempts: u32,
        #[arg(long, default_value_t = 250)]
        retry_backoff_ms: u64,
        #[arg(long, default_value_t = 30)]
        download_idle_timeout_seconds: u64,
        #[arg(long)]
        no_resume: bool,
        #[arg(long, value_enum, value_name = "KIND")]
        only: Option<DownloadOnlyArg>,
        #[arg(long)]
        no_cover: bool,
        #[arg(long)]
        no_subtitles: bool,
        #[arg(long)]
        no_danmaku: bool,
        #[arg(
            long = "danmaku-format",
            value_enum,
            value_delimiter = ',',
            default_value = "xml",
            value_name = "FORMAT"
        )]
        danmaku_formats: Vec<DanmakuFormatArg>,
        #[arg(long)]
        no_mux: bool,
        #[arg(long, value_name = "ID")]
        video_quality: Option<u32>,
        #[arg(long, value_name = "ID")]
        audio_quality: Option<u32>,
        #[arg(long, default_value = "ffmpeg")]
        ffmpeg: PathBuf,
        #[arg(long, value_name = "PATH")]
        archive_file: Option<PathBuf>,
        #[arg(long, value_enum)]
        on_duplicate: Option<DuplicateDecisionArg>,
        #[arg(long, value_name = "HOST")]
        upos_host: Option<String>,
        #[arg(long)]
        force_replace_host: bool,
        #[arg(long)]
        allow_pcdn: bool,
    },
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    Status,
    ImportCookie(SecretImportArgs),
    ImportAccessKey(SecretImportArgs),
    LoginWeb(QrLoginArgs),
    LoginTv(QrLoginArgs),
    Logout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum DuplicateDecisionArg {
    Replace,
    KeepBoth,
    Cancel,
}

impl From<DuplicateDecisionArg> for DuplicateDecision {
    fn from(value: DuplicateDecisionArg) -> Self {
        match value {
            DuplicateDecisionArg::Replace => Self::Replace,
            DuplicateDecisionArg::KeepBoth => Self::KeepBoth,
            DuplicateDecisionArg::Cancel => Self::Cancel,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum DownloadOnlyArg {
    Video,
    Audio,
    #[value(alias = "subtitles")]
    Subtitle,
    Danmaku,
    Cover,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum DanmakuFormatArg {
    Xml,
    Ass,
}

impl From<DanmakuFormatArg> for DanmakuFormat {
    fn from(value: DanmakuFormatArg) -> Self {
        match value {
            DanmakuFormatArg::Xml => Self::Xml,
            DanmakuFormatArg::Ass => Self::Ass,
        }
    }
}

impl From<DownloadOnlyArg> for DownloadMode {
    fn from(value: DownloadOnlyArg) -> Self {
        match value {
            DownloadOnlyArg::Video => Self::VideoOnly,
            DownloadOnlyArg::Audio => Self::AudioOnly,
            DownloadOnlyArg::Subtitle => Self::SubtitleOnly,
            DownloadOnlyArg::Danmaku => Self::DanmakuOnly,
            DownloadOnlyArg::Cover => Self::CoverOnly,
        }
    }
}

fn validate_single_download_args(
    only: Option<DownloadOnlyArg>,
    no_cover: bool,
    no_subtitles: bool,
    no_danmaku: bool,
    video_quality: Option<u32>,
    audio_quality: Option<u32>,
) -> anyhow::Result<()> {
    let Some(only) = only else {
        return Ok(());
    };
    ensure!(
        !(matches!(only, DownloadOnlyArg::Cover) && no_cover),
        "--only cover conflicts with --no-cover"
    );
    ensure!(
        !(matches!(only, DownloadOnlyArg::Subtitle) && no_subtitles),
        "--only subtitle conflicts with --no-subtitles"
    );
    ensure!(
        !(matches!(only, DownloadOnlyArg::Danmaku) && no_danmaku),
        "--only danmaku conflicts with --no-danmaku"
    );
    ensure!(
        !(matches!(only, DownloadOnlyArg::Video) && audio_quality.is_some()),
        "--only video conflicts with --audio-quality"
    );
    ensure!(
        !(matches!(only, DownloadOnlyArg::Audio) && video_quality.is_some()),
        "--only audio conflicts with --video-quality"
    );
    ensure!(
        matches!(only, DownloadOnlyArg::Video | DownloadOnlyArg::Audio)
            || (video_quality.is_none() && audio_quality.is_none()),
        "stream quality selection requires --only video, --only audio, or the default download mode"
    );
    Ok(())
}

struct DownloadOptionCliArgs {
    output_dir: PathBuf,
    retry_attempts: u32,
    retry_backoff_ms: u64,
    download_idle_timeout_seconds: u64,
    only: Option<DownloadOnlyArg>,
    execution: DownloadExecutionCliFlags,
    sidecars: DownloadSidecarCliFlags,
    media_hosts: DownloadMediaHostCliFlags,
    danmaku_formats: Vec<DanmakuFormatArg>,
    video_quality: Option<u32>,
    audio_quality: Option<u32>,
    ffmpeg: PathBuf,
}

struct DownloadExecutionCliFlags {
    no_resume: bool,
    no_mux: bool,
}

struct DownloadSidecarCliFlags {
    no_cover: bool,
    no_subtitles: bool,
    no_danmaku: bool,
}

struct DownloadMediaHostCliFlags {
    upos_host: Option<String>,
    force_replace_host: bool,
    allow_pcdn: bool,
}

fn download_options_from_cli(args: DownloadOptionCliArgs) -> anyhow::Result<DownloadOptions> {
    ensure!(
        args.retry_attempts > 0,
        "--retry-attempts must be greater than 0"
    );
    validate_single_download_args(
        args.only,
        args.sidecars.no_cover,
        args.sidecars.no_subtitles,
        args.sidecars.no_danmaku,
        args.video_quality,
        args.audio_quality,
    )?;
    let download_idle_timeout = if args.download_idle_timeout_seconds == 0 {
        None
    } else {
        Some(Duration::from_secs(args.download_idle_timeout_seconds))
    };
    let mode = args.only.map_or(DownloadMode::All, Into::into);
    let mux = if args.execution.no_mux {
        MuxOptions::Disabled
    } else {
        MuxOptions::ffmpeg(args.ffmpeg)
    };
    let media_hosts = media_host_options_from_cli(args.media_hosts)?;
    Ok(DownloadOptions::new(args.output_dir)
        .with_retry_policy(RetryPolicy::new(
            args.retry_attempts,
            Duration::from_millis(args.retry_backoff_ms),
        ))
        .with_stream_selection(StreamSelection::new(args.video_quality, args.audio_quality))
        .with_download_idle_timeout(download_idle_timeout)
        .with_resume(!args.execution.no_resume)
        .with_download_mode(mode)
        .with_cover(!args.sidecars.no_cover)
        .with_subtitles(!args.sidecars.no_subtitles)
        .with_danmaku(!args.sidecars.no_danmaku)
        .with_danmaku_formats(args.danmaku_formats.into_iter().map(Into::into))
        .with_media_hosts(media_hosts)
        .with_mux(mux))
}

fn media_host_options_from_cli(
    flags: DownloadMediaHostCliFlags,
) -> anyhow::Result<MediaHostOptions> {
    let options = MediaHostOptions::bbdown_cli_default()
        .with_force_replace_host(flags.force_replace_host)
        .with_allow_pcdn(flags.allow_pcdn);
    let Some(upos_host) = flags.upos_host else {
        return Ok(options);
    };
    validate_media_host_spec(&upos_host)?;
    Ok(options.with_upos_host(upos_host))
}

fn validate_media_host_spec(host: &str) -> anyhow::Result<()> {
    let host = host.trim().trim_end_matches('/');
    ensure!(!host.is_empty(), "--upos-host must not be empty");
    let parse_input = if host.contains("://") {
        host.to_owned()
    } else {
        format!("https://{host}")
    };
    let parsed = url::Url::parse(&parse_input)
        .with_context(|| format!("invalid --upos-host value `{host}`"))?;
    ensure!(
        parsed.host_str().is_some(),
        "--upos-host must include a host name or IP address"
    );
    ensure!(
        parsed.path() == "/" && parsed.query().is_none() && parsed.fragment().is_none(),
        "--upos-host expects only a host or host:port, not a path, query, or fragment"
    );
    Ok(())
}

#[derive(Debug, Args)]
struct SecretImportArgs {
    #[arg(long, conflicts_with = "file")]
    stdin: bool,
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct QrLoginArgs {
    #[arg(long)]
    json: bool,
    #[arg(long, default_value_t = 180)]
    timeout_seconds: u64,
    #[arg(long, default_value_t = 1)]
    poll_interval_seconds: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let raw_args = std::env::args_os().collect::<Vec<_>>();
    let cli = Cli::parse_from(raw_args.clone());
    ensure!(
        cli.request_timeout_seconds > 0,
        "--request-timeout-seconds must be greater than 0"
    );
    let endpoints = endpoints_from_cli(&cli);
    let restricted_area = restricted_area_from_cli_with_args(&cli, raw_args)?;
    let request_timeout = Duration::from_secs(cli.request_timeout_seconds);
    let store = CredentialStore::new(credential_path(cli.credential_file)?);
    match cli.command {
        Command::Info { url, select, json } => {
            handle_info(
                &store,
                endpoints.clone(),
                restricted_area.clone(),
                request_timeout,
                url,
                select,
                json,
            )
            .await?;
        }
        Command::Plan { url, select, json } => {
            handle_plan(
                &store,
                endpoints.clone(),
                restricted_area.clone(),
                request_timeout,
                url,
                select,
                json,
            )
            .await?;
        }
        Command::Download {
            url,
            select,
            output_dir,
            json,
            retry_attempts,
            retry_backoff_ms,
            download_idle_timeout_seconds,
            no_resume,
            only,
            no_cover,
            no_subtitles,
            no_danmaku,
            danmaku_formats,
            no_mux,
            video_quality,
            audio_quality,
            ffmpeg,
            archive_file,
            on_duplicate,
            upos_host,
            force_replace_host,
            allow_pcdn,
        } => {
            ensure!(
                archive_file.is_some() || on_duplicate.is_none(),
                "--on-duplicate requires --archive-file"
            );
            let options = download_options_from_cli(DownloadOptionCliArgs {
                output_dir,
                retry_attempts,
                retry_backoff_ms,
                download_idle_timeout_seconds,
                only,
                execution: DownloadExecutionCliFlags { no_resume, no_mux },
                sidecars: DownloadSidecarCliFlags {
                    no_cover,
                    no_subtitles,
                    no_danmaku,
                },
                media_hosts: DownloadMediaHostCliFlags {
                    upos_host,
                    force_replace_host,
                    allow_pcdn,
                },
                danmaku_formats,
                video_quality,
                audio_quality,
                ffmpeg,
            })?;
            let args = DownloadCommandArgs {
                url,
                select,
                json,
                options,
                archive_file,
                on_duplicate: on_duplicate.map(Into::into),
            };
            handle_download(&store, endpoints, restricted_area, request_timeout, args).await?;
        }
        Command::Auth { command } => {
            handle_auth(command, &store, endpoints, restricted_area, request_timeout).await?;
        }
    }
    Ok(())
}

struct DownloadCommandArgs {
    url: String,
    select: Option<Selection>,
    json: bool,
    options: DownloadOptions,
    archive_file: Option<PathBuf>,
    on_duplicate: Option<DuplicateDecision>,
}

async fn handle_info(
    store: &CredentialStore,
    endpoints: EndpointConfig,
    restricted_area: RestrictedAreaConfig,
    request_timeout: Duration,
    url: String,
    select: Option<Selection>,
    json: bool,
) -> anyhow::Result<()> {
    let credentials = store.load().context("failed to load credentials")?;
    let client = BiliClient::new(client_config(
        endpoints,
        restricted_area,
        request_timeout,
        credentials,
    ));
    let resolved = client.resolve_input(&url, select).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resolved)?);
    } else {
        print_human_summary(&resolved);
    }
    Ok(())
}

async fn handle_plan(
    store: &CredentialStore,
    endpoints: EndpointConfig,
    restricted_area: RestrictedAreaConfig,
    request_timeout: Duration,
    url: String,
    select: Option<Selection>,
    json: bool,
) -> anyhow::Result<()> {
    let credentials = store.load().context("failed to load credentials")?;
    let client = BiliClient::new(client_config(
        endpoints,
        restricted_area,
        request_timeout,
        credentials,
    ));
    let plan = client.plan_download(&url, select).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print_plan_summary(&plan);
    }
    Ok(())
}

fn print_plan_summary(plan: &bbdown_core::DownloadPlan) {
    println!("title: {}", plan.title);
    println!("entries: {}", plan.entries.len());
    for entry in &plan.entries {
        println!(
            "- P{} aid={} cid={} title={}",
            entry.index, entry.aid, entry.cid, entry.title
        );
        print_streams(&entry.streams);
        println!("  subtitles: {}", entry.subtitles.len());
        println!("  danmaku: {}", entry.danmaku.xml_url);
    }
}

fn print_streams(streams: &StreamSet) {
    println!("  qualities: {}", quality_list(&streams.qualities));
    println!("  videos: {}", streams.videos.len());
    for stream in &streams.videos {
        println!("    - {}", media_stream_summary("q", stream));
    }
    println!("  audios: {}", streams.audios.len());
    for stream in &streams.audios {
        println!("    - {}", media_stream_summary("id", stream));
    }
    println!("  flv_segments: {}", streams.flv_segments.len());
}

fn quality_list(qualities: &[StreamQuality]) -> String {
    if qualities.is_empty() {
        return "none".to_owned();
    }
    qualities
        .iter()
        .map(|quality| match quality.description.as_deref() {
            Some(description) => format!("{} ({description})", quality.id),
            None => quality.id.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn media_stream_summary(label: &str, stream: &MediaStream) -> String {
    let mut parts = vec![format!("{label}={}", stream.id)];
    if let (Some(width), Some(height)) = (stream.width, stream.height) {
        parts.push(format!("{width}x{height}"));
    }
    if let Some(frame_rate) = stream.frame_rate.as_deref() {
        parts.push(format!("{frame_rate}fps"));
    }
    if let Some(codecs) = stream.codecs.as_deref() {
        parts.push(codecs.to_owned());
    }
    if let Some(bandwidth) = stream.bandwidth {
        parts.push(format!("{bandwidth}bps"));
    }
    parts.join(" ")
}

async fn handle_download(
    store: &CredentialStore,
    endpoints: EndpointConfig,
    restricted_area: RestrictedAreaConfig,
    request_timeout: Duration,
    args: DownloadCommandArgs,
) -> anyhow::Result<()> {
    let credentials = store.load().context("failed to load credentials")?;
    let client = BiliClient::new(client_config(
        endpoints,
        restricted_area,
        request_timeout,
        credentials,
    ));
    let report = if let Some(archive_file) = args.archive_file {
        let plan = client
            .plan_download_with_mode(&args.url, args.select, args.options.mode)
            .await?;
        let mut archive = DownloadArchive::load(&archive_file)
            .with_context(|| format!("failed to load archive {}", archive_file.display()))?;
        let preflight = DownloadPreflight::inspect(&plan, &args.options, Some(&archive))?;
        let stdin_is_terminal = io::stdin().is_terminal();
        let duplicate_prompt_printed_preflight = should_prompt_duplicate_decision(
            args.on_duplicate,
            args.json,
            &preflight,
            stdin_is_terminal,
        );
        let decision =
            duplicate_decision(args.on_duplicate, args.json, stdin_is_terminal, &preflight)?;
        let execution_decision = if args.on_duplicate.is_none() && !preflight.requires_decision() {
            DuplicateDecision::Cancel
        } else {
            decision
        };
        if preflight.requires_decision() && decision == DuplicateDecision::Cancel {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "canceled",
                        "preflight": preflight,
                    }))?
                );
            } else {
                if !duplicate_prompt_printed_preflight {
                    print_duplicate_preflight(&preflight);
                }
                println!("download canceled");
            }
            return Ok(());
        }
        let decision_output_dir = preflight.output_dir_for_decision(decision)?;
        ensure_archive_file_is_not_output_root(&archive_file, &decision_output_dir)?;
        let report = client
            .download_plan_with_archive_preflight_decision(
                &plan,
                args.options,
                &mut archive,
                &preflight,
                execution_decision,
            )
            .await?;
        ensure_archive_file_is_not_output_root(&archive_file, &report.output_dir)?;
        archive
            .save(&archive_file)
            .with_context(|| format!("failed to save archive {}", archive_file.display()))?;
        report
    } else {
        client
            .download_input(&args.url, args.select, args.options)
            .await?
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_download_report(&report);
    }
    Ok(())
}

fn ensure_archive_file_is_not_output_root(
    archive_file: &Path,
    output_dir: &Path,
) -> anyhow::Result<()> {
    let output_dir_display = output_dir.display().to_string();
    let output_lexical =
        lexical_path_components(output_dir).context("failed to resolve output directory")?;
    let output_canonical =
        canonical_path_components(output_dir).context("failed to resolve output directory")?;
    for archive_path in archive_write_paths(archive_file)? {
        let archive_lexical =
            lexical_path_components(&archive_path).context("failed to resolve archive path")?;
        let archive_canonical =
            canonical_path_components(&archive_path).context("failed to resolve archive path")?;
        ensure!(
            !paths_overlap(&archive_lexical, &output_lexical)
                && !paths_overlap(&archive_canonical, &output_canonical),
            "--archive-file and its sidecar files must not overlap the chosen output directory ({output_dir_display})"
        );
    }
    Ok(())
}

fn archive_write_paths(archive_file: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    push_archive_write_paths(&mut paths, archive_file);
    let storage_path = archive_storage_path(archive_file)?;
    if storage_path != archive_file {
        push_archive_write_paths(&mut paths, &storage_path);
    }
    Ok(paths)
}

fn push_archive_write_paths(paths: &mut Vec<PathBuf>, archive_file: &Path) {
    for path in [
        archive_file.to_path_buf(),
        archive_sidecar_path(archive_file, ".bbdown-archive-tmp"),
        archive_sidecar_path(archive_file, ".bbdown-archive-backup"),
    ] {
        if !paths.iter().any(|existing| existing == &path) {
            paths.push(path);
        }
    }
}

fn archive_storage_path(path: &Path) -> anyhow::Result<PathBuf> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(path.to_path_buf());
        }
        Err(error) => return Err(error).context("failed to inspect archive path"),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(path.to_path_buf());
    }
    let target = fs::read_link(path).context("failed to read archive symlink")?;
    let target = if target.is_absolute() {
        target
    } else {
        path.parent().unwrap_or_else(|| Path::new("")).join(target)
    };
    Ok(canonicalize_existing_prefix(&absolute_path(&target)?))
}

fn archive_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let base = path.file_name().map_or_else(
        || "download-archive".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!("{base}{suffix}"))
}

fn lexical_path_components(path: &Path) -> anyhow::Result<Vec<String>> {
    Ok(absolute_lexical_path(path)?
        .components()
        .map(path_component_key)
        .collect())
}

fn canonical_path_components(path: &Path) -> anyhow::Result<Vec<String>> {
    let path = canonicalize_existing_prefix(&absolute_path(path)?);
    Ok(path.components().map(path_component_key).collect())
}

fn paths_overlap(path: &[String], other: &[String]) -> bool {
    components_start_with(path, other) || components_start_with(other, path)
}

fn components_start_with(path: &[String], prefix: &[String]) -> bool {
    prefix.len() <= path.len()
        && path
            .iter()
            .zip(prefix)
            .all(|(component, prefix_component)| component == prefix_component)
}

fn absolute_lexical_path(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(lexical_clean_path(&absolute_path(path)?))
}

fn absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    })
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
    let mut normalized = fs::canonicalize(&existing_prefix).unwrap_or(existing_prefix);
    for component in missing_components.iter().rev() {
        normalized.push(component);
    }
    lexical_clean_path(&normalized)
}

fn lexical_clean_path(path: &Path) -> PathBuf {
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => clean.push(prefix.as_os_str()),
            Component::RootDir => clean.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = clean.pop();
            }
            Component::Normal(part) => clean.push(part),
        }
    }
    clean
}

fn path_component_key(component: Component<'_>) -> String {
    let value = component.as_os_str().to_string_lossy();
    if cfg!(windows) || cfg!(target_os = "macos") {
        value.to_lowercase()
    } else {
        value.into_owned()
    }
}

fn duplicate_decision(
    explicit: Option<DuplicateDecision>,
    json: bool,
    stdin_is_terminal: bool,
    preflight: &DownloadPreflight,
) -> anyhow::Result<DuplicateDecision> {
    if let Some(decision) = explicit {
        return Ok(decision);
    }
    if !preflight.requires_decision() {
        return Ok(DuplicateDecision::Replace);
    }
    if json || !stdin_is_terminal {
        bail!(
            "download archive found an existing record or output conflict; pass --on-duplicate replace, keep-both, or cancel"
        );
    }
    prompt_duplicate_decision(preflight)
}

fn should_prompt_duplicate_decision(
    explicit: Option<DuplicateDecision>,
    json: bool,
    preflight: &DownloadPreflight,
    stdin_is_terminal: bool,
) -> bool {
    explicit.is_none() && preflight.requires_decision() && !json && stdin_is_terminal
}

fn prompt_duplicate_decision(preflight: &DownloadPreflight) -> anyhow::Result<DuplicateDecision> {
    print_duplicate_preflight(preflight);
    eprintln!("Choose action: [r]eplace, [k]eep-both, [c]ancel");
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("failed to read duplicate decision")?;
    match answer.trim().to_ascii_lowercase().as_str() {
        "r" | "replace" => Ok(DuplicateDecision::Replace),
        "k" | "keep" | "keep-both" | "keep_both" => Ok(DuplicateDecision::KeepBoth),
        "c" | "cancel" => Ok(DuplicateDecision::Cancel),
        other => bail!("unsupported duplicate decision `{other}`"),
    }
}

fn print_duplicate_preflight(preflight: &DownloadPreflight) {
    eprintln!("possible duplicate download: {}", preflight.title);
    eprintln!("planned output: {}", preflight.planned_output_dir.display());
    if let Some(conflict) = &preflight.output_conflict {
        eprintln!("output already exists: {}", conflict.path.display());
    }
    for record in &preflight.archived_records {
        eprintln!(
            "archive record: {} completed_at={} entries={}",
            record.output_dir.display(),
            record.completed_at_unix,
            record.entries.len()
        );
    }
}

fn client_config(
    endpoints: EndpointConfig,
    restricted_area: RestrictedAreaConfig,
    request_timeout: Duration,
    credentials: Credentials,
) -> ClientConfig {
    ClientConfig::new(endpoints, credentials)
        .with_restricted_area(restricted_area)
        .with_user_agent("bbdown-rs/0.1")
        .with_request_timeout(request_timeout)
}

async fn handle_auth(
    command: AuthCommand,
    store: &CredentialStore,
    endpoints: EndpointConfig,
    restricted_area: RestrictedAreaConfig,
    request_timeout: Duration,
) -> anyhow::Result<()> {
    match command {
        AuthCommand::Status => {
            let credentials = store.load().context("failed to load credentials")?;
            println!(
                "{}",
                serde_json::to_string_pretty(&credentials.redacted_summary())?
            );
        }
        AuthCommand::ImportCookie(args) => {
            let mut credentials = store.load().context("failed to load credentials")?;
            let cookie = read_secret(args, "BBDOWN_COOKIE", "cookie")?;
            credentials.cookie = Some(cookie);
            store
                .save(&credentials)
                .context("failed to save credentials")?;
            println!("cookie imported");
        }
        AuthCommand::ImportAccessKey(args) => {
            let mut credentials = store.load().context("failed to load credentials")?;
            let access_key = read_secret(args, "BBDOWN_ACCESS_KEY", "access key")?;
            credentials.access_key = Some(access_key);
            store
                .save(&credentials)
                .context("failed to save credentials")?;
            println!("access key imported");
        }
        AuthCommand::LoginWeb(args) => {
            handle_qr_login(
                QrLoginKind::Web,
                args,
                store,
                endpoints,
                restricted_area,
                request_timeout,
            )
            .await?;
        }
        AuthCommand::LoginTv(args) => {
            handle_qr_login(
                QrLoginKind::Tv,
                args,
                store,
                endpoints,
                restricted_area,
                request_timeout,
            )
            .await?;
        }
        AuthCommand::Logout => {
            store.clear().context("failed to clear credentials")?;
            println!("credentials cleared");
        }
    }
    Ok(())
}

async fn handle_qr_login(
    kind: QrLoginKind,
    args: QrLoginArgs,
    store: &CredentialStore,
    endpoints: EndpointConfig,
    restricted_area: RestrictedAreaConfig,
    request_timeout: Duration,
) -> anyhow::Result<()> {
    ensure!(
        args.timeout_seconds > 0,
        "--timeout-seconds must be greater than 0"
    );
    ensure!(
        args.poll_interval_seconds > 0,
        "--poll-interval-seconds must be greater than 0"
    );
    let client = BiliClient::new(client_config(
        endpoints,
        restricted_area,
        request_timeout,
        Credentials::default(),
    ));
    let ticket = match kind {
        QrLoginKind::Web => client.create_web_qr_login().await?,
        QrLoginKind::Tv => client.create_tv_qr_login().await?,
    };
    if args.json {
        print_json_line(&serde_json::json!({
            "event": "ticket",
            "kind": kind,
            "url": ticket.url,
        }))?;
    } else {
        print_human_line(format_args!("scan: {}", ticket.url))?;
    }
    let credentials = wait_for_qr_login(&client, &ticket, &args).await?;
    let summary = save_qr_credentials(store, credentials)?;
    if args.json {
        print_json_line(&serde_json::json!({
            "event": "saved",
            "kind": kind,
            "saved": summary,
        }))?;
    } else {
        print_human_line("credentials saved")?;
    }
    Ok(())
}

fn save_qr_credentials(
    store: &CredentialStore,
    credentials: Credentials,
) -> anyhow::Result<bbdown_core::CredentialSource> {
    let mut stored = store.load().context("failed to load credentials")?;
    merge_credentials(&mut stored, credentials);
    store.save(&stored).context("failed to save credentials")?;
    Ok(stored.redacted_summary())
}

fn merge_credentials(stored: &mut Credentials, credentials: Credentials) {
    if credentials.cookie.is_some() {
        stored.cookie = credentials.cookie;
    }
    if credentials.access_key.is_some() {
        stored.access_key = credentials.access_key;
    }
    if credentials.tv_access_key.is_some() {
        stored.tv_access_key = credentials.tv_access_key;
    }
}

async fn wait_for_qr_login(
    client: &BiliClient,
    ticket: &QrLoginTicket,
    args: &QrLoginArgs,
) -> anyhow::Result<Credentials> {
    let interval = Duration::from_secs(args.poll_interval_seconds);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(args.timeout_seconds))
        .context("--timeout-seconds is too large")?;
    let mut last_waiting_state: Option<&'static str> = None;
    loop {
        let poll_timeout =
            remaining_until(Instant::now(), deadline).context("QR login timed out")?;
        let state = poll_qr_login(client, ticket, poll_timeout).await?;
        match state {
            QrLoginState::WaitingForScan => {
                if !args.json && last_waiting_state != Some("waiting_for_scan") {
                    print_human_line("waiting for scan")?;
                }
                last_waiting_state = Some("waiting_for_scan");
            }
            QrLoginState::WaitingForConfirm => {
                if !args.json && last_waiting_state != Some("waiting_for_confirm") {
                    print_human_line("waiting for confirmation")?;
                }
                last_waiting_state = Some("waiting_for_confirm");
            }
            QrLoginState::Expired => bail!("QR code expired"),
            QrLoginState::Succeeded { credentials } => return Ok(credentials),
        }
        let sleep_duration =
            next_poll_sleep(Instant::now(), deadline, interval).context("QR login timed out")?;
        tokio::time::sleep(sleep_duration).await;
    }
}

async fn poll_qr_login(
    client: &BiliClient,
    ticket: &QrLoginTicket,
    timeout: Duration,
) -> anyhow::Result<QrLoginState> {
    tokio::time::timeout(timeout, async {
        match ticket.kind {
            QrLoginKind::Web => client.poll_web_qr_login(&ticket.key).await,
            QrLoginKind::Tv => client.poll_tv_qr_login(ticket).await,
        }
    })
    .await
    .context("QR login timed out")?
    .map_err(Into::into)
}

fn remaining_until(now: Instant, deadline: Instant) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(now)?;
    if remaining.is_zero() {
        None
    } else {
        Some(remaining)
    }
}

fn next_poll_sleep(now: Instant, deadline: Instant, interval: Duration) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(now)?;
    if remaining.is_zero() {
        None
    } else {
        Some(remaining.min(interval))
    }
}

fn print_json_line(value: &serde_json::Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string(value)?);
    flush_stdout()?;
    Ok(())
}

fn print_human_line(message: impl std::fmt::Display) -> anyhow::Result<()> {
    println!("{message}");
    flush_stdout()?;
    Ok(())
}

fn flush_stdout() -> anyhow::Result<()> {
    use std::io::Write as _;
    std::io::stdout()
        .flush()
        .context("failed to flush stdout")?;
    Ok(())
}

fn endpoints_from_cli(cli: &Cli) -> EndpointConfig {
    let default_endpoints = EndpointConfig::default();
    let tv_passport_base = cli
        .tv_passport_base
        .clone()
        .unwrap_or_else(|| default_endpoints.tv_passport_base.clone());
    let tv_passport_poll_base = cli.tv_passport_poll_base.clone().unwrap_or_else(|| {
        if cli.tv_passport_base.is_some() {
            tv_passport_base.clone()
        } else {
            default_endpoints.tv_passport_poll_base.clone()
        }
    });
    EndpointConfig::default()
        .with_api_base(cli.api_base.clone())
        .with_pgc_base(cli.pgc_base.clone())
        .with_intl_base(cli.intl_base.clone())
        .with_comment_base(cli.comment_base.clone())
        .with_passport_base(cli.passport_base.clone())
        .with_tv_passport_base(tv_passport_base)
        .with_tv_passport_poll_base(tv_passport_poll_base)
}

#[derive(Clone, Debug)]
struct RestrictedProxyArg {
    kind: RestrictedAreaProxyKind,
    spec: String,
    order_priority: u8,
}

fn restricted_area_from_cli_with_args(
    cli: &Cli,
    raw_args: impl IntoIterator<Item = OsString>,
) -> anyhow::Result<RestrictedAreaConfig> {
    let env_area_proxy = std::env::var("BBDOWN_RESTRICTED_AREA_PROXY").ok();
    let env_api_proxy = std::env::var("BBDOWN_RESTRICTED_API_PROXY").ok();
    restricted_area_from_cli_with_env_values(
        cli,
        raw_args,
        env_area_proxy.as_deref(),
        env_api_proxy.as_deref(),
    )
}

fn restricted_area_from_cli_with_env_values(
    cli: &Cli,
    raw_args: impl IntoIterator<Item = OsString>,
    env_area_proxy: Option<&str>,
    env_api_proxy: Option<&str>,
) -> anyhow::Result<RestrictedAreaConfig> {
    let area_hint = cli
        .restricted_area
        .as_deref()
        .map(parse_restricted_area)
        .transpose()?;
    let proxy_args = proxy_args_from_sources(cli, raw_args, env_area_proxy, env_api_proxy)?;
    let proxies = proxy_args
        .iter()
        .map(|arg| parse_restricted_proxy_spec(&arg.spec, arg.kind, arg.order_priority))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(RestrictedAreaConfig::new(area_hint, proxies))
}

fn proxy_args_from_sources(
    cli: &Cli,
    raw_args: impl IntoIterator<Item = OsString>,
    env_area_proxy: Option<&str>,
    env_api_proxy: Option<&str>,
) -> anyhow::Result<Vec<RestrictedProxyArg>> {
    let mut proxy_args = proxy_args_from_raw_args(raw_args)?;
    proxy_args.extend(proxy_args_from_env_values(env_area_proxy, env_api_proxy));
    if proxy_args.is_empty() {
        proxy_args = proxy_args_from_cli_fields(cli);
    }
    Ok(proxy_args)
}

fn proxy_args_from_cli_fields(cli: &Cli) -> Vec<RestrictedProxyArg> {
    cli.restricted_area_proxy
        .iter()
        .filter(|spec| !spec.trim().is_empty())
        .map(|spec| RestrictedProxyArg {
            kind: RestrictedAreaProxyKind::PlayUrl,
            spec: spec.clone(),
            order_priority: 0,
        })
        .chain(
            cli.restricted_api_proxy
                .iter()
                .filter(|spec| !spec.trim().is_empty())
                .map(|spec| RestrictedProxyArg {
                    kind: RestrictedAreaProxyKind::BilibiliApi,
                    spec: spec.clone(),
                    order_priority: 0,
                }),
        )
        .collect()
}

fn proxy_args_from_env_values(
    area_proxy: Option<&str>,
    api_proxy: Option<&str>,
) -> Vec<RestrictedProxyArg> {
    let mut proxy_args = Vec::new();
    if let Some(value) = area_proxy.filter(|value| !value.trim().is_empty()) {
        push_proxy_env_arg_values(&mut proxy_args, RestrictedAreaProxyKind::PlayUrl, value, 1);
    }
    if let Some(value) = api_proxy.filter(|value| !value.trim().is_empty()) {
        push_proxy_env_arg_values(
            &mut proxy_args,
            RestrictedAreaProxyKind::BilibiliApi,
            value,
            2,
        );
    }
    proxy_args
}

fn proxy_args_from_raw_args(
    raw_args: impl IntoIterator<Item = OsString>,
) -> anyhow::Result<Vec<RestrictedProxyArg>> {
    let mut args = raw_args.into_iter().skip(1).peekable();
    let mut proxy_args = Vec::new();
    while let Some(arg) = args.next() {
        if let Some(value) = os_str_strip_prefix(&arg, "--restricted-area-proxy=") {
            push_proxy_arg_values(&mut proxy_args, RestrictedAreaProxyKind::PlayUrl, value, 0);
        } else if let Some(value) = os_str_strip_prefix(&arg, "--restricted-api-proxy=") {
            push_proxy_arg_values(
                &mut proxy_args,
                RestrictedAreaProxyKind::BilibiliApi,
                value,
                0,
            );
        } else if arg == OsStr::new("--restricted-area-proxy") {
            if let Some(value) = args.next() {
                let value = value.into_string().map_err(|_| {
                    anyhow::anyhow!("restricted-area proxy URL must be valid UTF-8")
                })?;
                push_proxy_arg_values(&mut proxy_args, RestrictedAreaProxyKind::PlayUrl, &value, 0);
            }
        } else if arg == OsStr::new("--restricted-api-proxy")
            && let Some(value) = args.next()
        {
            let value = value
                .into_string()
                .map_err(|_| anyhow::anyhow!("restricted-area proxy URL must be valid UTF-8"))?;
            push_proxy_arg_values(
                &mut proxy_args,
                RestrictedAreaProxyKind::BilibiliApi,
                &value,
                0,
            );
        }
    }
    Ok(proxy_args)
}

fn os_str_strip_prefix<'a>(value: &'a OsStr, prefix: &str) -> Option<&'a str> {
    value.to_str()?.strip_prefix(prefix)
}

fn push_proxy_arg_values(
    proxy_args: &mut Vec<RestrictedProxyArg>,
    kind: RestrictedAreaProxyKind,
    value: &str,
    order_priority: u8,
) {
    proxy_args.extend(value.split(',').map(|spec| RestrictedProxyArg {
        kind,
        spec: spec.to_owned(),
        order_priority,
    }));
}

fn push_proxy_env_arg_values(
    proxy_args: &mut Vec<RestrictedProxyArg>,
    kind: RestrictedAreaProxyKind,
    value: &str,
    order_priority: u8,
) {
    proxy_args.extend(
        value
            .split(',')
            .filter(|spec| !spec.trim().is_empty())
            .map(|spec| RestrictedProxyArg {
                kind,
                spec: spec.to_owned(),
                order_priority,
            }),
    );
}

fn parse_restricted_proxy_spec(
    spec: &str,
    kind: RestrictedAreaProxyKind,
    order_priority: u8,
) -> anyhow::Result<RestrictedAreaProxy> {
    let trimmed = spec.trim();
    ensure!(!trimmed.is_empty(), "restricted-area proxy cannot be empty");
    let (area, base_url) = if let Some((area, base_url)) = parse_area_prefixed_proxy(trimmed)? {
        (Some(parse_restricted_area(area)?), base_url.trim())
    } else {
        (None, trimmed)
    };
    ensure!(
        !base_url.is_empty(),
        "restricted-area proxy URL cannot be empty"
    );
    let parsed = url::Url::parse(base_url).with_context(|| {
        format!(
            "failed to parse restricted-area proxy URL `{}`",
            redact_cli_url_for_error(base_url)
        )
    })?;
    ensure!(
        matches!(parsed.scheme(), "http" | "https"),
        "restricted-area proxy URL `{}` must use http or https",
        redact_cli_url_for_error(base_url)
    );
    Ok(match kind {
        RestrictedAreaProxyKind::PlayUrl => RestrictedAreaProxy::playurl(base_url, area),
        RestrictedAreaProxyKind::BilibiliApi => RestrictedAreaProxy::bilibili_api(base_url, area),
    }
    .with_order_priority(order_priority))
}

fn parse_area_prefixed_proxy(spec: &str) -> anyhow::Result<Option<(&str, &str)>> {
    if starts_with_url_scheme(spec) {
        return Ok(None);
    }
    let Some((area, base_url)) = spec.split_once('=') else {
        return Ok(None);
    };
    match area.trim().to_ascii_lowercase().as_str() {
        "cn" | "th" | "hk" | "tw" => Ok(Some((area, base_url))),
        other => bail!("unsupported restricted area `{other}`; expected cn, th, hk, or tw"),
    }
}

fn starts_with_url_scheme(value: &str) -> bool {
    let Some(scheme_end) = value.find("://") else {
        return false;
    };
    let scheme = &value[..scheme_end];
    scheme
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn parse_restricted_area(value: &str) -> anyhow::Result<RestrictedArea> {
    match value.trim().to_ascii_lowercase().as_str() {
        "cn" => Ok(RestrictedArea::Cn),
        "th" => Ok(RestrictedArea::Th),
        "hk" => Ok(RestrictedArea::Hk),
        "tw" => Ok(RestrictedArea::Tw),
        other => bail!("unsupported restricted area `{other}`; expected cn, th, hk, or tw"),
    }
}

fn redact_cli_url_for_error(raw: &str) -> String {
    url::Url::parse(raw).map_or_else(
        |_| redact_unparsed_cli_url_for_error(raw),
        |mut url| {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_path("");
            url.set_query(None);
            url.set_fragment(None);
            url.to_string().trim_end_matches('/').to_owned()
        },
    )
}

fn redact_unparsed_cli_url_for_error(raw: &str) -> String {
    let without_query = raw
        .split(|character: char| character.is_whitespace() || matches!(character, '?' | '#'))
        .next()
        .unwrap_or(raw);
    let Some(scheme_end) = without_query.find("//") else {
        return "<invalid-url>".to_owned();
    };
    let prefix = &without_query[..(scheme_end + 2)];
    let after_scheme = &without_query[(scheme_end + 2)..];
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if host.is_empty() {
        "<invalid-url>".to_owned()
    } else {
        format!("{prefix}{host}")
    }
}

fn read_secret(
    args: SecretImportArgs,
    env_key: &'static str,
    label: &str,
) -> anyhow::Result<String> {
    let raw = if args.stdin {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .with_context(|| format!("failed to read {label} from stdin"))?;
        buffer
    } else if let Some(path) = args.file {
        fs::read_to_string(&path)
            .with_context(|| format!("failed to read {label} from {}", path.display()))?
    } else if let Ok(value) = std::env::var(env_key) {
        value
    } else {
        bail!("provide {label} through --stdin, --file, or {env_key}");
    };
    let value = raw.trim_end_matches(['\r', '\n']).to_owned();
    ensure!(!value.is_empty(), "{label} is empty");
    Ok(value)
}

fn credential_path(path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(path) = path {
        return Ok(path);
    }
    let base = dirs::config_dir()
        .or_else(|| std::env::current_dir().ok())
        .context("failed to determine config directory")?;
    Ok(base.join("bbdown-rs").join("credentials.json"))
}

fn print_human_summary(resolved: &ResolvedContent) {
    match resolved {
        ResolvedContent::Video(video) => {
            println!("title: {}", video.title);
            println!("aid: {}", video.aid);
            if let Some(owner) = &video.owner {
                println!("owner: {} ({})", owner.name, owner.mid);
            }
            if !video.tags.is_empty() {
                let tags = video
                    .tags
                    .iter()
                    .map(|tag| tag.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("tags: {tags}");
            }
            println!("pages: {}", video.pages.len());
        }
        ResolvedContent::Season(season) => {
            println!("title: {}", season.season.title);
            println!("episodes: {}", season.season.episodes.len());
            println!("selected: {}", season.selected_episodes.len());
        }
        ResolvedContent::Collection(collection) => {
            println!("title: {}", collection.collection.title);
            println!("kind: {:?}", collection.collection.kind);
            println!("items: {}", collection.collection.items.len());
            println!("selected: {}", collection.selected_items.len());
        }
    }
}

fn print_download_report(report: &DownloadReport) {
    println!("title: {}", report.title);
    println!("output: {}", report.output_dir.display());
    println!("entries: {}", report.entries.len());
    for entry in &report.entries {
        println!(
            "- P{} files={} dir={}",
            entry.index,
            entry.files.len(),
            entry.directory.display()
        );
        if let Some(mux) = &entry.mux {
            println!("  mux: {}", mux.output_path.display());
        }
    }
}

#[allow(dead_code)]
fn _assert_credentials_send_sync(_: Credentials) {}

#[cfg(test)]
mod tests {
    use super::{
        Cli, archive_sidecar_path, endpoints_from_cli, ensure_archive_file_is_not_output_root,
        next_poll_sleep, remaining_until, restricted_area_from_cli_with_args,
        restricted_area_from_cli_with_env_values, save_qr_credentials,
        should_prompt_duplicate_decision,
    };
    use bbdown_core::{
        CredentialStore, Credentials, DownloadOutputConflict, DownloadPreflight, DuplicateDecision,
        EndpointConfig,
    };
    use clap::Parser as _;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    #[test]
    fn next_poll_sleep_caps_interval_by_deadline() {
        let now = Instant::now();
        assert_eq!(
            next_poll_sleep(now, now + Duration::from_secs(119), Duration::from_secs(61)),
            Some(Duration::from_secs(61))
        );
        assert_eq!(
            next_poll_sleep(
                now + Duration::from_secs(61),
                now + Duration::from_secs(119),
                Duration::from_secs(61),
            ),
            Some(Duration::from_secs(58))
        );
        assert_eq!(
            next_poll_sleep(
                now + Duration::from_secs(119),
                now + Duration::from_secs(119),
                Duration::from_secs(61),
            ),
            None
        );
    }

    #[test]
    fn remaining_until_uses_total_deadline_without_poll_interval_cap() {
        let now = Instant::now();
        assert_eq!(
            remaining_until(now, now + Duration::from_secs(119)),
            Some(Duration::from_secs(119))
        );
        assert_eq!(remaining_until(now, now), None);
    }

    #[test]
    fn archive_file_guard_rejects_output_root_overlap() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let output_root = temp.path().join("downloads").join("Mock video");
        let output_parent = output_root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing output parent"))?;
        let same_output_root = output_root.join(".");
        let nested_archive = output_root.join("archive.json");
        let sibling_archive = output_parent.join("archive.json");
        #[cfg(any(windows, target_os = "macos"))]
        let case_variant_archive = output_parent.join("mock video").join("archive.json");

        assert!(ensure_archive_file_is_not_output_root(&same_output_root, &output_root).is_err());
        assert!(ensure_archive_file_is_not_output_root(&nested_archive, &output_root).is_err());
        assert!(ensure_archive_file_is_not_output_root(output_parent, &output_root).is_err());
        #[cfg(any(windows, target_os = "macos"))]
        assert!(
            ensure_archive_file_is_not_output_root(&case_variant_archive, &output_root).is_err()
        );
        assert!(ensure_archive_file_is_not_output_root(&sibling_archive, &output_root).is_ok());
        Ok(())
    }

    #[test]
    fn archive_file_guard_rejects_sidecar_output_root_overlap() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let archive_file = temp.path().join("downloads").join("archive.json");
        let temporary_output_root = archive_sidecar_path(&archive_file, ".bbdown-archive-tmp");
        let backup_output_root = archive_sidecar_path(&archive_file, ".bbdown-archive-backup");
        let unrelated_output_root = temp.path().join("downloads").join("Mock video");

        assert!(
            ensure_archive_file_is_not_output_root(&archive_file, &temporary_output_root).is_err()
        );
        assert!(
            ensure_archive_file_is_not_output_root(&archive_file, &backup_output_root).is_err()
        );
        assert!(
            ensure_archive_file_is_not_output_root(&archive_file, &unrelated_output_root).is_ok()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn archive_file_guard_rejects_lexical_symlink_inside_output_root() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let output_root = temp.path().join("downloads").join("Mock video");
        fs::create_dir_all(&output_root)?;
        let external_archive = temp.path().join("external").join("archive.json");
        let external_parent = external_archive
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing external archive parent"))?;
        fs::create_dir_all(external_parent)?;
        fs::write(&external_archive, "{}")?;
        let archive_symlink = output_root.join("archive.json");
        std::os::unix::fs::symlink(&external_archive, &archive_symlink)?;

        assert!(ensure_archive_file_is_not_output_root(&archive_symlink, &output_root).is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn archive_file_guard_resolves_symlink_before_parent_components() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let output_root = temp.path().join("downloads").join("Mock video");
        let output_subdir = output_root.join("subdir");
        fs::create_dir_all(&output_subdir)?;
        let external_parent = temp.path().join("external");
        fs::create_dir_all(&external_parent)?;
        let link_to_output_subdir = external_parent.join("link");
        std::os::unix::fs::symlink(&output_subdir, &link_to_output_subdir)?;
        let archive_through_symlink_parent = link_to_output_subdir.join("..").join("archive.json");

        assert!(
            ensure_archive_file_is_not_output_root(&archive_through_symlink_parent, &output_root)
                .is_err()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn archive_file_guard_rejects_symlink_target_sidecar_overlap() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let shared_dir = temp.path().join("shared");
        fs::create_dir_all(&shared_dir)?;
        let archive_target = shared_dir.join("archive.json");
        fs::write(&archive_target, "{\"records\":[]}")?;
        let archive_link = temp.path().join("archive-link.json");
        std::os::unix::fs::symlink(&archive_target, &archive_link)?;
        let target_temporary_sidecar = archive_sidecar_path(&archive_target, ".bbdown-archive-tmp");

        assert!(
            ensure_archive_file_is_not_output_root(&archive_link, &target_temporary_sidecar)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn duplicate_decision_prompt_state_tracks_displayed_preflight() {
        let preflight = DownloadPreflight {
            content_key: "plan|aid=1|cid=2".to_owned(),
            title: "Mock video".to_owned(),
            planned_output_dir: PathBuf::from("Mock video"),
            archived_records: Vec::new(),
            output_conflict: Some(DownloadOutputConflict {
                path: PathBuf::from("Mock video"),
            }),
            reserved_output_dirs: Vec::new(),
        };
        let clean_preflight = DownloadPreflight {
            output_conflict: None,
            ..preflight.clone()
        };

        assert!(should_prompt_duplicate_decision(
            None, false, &preflight, true
        ));
        assert!(!should_prompt_duplicate_decision(
            Some(DuplicateDecision::Cancel),
            false,
            &preflight,
            true
        ));
        assert!(!should_prompt_duplicate_decision(
            None, true, &preflight, true
        ));
        assert!(!should_prompt_duplicate_decision(
            None, false, &preflight, false
        ));
        assert!(!should_prompt_duplicate_decision(
            None,
            false,
            &clean_preflight,
            true
        ));
    }

    #[test]
    fn passport_base_does_not_override_default_tv_poll_base() {
        let cli = Cli::parse_from([
            "bbdown",
            "--passport-base",
            "http://127.0.0.1:8080",
            "auth",
            "status",
        ]);
        let endpoints = endpoints_from_cli(&cli);
        let defaults = EndpointConfig::default();

        assert_eq!(endpoints.passport_base, "http://127.0.0.1:8080");
        assert_eq!(endpoints.tv_passport_base, defaults.tv_passport_base);
        assert_eq!(
            endpoints.tv_passport_poll_base,
            defaults.tv_passport_poll_base
        );
    }

    #[test]
    fn tv_passport_base_controls_tv_poll_when_poll_base_is_implicit() {
        let cli = Cli::parse_from([
            "bbdown",
            "--tv-passport-base",
            "http://127.0.0.1:8080",
            "auth",
            "status",
        ]);
        let endpoints = endpoints_from_cli(&cli);

        assert_eq!(endpoints.tv_passport_base, "http://127.0.0.1:8080");
        assert_eq!(endpoints.tv_passport_poll_base, "http://127.0.0.1:8080");
    }

    #[test]
    fn explicit_tv_passport_poll_base_wins() {
        let cli = Cli::parse_from([
            "bbdown",
            "--tv-passport-base",
            "http://127.0.0.1:8080",
            "--tv-passport-poll-base",
            "http://127.0.0.1:8081",
            "auth",
            "status",
        ]);
        let endpoints = endpoints_from_cli(&cli);

        assert_eq!(endpoints.tv_passport_base, "http://127.0.0.1:8080");
        assert_eq!(endpoints.tv_passport_poll_base, "http://127.0.0.1:8081");
    }

    #[test]
    fn restricted_area_cli_builds_proxy_chain() -> anyhow::Result<()> {
        let args = [
            "bbdown",
            "--restricted-area",
            "hk",
            "--restricted-area-proxy",
            "https://generic.example/playurl",
            "--restricted-api-proxy",
            "tw=https://tw.example/api",
            "--restricted-api-proxy",
            "hk=https://hk.example/api",
            "auth",
            "status",
        ];
        let cli = Cli::parse_from(args);
        let config = restricted_area_from_cli_with_args(&cli, args.map(std::ffi::OsString::from))?;
        let ordered = config.ordered_proxies();

        assert_eq!(ordered[0].base_url, "https://hk.example/api");
        assert_eq!(ordered[1].base_url, "https://generic.example/playurl");
        assert_eq!(ordered[2].base_url, "https://tw.example/api");
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_bare_url_may_contain_query_equals() -> anyhow::Result<()> {
        let args = [
            "bbdown",
            "--restricted-area-proxy",
            "https://generic.example/playurl?token=a=b",
            "auth",
            "status",
        ];
        let cli = Cli::parse_from(args);
        let config = restricted_area_from_cli_with_args(&cli, args.map(std::ffi::OsString::from))?;

        assert_eq!(config.proxies[0].area, None);
        assert_eq!(
            config.proxies[0].base_url,
            "https://generic.example/playurl?token=a=b"
        );
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_uppercase_scheme_may_contain_query_equals() -> anyhow::Result<()> {
        let args = [
            "bbdown",
            "--restricted-area-proxy",
            "HTTPS://generic.example/playurl?token=a=b",
            "auth",
            "status",
        ];
        let cli = Cli::parse_from(args);
        let config = restricted_area_from_cli_with_args(&cli, args.map(std::ffi::OsString::from))?;

        assert_eq!(config.proxies[0].area, None);
        assert_eq!(
            config.proxies[0].base_url,
            "HTTPS://generic.example/playurl?token=a=b"
        );
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_preserves_cross_flag_cli_order() -> anyhow::Result<()> {
        let args = [
            "bbdown",
            "--restricted-area",
            "hk",
            "--restricted-api-proxy",
            "hk=https://api.example/base",
            "--restricted-area-proxy",
            "hk=https://play.example/playurl",
            "auth",
            "status",
        ];
        let cli = Cli::parse_from(args);
        let config = restricted_area_from_cli_with_args(&cli, args.map(std::ffi::OsString::from))?;
        let ordered = config.ordered_proxies();

        assert_eq!(ordered[0].base_url, "https://api.example/base");
        assert_eq!(ordered[1].base_url, "https://play.example/playurl");
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_merges_cli_and_env_values() -> anyhow::Result<()> {
        let args = [
            "bbdown",
            "--restricted-area",
            "hk",
            "--restricted-area-proxy",
            "hk=https://cli-play.example/playurl",
            "auth",
            "status",
        ];
        let cli = Cli::parse_from(args);
        let config = restricted_area_from_cli_with_env_values(
            &cli,
            args.map(std::ffi::OsString::from),
            None,
            Some("hk=https://env-api.example/api"),
        )?;
        let ordered = config.ordered_proxies();

        assert_eq!(ordered[0].base_url, "https://cli-play.example/playurl");
        assert_eq!(ordered[1].base_url, "https://env-api.example/api");
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_keeps_cli_source_before_env_area_match() -> anyhow::Result<()> {
        let args = [
            "bbdown",
            "--restricted-area",
            "hk",
            "--restricted-area-proxy",
            "https://cli-play.example/playurl",
            "auth",
            "status",
        ];
        let cli = Cli::parse_from(args);
        let config = restricted_area_from_cli_with_env_values(
            &cli,
            args.map(std::ffi::OsString::from),
            Some("hk=https://env-play.example/playurl"),
            None,
        )?;
        let ordered = config.ordered_proxies();

        assert_eq!(ordered[0].base_url, "https://cli-play.example/playurl");
        assert_eq!(ordered[1].base_url, "https://env-play.example/playurl");
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_keeps_env_playurl_before_env_api_area_match() -> anyhow::Result<()> {
        let args = ["bbdown", "--restricted-area", "hk", "auth", "status"];
        let cli = Cli::parse_from(args);
        let config = restricted_area_from_cli_with_env_values(
            &cli,
            args.map(std::ffi::OsString::from),
            Some("https://env-play.example/playurl"),
            Some("hk=https://env-api.example/api"),
        )?;
        let ordered = config.ordered_proxies();

        assert_eq!(ordered[0].base_url, "https://env-play.example/playurl");
        assert_eq!(ordered[1].base_url, "https://env-api.example/api");
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_ignores_empty_env_values() -> anyhow::Result<()> {
        let args = ["bbdown", "auth", "status"];
        let mut cli = Cli::parse_from(args);
        cli.restricted_area_proxy = vec![String::new(), String::new()];
        cli.restricted_api_proxy = vec![" ".to_owned(), " ".to_owned()];
        let config = restricted_area_from_cli_with_env_values(
            &cli,
            args.map(std::ffi::OsString::from),
            Some(","),
            Some(" , "),
        )?;

        assert!(config.proxies.is_empty());
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_ignores_trailing_empty_env_segment() -> anyhow::Result<()> {
        let args = ["bbdown", "auth", "status"];
        let cli = Cli::parse_from(args);
        let config = restricted_area_from_cli_with_env_values(
            &cli,
            args.map(std::ffi::OsString::from),
            Some("https://env-play.example/playurl,"),
            Some("hk=https://env-api.example/api, "),
        )?;
        let ordered = config.ordered_proxies();

        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].base_url, "https://env-play.example/playurl");
        assert_eq!(ordered[1].base_url, "https://env-api.example/api");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn restricted_area_raw_arg_scan_ignores_non_utf8_non_proxy_values() -> anyhow::Result<()> {
        use std::os::unix::ffi::OsStringExt;

        let cli = Cli::parse_from([
            "bbdown",
            "--credential-file",
            "placeholder",
            "auth",
            "status",
        ]);
        let raw_args = vec![
            std::ffi::OsString::from("bbdown"),
            std::ffi::OsString::from("--credential-file"),
            std::ffi::OsString::from_vec(vec![b'p', b'a', b't', b'h', 0xff]),
            std::ffi::OsString::from("auth"),
            std::ffi::OsString::from("status"),
        ];
        let config = restricted_area_from_cli_with_env_values(&cli, raw_args, None, None)?;

        assert!(config.proxies.is_empty());
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_parse_error_redacts_spec() {
        let args = [
            "bbdown",
            "--restricted-area-proxy",
            "hk=https://user:pass@proxy.example token=TOKEN_SECRET/t/PATH_SECRET?token=QUERY_SECRET",
            "auth",
            "status",
        ];
        let cli = Cli::parse_from(args);
        let error = restricted_area_from_cli_with_args(&cli, args.map(std::ffi::OsString::from))
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();

        assert!(error.contains("failed to parse restricted-area proxy URL"));
        for sensitive in [
            "user:pass",
            "TOKEN_SECRET",
            "PATH_SECRET",
            "QUERY_SECRET",
            "token=",
        ] {
            assert!(
                !error.contains(sensitive),
                "parse error leaked {sensitive}: {error}"
            );
        }
    }

    #[test]
    fn restricted_area_proxy_uppercase_scheme_parse_error_redacts_spec() {
        let args = [
            "bbdown",
            "--restricted-area-proxy",
            "HTTPS://user:pass@exa mple/t/PATH_SECRET?token=QUERY_SECRET",
            "auth",
            "status",
        ];
        let cli = Cli::parse_from(args);
        let error = restricted_area_from_cli_with_args(&cli, args.map(std::ffi::OsString::from))
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();

        assert!(error.contains("failed to parse restricted-area proxy URL"));
        for sensitive in ["user:pass", "PATH_SECRET", "QUERY_SECRET", "token="] {
            assert!(
                !error.contains(sensitive),
                "parse error leaked {sensitive}: {error}"
            );
        }
    }

    #[test]
    fn restricted_area_proxy_invalid_scheme_parse_error_redacts_spec() {
        let args = [
            "bbdown",
            "--restricted-area-proxy",
            "HTPS://user:pass@proxy.example/t/PATH_SECRET?token=QUERY_SECRET",
            "auth",
            "status",
        ];
        let cli = Cli::parse_from(args);
        let error = restricted_area_from_cli_with_args(&cli, args.map(std::ffi::OsString::from))
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();

        assert!(error.contains("must use http or https"));
        for sensitive in ["user:pass", "PATH_SECRET", "QUERY_SECRET", "token="] {
            assert!(
                !error.contains(sensitive),
                "parse error leaked {sensitive}: {error}"
            );
        }
    }

    #[test]
    fn save_qr_credentials_merges_with_current_store() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        store.save(&Credentials {
            cookie: Some("SESSDATA=fresh".to_owned()),
            access_key: Some("BSTAR".to_owned()),
            tv_access_key: None,
        })?;

        let summary = save_qr_credentials(
            &store,
            Credentials {
                cookie: None,
                access_key: None,
                tv_access_key: Some("TV".to_owned()),
            },
        )?;
        let saved = store.load()?;

        assert_eq!(
            saved,
            Credentials {
                cookie: Some("SESSDATA=fresh".to_owned()),
                access_key: Some("BSTAR".to_owned()),
                tv_access_key: Some("TV".to_owned()),
            }
        );
        assert!(summary.has_cookie);
        assert!(summary.has_access_key);
        assert!(summary.has_tv_access_key);
        Ok(())
    }
}

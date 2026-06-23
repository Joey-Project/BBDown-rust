#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

use anyhow::{Context, bail, ensure};
use bbdown_core::{
    AccessKeyAutomaticRefreshReadiness, AccessKeyLoginConfig, AccessKeyLoginCredentials,
    AccessKeyLoginTicketOutput, AccessKeyProvider, AccessKeyProviderSecret,
    AccessKeyRefreshKeypair, AccessKeyRefreshProvider, AccessKeyRefreshRequest,
    AccessKeyRenewalAction, AccessKeyRenewalDecision, AccessKeyRenewalReason, BiliClient,
    ClientConfig, CredentialHealthReport, CredentialHealthScope, CredentialHealthStatus,
    CredentialHealthSummaryStatus, CredentialKind, CredentialLifecycleMetadata,
    CredentialLifecyclePolicy, CredentialLifecycleSource, CredentialLifecycleStatus,
    CredentialPreflightMode, CredentialPreflightReport, CredentialPreflightRequestPath,
    CredentialPreflightRequirement, CredentialProfileLifecycleStatus, CredentialProfileSelection,
    CredentialProfiles, CredentialStore, Credentials, DanmakuFormat, DanmakuUpdateOptions,
    DownloadArchive, DownloadCancellationToken, DownloadMode, DownloadOptions,
    DownloadPathTemplates, DownloadPlan, DownloadPreflight, DownloadProgressEvent,
    DownloadProgressSink, DownloadReport, DuplicateDecision, EndpointConfig, Input,
    MediaHostOptions, MediaStream, MuxOptions, PlaybackPlan, PlayurlMode, QrLoginKind,
    QrLoginState, QrLoginTicket, QrLoginTicketOutput, ResolvedContent, RestrictedArea,
    RestrictedAreaConfig, RestrictedAreaProxy, RestrictedAreaProxyKind, RetryPolicy, Selection,
    StreamQuality, StreamSelection, StreamSet, SubtitleAiPolicy,
    archive_entry_allows_danmaku_update, credential_preflight_requirements_for_media_paths,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;

const DEFAULT_ACCESS_KEY_AUTH_BASE: &str = "https://www.biliplus.com";
const DEFAULT_ACCESS_KEY_CALLBACK_ORIGIN: &str = "https://www.bilibili.com";
const BALH_LOGIN_CREDENTIALS_PREFIX: &str = "balh-login-credentials:";

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
        env = "BBDOWN_INTL_PASSPORT_BASE",
        default_value = "https://passport.biliintl.com"
    )]
    intl_passport_base: String,
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
    #[arg(
        long,
        env = "BBDOWN_TV_API_BASE",
        default_value = "https://api.snm0516.aisee.tv"
    )]
    tv_api_base: String,
    #[arg(
        long,
        env = "BBDOWN_APP_GRPC_BASE",
        default_value = "https://grpc.biliapi.net"
    )]
    app_grpc_base: String,
    #[arg(
        long,
        env = "BBDOWN_APP_PGC_GRPC_BASE",
        default_value = "https://grpc.biliapi.net"
    )]
    app_pgc_grpc_base: String,
    #[arg(long, env = "BBDOWN_TV_PASSPORT_BASE")]
    tv_passport_base: Option<String>,
    #[arg(long, env = "BBDOWN_TV_PASSPORT_POLL_BASE")]
    tv_passport_poll_base: Option<String>,
    #[arg(long, env = "BBDOWN_PLAYURL_MODE", default_value = "web")]
    playurl_mode: PlayurlModeArg,
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
    #[arg(long, env = "BBDOWN_CREDENTIAL_PROFILE", value_name = "NAME")]
    credential_profile: Option<String>,
    #[arg(
        long,
        env = "BBDOWN_CREDENTIAL_PREFLIGHT",
        value_enum,
        default_value = "off",
        value_name = "MODE"
    )]
    credential_preflight: CredentialPreflightModeArg,
    #[arg(
        long,
        env = "BBDOWN_CREDENTIAL_STALE_AFTER_SECONDS",
        default_value_t = 7 * 24 * 60 * 60
    )]
    credential_stale_after_seconds: u64,
    #[arg(
        long,
        env = "BBDOWN_CREDENTIAL_EXPIRING_WITHIN_SECONDS",
        default_value_t = 24 * 60 * 60
    )]
    credential_expiring_within_seconds: u64,
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
    Playback {
        url: String,
        #[arg(long)]
        select: Option<Selection>,
        #[arg(long)]
        json: bool,
    },
    Download(Box<DownloadCliArgs>),
    Danmaku {
        #[command(subcommand)]
        command: DanmakuCommand,
    },
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
struct DownloadCliArgs {
    url: String,
    #[arg(long)]
    select: Option<Selection>,
    #[arg(long, default_value = ".")]
    output_dir: PathBuf,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    progress_json: bool,
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
    #[arg(long, value_enum, default_value = "include", value_name = "POLICY")]
    subtitle_ai: SubtitleAiPolicyArg,
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
    #[arg(long, value_name = "LANG")]
    audio_language: Option<String>,
    #[arg(long, value_name = "TEMPLATE")]
    output_template: Option<String>,
    #[arg(long, value_name = "TEMPLATE")]
    entry_template: Option<String>,
    #[arg(long, value_name = "TEMPLATE")]
    mux_template: Option<String>,
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
}

#[derive(Debug, Subcommand)]
enum DanmakuCommand {
    #[command(about = "Append-update danmaku sidecars for existing download archive entries")]
    Update(DanmakuUpdateCliArgs),
}

#[derive(Debug, Args)]
struct DanmakuUpdateCliArgs {
    url: String,
    #[arg(long)]
    select: Option<Selection>,
    #[arg(long, value_name = "PATH")]
    archive_file: PathBuf,
    #[arg(long)]
    json: bool,
    #[arg(long, default_value_t = 3)]
    retry_attempts: u32,
    #[arg(long, default_value_t = 250)]
    retry_backoff_ms: u64,
    #[arg(long, default_value_t = 30)]
    download_idle_timeout_seconds: u64,
    #[arg(
        long = "danmaku-format",
        value_enum,
        value_delimiter = ',',
        default_value = "xml",
        value_name = "FORMAT"
    )]
    danmaku_formats: Vec<DanmakuFormatArg>,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    Status(AuthStatusArgs),
    Health(AuthHealthArgs),
    ImportCookie(SecretImportArgs),
    ImportAccessKey(SecretImportArgs),
    LoginWeb(QrLoginArgs),
    LoginTv(QrLoginArgs),
    #[command(about = "Acquire a generic access key through a BiliPlus/BALH browser handoff")]
    LoginAccessKey(AccessKeyLoginArgs),
    #[command(about = "Plan or complete generic access-key reauthorization")]
    RenewAccessKey(AccessKeyRenewalArgs),
    Logout,
}

#[derive(Debug, Args)]
struct AuthStatusArgs {
    #[arg(long)]
    profiles: bool,
    #[arg(long)]
    all_profiles: bool,
    #[arg(long, default_value_t = 7 * 24 * 60 * 60)]
    stale_after_seconds: u64,
    #[arg(long, default_value_t = 24 * 60 * 60)]
    expiring_within_seconds: u64,
}

#[derive(Debug, Args)]
struct AuthHealthArgs {
    #[arg(long)]
    json: bool,
    #[arg(long)]
    all_profiles: bool,
    #[arg(long, default_value_t = 7 * 24 * 60 * 60)]
    stale_after_seconds: u64,
    #[arg(long, default_value_t = 24 * 60 * 60)]
    expiring_within_seconds: u64,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum SubtitleAiPolicyArg {
    Include,
    PreferNonAi,
    ExcludeAi,
    OnlyAi,
}

impl From<SubtitleAiPolicyArg> for SubtitleAiPolicy {
    fn from(value: SubtitleAiPolicyArg) -> Self {
        match value {
            SubtitleAiPolicyArg::Include => Self::Include,
            SubtitleAiPolicyArg::PreferNonAi => Self::PreferNonAi,
            SubtitleAiPolicyArg::ExcludeAi => Self::ExcludeAi,
            SubtitleAiPolicyArg::OnlyAi => Self::OnlyAi,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum PlayurlModeArg {
    Web,
    Tv,
    App,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum CredentialPreflightModeArg {
    Off,
    Warn,
    Fail,
    Renew,
}

impl From<PlayurlModeArg> for PlayurlMode {
    fn from(value: PlayurlModeArg) -> Self {
        match value {
            PlayurlModeArg::Web => Self::Web,
            PlayurlModeArg::Tv => Self::Tv,
            PlayurlModeArg::App => Self::App,
        }
    }
}

impl From<CredentialPreflightModeArg> for CredentialPreflightMode {
    fn from(value: CredentialPreflightModeArg) -> Self {
        match value {
            CredentialPreflightModeArg::Off => Self::Off,
            CredentialPreflightModeArg::Warn => Self::Warn,
            CredentialPreflightModeArg::Fail => Self::Fail,
            CredentialPreflightModeArg::Renew => Self::Renew,
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

#[derive(Clone, Copy, Debug)]
struct SingleDownloadValidationArgs<'a> {
    only: Option<DownloadOnlyArg>,
    no_cover: bool,
    no_subtitles: bool,
    no_danmaku: bool,
    subtitle_ai: SubtitleAiPolicyArg,
    video_quality: Option<u32>,
    audio_quality: Option<u32>,
    audio_language: Option<&'a str>,
}

fn validate_single_download_args(args: SingleDownloadValidationArgs<'_>) -> anyhow::Result<()> {
    let SingleDownloadValidationArgs {
        only,
        no_cover,
        no_subtitles,
        no_danmaku,
        subtitle_ai,
        video_quality,
        audio_quality,
        audio_language,
    } = args;
    ensure!(
        subtitle_ai == SubtitleAiPolicyArg::Include
            || (!no_subtitles
                && (matches!(only, Some(DownloadOnlyArg::Subtitle)) || only.is_none())),
        "--subtitle-ai requires downloadable subtitles"
    );
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
        !(matches!(only, DownloadOnlyArg::Video) && audio_language.is_some()),
        "--only video conflicts with --audio-language"
    );
    ensure!(
        !(matches!(only, DownloadOnlyArg::Audio) && video_quality.is_some()),
        "--only audio conflicts with --video-quality"
    );
    ensure!(
        matches!(only, DownloadOnlyArg::Video | DownloadOnlyArg::Audio)
            || (video_quality.is_none() && audio_quality.is_none() && audio_language.is_none()),
        "stream selection requires --only video, --only audio, or the default download mode"
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
    audio_language: Option<String>,
    templates: DownloadTemplateCliFlags,
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
    subtitle_ai: SubtitleAiPolicyArg,
}

struct DownloadMediaHostCliFlags {
    upos_host: Option<String>,
    force_replace_host: bool,
    allow_pcdn: bool,
}

struct DownloadTemplateCliFlags {
    output: Option<String>,
    entry: Option<String>,
    mux: Option<String>,
}

fn download_options_from_cli(args: DownloadOptionCliArgs) -> anyhow::Result<DownloadOptions> {
    ensure!(
        args.retry_attempts > 0,
        "--retry-attempts must be greater than 0"
    );
    validate_single_download_args(SingleDownloadValidationArgs {
        only: args.only,
        no_cover: args.sidecars.no_cover,
        no_subtitles: args.sidecars.no_subtitles,
        no_danmaku: args.sidecars.no_danmaku,
        subtitle_ai: args.sidecars.subtitle_ai,
        video_quality: args.video_quality,
        audio_quality: args.audio_quality,
        audio_language: args.audio_language.as_deref(),
    })?;
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
    let path_templates = path_templates_from_cli(args.templates);
    Ok(DownloadOptions::new(args.output_dir)
        .with_retry_policy(RetryPolicy::new(
            args.retry_attempts,
            Duration::from_millis(args.retry_backoff_ms),
        ))
        .with_stream_selection(
            StreamSelection::new(args.video_quality, args.audio_quality)
                .with_audio_language(args.audio_language.unwrap_or_default()),
        )
        .with_path_templates(path_templates)
        .with_download_idle_timeout(download_idle_timeout)
        .with_resume(!args.execution.no_resume)
        .with_download_mode(mode)
        .with_cover(!args.sidecars.no_cover)
        .with_subtitles(!args.sidecars.no_subtitles)
        .with_subtitle_ai_policy(args.sidecars.subtitle_ai.into())
        .with_danmaku(!args.sidecars.no_danmaku)
        .with_danmaku_formats(args.danmaku_formats.into_iter().map(Into::into))
        .with_media_hosts(media_hosts)
        .with_mux(mux))
}

fn danmaku_update_options_from_cli(
    args: &DanmakuUpdateCliArgs,
) -> anyhow::Result<DanmakuUpdateOptions> {
    ensure!(
        args.retry_attempts > 0,
        "--retry-attempts must be greater than 0"
    );
    let download_idle_timeout = if args.download_idle_timeout_seconds == 0 {
        None
    } else {
        Some(Duration::from_secs(args.download_idle_timeout_seconds))
    };
    Ok(DanmakuUpdateOptions::default()
        .with_retry_policy(RetryPolicy::new(
            args.retry_attempts,
            Duration::from_millis(args.retry_backoff_ms),
        ))
        .with_download_idle_timeout(download_idle_timeout)
        .with_danmaku_formats(args.danmaku_formats.iter().copied().map(Into::into)))
}

fn path_templates_from_cli(flags: DownloadTemplateCliFlags) -> DownloadPathTemplates {
    let mut templates = DownloadPathTemplates::new();
    if let Some(output_template) = flags.output {
        templates = templates.with_output_dir(output_template);
    }
    if let Some(entry_template) = flags.entry {
        templates = templates.with_entry_dir(entry_template);
    }
    if let Some(mux_template) = flags.mux {
        templates = templates.with_mux_file_stem(mux_template);
    }
    templates
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
    ensure!(
        !starts_with_url_scheme(host),
        "--upos-host expects only a host or host:port, not a URL"
    );
    ensure!(
        !host.contains('@'),
        "--upos-host must not include username or password data"
    );
    let parse_input = format!("https://{host}");
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

#[derive(Debug, Args)]
struct AccessKeyLoginArgs {
    #[arg(long, help = "Emit newline-delimited JSON ticket and saved events")]
    json: bool,
    #[arg(
        long,
        default_value = DEFAULT_ACCESS_KEY_AUTH_BASE,
        value_name = "URL",
        help = "BiliPlus/BALH-compatible authorization base URL"
    )]
    auth_base: String,
    #[arg(
        long,
        default_value = DEFAULT_ACCESS_KEY_CALLBACK_ORIGIN,
        value_name = "ORIGIN",
        help = "Callback origin passed to the authorization page"
    )]
    callback_origin: String,
    #[arg(
        long,
        value_name = "ORIGIN",
        help = "Validate a browser postMessage sender origin before parsing BALH data"
    )]
    message_origin: Option<String>,
    #[arg(
        long,
        conflicts_with = "file",
        help = "Read pasted BALH message or callback URL/query from piped or redirected stdin"
    )]
    stdin: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read pasted BALH message or callback URL/query from a file"
    )]
    file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct AccessKeyRenewalArgs {
    #[arg(
        long,
        help = "Emit newline-delimited JSON decision, ticket, and saved events"
    )]
    json: bool,
    #[arg(
        long,
        help = "Force reauthorization even when lifecycle metadata is fresh"
    )]
    force: bool,
    #[arg(long, default_value_t = 7 * 24 * 60 * 60)]
    stale_after_seconds: u64,
    #[arg(long, default_value_t = 24 * 60 * 60)]
    expiring_within_seconds: u64,
    #[arg(
        long,
        default_value = DEFAULT_ACCESS_KEY_AUTH_BASE,
        value_name = "URL",
        help = "BiliPlus/BALH-compatible authorization base URL"
    )]
    auth_base: String,
    #[arg(
        long,
        default_value = DEFAULT_ACCESS_KEY_CALLBACK_ORIGIN,
        value_name = "ORIGIN",
        help = "Callback origin passed to the authorization page"
    )]
    callback_origin: String,
    #[arg(
        long,
        value_name = "ORIGIN",
        help = "Validate a browser postMessage sender origin before parsing BALH data"
    )]
    message_origin: Option<String>,
    #[arg(
        long,
        conflicts_with = "file",
        help = "Read pasted BALH message or callback URL/query from piped or redirected stdin"
    )]
    stdin: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read pasted BALH message or callback URL/query from a file"
    )]
    file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(error) = run().await {
        if anyhow_error_is_cancelled(&error) {
            eprintln!("Error: {error:#}");
            std::process::exit(130);
        }
        return Err(error);
    }
    Ok(())
}

async fn run() -> anyhow::Result<()> {
    let raw_args = std::env::args_os().collect::<Vec<_>>();
    let cli = Cli::parse_from(raw_args.clone());
    ensure!(
        cli.request_timeout_seconds > 0,
        "--request-timeout-seconds must be greater than 0"
    );
    let endpoints = endpoints_from_cli(&cli);
    let restricted_area = restricted_area_from_cli_with_args(&cli, raw_args)?;
    let playurl_mode = cli.playurl_mode.into();
    let request_timeout = Duration::from_secs(cli.request_timeout_seconds);
    let client_runtime =
        ClientRuntimeConfig::new(endpoints, restricted_area, playurl_mode, request_timeout);
    let credential_preflight = CredentialPreflightRuntimeConfig::new(
        cli.credential_preflight.into(),
        cli.credential_stale_after_seconds,
        cli.credential_expiring_within_seconds,
    );
    let credential_runtime = CredentialRuntime::new(
        CredentialStore::new(credential_path(cli.credential_file)?),
        credential_profile_selection(cli.credential_profile)?,
    );
    match cli.command {
        Command::Info { url, select, json } => {
            handle_info(&credential_runtime, &client_runtime, url, select, json).await?;
        }
        Command::Plan { url, select, json } => {
            handle_plan(
                &credential_runtime,
                &client_runtime,
                &credential_preflight,
                url,
                select,
                json,
            )
            .await?;
        }
        Command::Playback { url, select, json } => {
            handle_playback(
                &credential_runtime,
                &client_runtime,
                &credential_preflight,
                url,
                select,
                json,
            )
            .await?;
        }
        Command::Download(args) => {
            handle_download_cli(
                &credential_runtime,
                &client_runtime,
                &credential_preflight,
                *args,
            )
            .await?;
        }
        Command::Danmaku { command } => {
            handle_danmaku(command, &credential_runtime, &client_runtime).await?;
        }
        Command::Auth { command } => {
            handle_auth(command, &credential_runtime, &client_runtime).await?;
        }
    }
    Ok(())
}

fn anyhow_error_is_cancelled(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<bbdown_core::Error>()
            .is_some_and(bbdown_core::Error::is_cancelled)
    })
}

async fn handle_download_cli(
    credentials: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    credential_preflight: &CredentialPreflightRuntimeConfig,
    args: DownloadCliArgs,
) -> anyhow::Result<()> {
    ensure!(
        args.archive_file.is_some() || args.on_duplicate.is_none(),
        "--on-duplicate requires --archive-file"
    );
    let options = download_options_from_cli(DownloadOptionCliArgs {
        output_dir: args.output_dir,
        retry_attempts: args.retry_attempts,
        retry_backoff_ms: args.retry_backoff_ms,
        download_idle_timeout_seconds: args.download_idle_timeout_seconds,
        only: args.only,
        execution: DownloadExecutionCliFlags {
            no_resume: args.no_resume,
            no_mux: args.no_mux,
        },
        sidecars: DownloadSidecarCliFlags {
            no_cover: args.no_cover,
            no_subtitles: args.no_subtitles,
            no_danmaku: args.no_danmaku,
            subtitle_ai: args.subtitle_ai,
        },
        media_hosts: DownloadMediaHostCliFlags {
            upos_host: args.upos_host,
            force_replace_host: args.force_replace_host,
            allow_pcdn: args.allow_pcdn,
        },
        danmaku_formats: args.danmaku_formats,
        video_quality: args.video_quality,
        audio_quality: args.audio_quality,
        audio_language: args.audio_language,
        templates: DownloadTemplateCliFlags {
            output: args.output_template,
            entry: args.entry_template,
            mux: args.mux_template,
        },
        ffmpeg: args.ffmpeg,
    })?;
    let command_args = DownloadCommandArgs {
        url: args.url,
        select: args.select,
        json: args.json,
        progress_json: args.progress_json,
        options,
        archive_file: args.archive_file,
        on_duplicate: args.on_duplicate.map(Into::into),
    };
    handle_download(
        credentials,
        client_runtime,
        credential_preflight,
        command_args,
    )
    .await
}

struct DownloadCommandArgs {
    url: String,
    select: Option<Selection>,
    json: bool,
    progress_json: bool,
    options: DownloadOptions,
    archive_file: Option<PathBuf>,
    on_duplicate: Option<DuplicateDecision>,
}

#[derive(Clone, Copy, Debug)]
struct CliProgressReporter {
    json: bool,
}

impl DownloadProgressSink for CliProgressReporter {
    fn on_download_progress(&self, event: &DownloadProgressEvent) {
        if !self.json {
            return;
        }
        if let Ok(line) = serde_json::to_string(event) {
            eprintln!("{line}");
        }
    }
}

#[derive(Debug)]
struct DeferredPlanCompletedProgress {
    inner: CliProgressReporter,
    plan_completed: Mutex<Option<DownloadProgressEvent>>,
}

impl DeferredPlanCompletedProgress {
    fn new(inner: CliProgressReporter) -> Self {
        Self {
            inner,
            plan_completed: Mutex::new(None),
        }
    }

    fn flush_plan_completed(&self) {
        let event = match self.plan_completed.lock() {
            Ok(mut event) => event.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(event) = event {
            self.inner.on_download_progress(&event);
        }
    }
}

impl DownloadProgressSink for DeferredPlanCompletedProgress {
    fn on_download_progress(&self, event: &DownloadProgressEvent) {
        if matches!(event, DownloadProgressEvent::PlanCompleted { .. }) {
            match self.plan_completed.lock() {
                Ok(mut stored) => *stored = Some(event.clone()),
                Err(poisoned) => *poisoned.into_inner() = Some(event.clone()),
            }
            return;
        }
        self.inner.on_download_progress(event);
    }
}

struct DownloadCancellationGuard {
    handle: JoinHandle<()>,
}

impl Drop for DownloadCancellationGuard {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn install_download_cancellation_handler(
    cancellation: DownloadCancellationToken,
    duplicate_prompt_active: Arc<AtomicBool>,
) -> DownloadCancellationGuard {
    let handle = tokio::spawn(async move {
        let mut graceful_cancel_requested = false;
        while tokio::signal::ctrl_c().await.is_ok() {
            match download_ctrl_c_action(&duplicate_prompt_active, graceful_cancel_requested) {
                DownloadCtrlCAction::GracefulCancel => {
                    cancellation.cancel_with_reason("download cancelled by Ctrl-C");
                    graceful_cancel_requested = true;
                }
                DownloadCtrlCAction::ForceExit => std::process::exit(130),
            }
        }
    });
    DownloadCancellationGuard { handle }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownloadCtrlCAction {
    GracefulCancel,
    ForceExit,
}

fn download_ctrl_c_action(
    duplicate_prompt_active: &AtomicBool,
    graceful_cancel_requested: bool,
) -> DownloadCtrlCAction {
    if graceful_cancel_requested || duplicate_prompt_active.load(Ordering::SeqCst) {
        DownloadCtrlCAction::ForceExit
    } else {
        DownloadCtrlCAction::GracefulCancel
    }
}

struct DuplicatePromptActiveGuard<'a> {
    active: &'a AtomicBool,
}

impl<'a> DuplicatePromptActiveGuard<'a> {
    fn new(active: &'a AtomicBool) -> Self {
        active.store(true, Ordering::SeqCst);
        Self { active }
    }
}

impl Drop for DuplicatePromptActiveGuard<'_> {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

fn emit_cli_plan_failed(
    progress: CliProgressReporter,
    title: &str,
    output_dir: &Path,
    completed_entries: usize,
    error: String,
) {
    progress.on_download_progress(&DownloadProgressEvent::PlanFailed {
        title: title.to_owned(),
        output_dir: output_dir.to_path_buf(),
        completed_entries,
        error,
    });
}

fn emit_cli_plan_cancelled(
    progress: CliProgressReporter,
    title: &str,
    output_dir: &Path,
    completed_entries: usize,
    error: String,
) {
    progress.on_download_progress(&DownloadProgressEvent::PlanCancelled {
        title: title.to_owned(),
        output_dir: output_dir.to_path_buf(),
        completed_entries,
        error,
    });
}

#[derive(Clone, Debug)]
struct ClientRuntimeConfig {
    endpoints: EndpointConfig,
    restricted_area: RestrictedAreaConfig,
    playurl_mode: PlayurlMode,
    request_timeout: Duration,
}

impl ClientRuntimeConfig {
    fn new(
        endpoints: EndpointConfig,
        restricted_area: RestrictedAreaConfig,
        playurl_mode: PlayurlMode,
        request_timeout: Duration,
    ) -> Self {
        Self {
            endpoints,
            restricted_area,
            playurl_mode,
            request_timeout,
        }
    }

    fn client_config(&self, credentials: Credentials) -> ClientConfig {
        self.client_config_with_access_key_provider(credentials, None)
    }

    fn client_config_with_access_key_provider(
        &self,
        credentials: Credentials,
        access_key_provider: Option<AccessKeyProvider>,
    ) -> ClientConfig {
        ClientConfig::new(self.endpoints.clone(), credentials)
            .with_access_key_provider(access_key_provider)
            .with_restricted_area(self.restricted_area.clone())
            .with_playurl_mode(self.playurl_mode)
            .with_user_agent("bbdown-rs/0.1")
            .with_request_timeout(self.request_timeout)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CredentialPreflightRuntimeConfig {
    mode: CredentialPreflightMode,
    stale_after_seconds: u64,
    expiring_within_seconds: u64,
}

impl CredentialPreflightRuntimeConfig {
    fn new(
        mode: CredentialPreflightMode,
        stale_after_seconds: u64,
        expiring_within_seconds: u64,
    ) -> Self {
        Self {
            mode,
            stale_after_seconds,
            expiring_within_seconds,
        }
    }

    fn policy(&self) -> CredentialLifecyclePolicy {
        lifecycle_policy_from_seconds(
            self.stale_after_seconds,
            self.expiring_within_seconds,
            current_unix_millis(),
        )
    }
}

#[derive(Clone, Debug)]
struct CredentialRuntime {
    store: CredentialStore,
    selection: CredentialProfileSelection,
}

impl CredentialRuntime {
    fn new(store: CredentialStore, selection: CredentialProfileSelection) -> Self {
        Self { store, selection }
    }

    fn load(&self) -> anyhow::Result<Credentials> {
        self.store
            .load_selected_profile(&self.selection)
            .context("failed to load credentials")
    }

    fn save(&self, credentials: &Credentials) -> anyhow::Result<()> {
        self.store
            .save_selected_profile(&self.selection, credentials)
            .context("failed to save credentials")
    }

    fn logout(&self) -> anyhow::Result<()> {
        match self.selection.profile_name() {
            Some(profile) => {
                self.store
                    .remove_profile(profile)
                    .context("failed to clear credential profile")?;
            }
            None => {
                self.store.clear().context("failed to clear credentials")?;
            }
        }
        Ok(())
    }

    fn selected_profile_name(&self, profiles: &CredentialProfiles) -> String {
        self.selection
            .profile_name()
            .map_or_else(|| profiles.default_profile.clone(), str::to_owned)
    }

    fn selected_access_key_provider(&self) -> anyhow::Result<Option<AccessKeyProvider>> {
        let profiles = self
            .store
            .load_profiles()
            .context("failed to load credential profiles")?;
        let selected_profile = self.selected_profile_name(&profiles);
        let metadata = profiles
            .profile_metadata(&selected_profile)
            .context("failed to load selected credential profile metadata")?;
        Ok(metadata
            .credential(CredentialKind::AccessKey)
            .and_then(|metadata| metadata.access_key_provider))
    }
}

fn credential_profile_selection(
    profile: Option<String>,
) -> anyhow::Result<CredentialProfileSelection> {
    match profile {
        Some(profile) => CredentialProfileSelection::named(profile)
            .map_err(anyhow::Error::from)
            .context("invalid credential profile"),
        None => Ok(CredentialProfileSelection::default_profile()),
    }
}

async fn handle_info(
    credentials: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    url: String,
    select: Option<Selection>,
    json: bool,
) -> anyhow::Result<()> {
    let client = BiliClient::new(client_runtime.client_config(credentials.load()?));
    let resolved = client.resolve_input(&url, select).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resolved)?);
    } else {
        print_human_summary(&resolved);
    }
    Ok(())
}

async fn prepare_credentials_for_media_request(
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    credential_preflight: &CredentialPreflightRuntimeConfig,
    request: MediaCredentialPreflightRequest<'_>,
) -> anyhow::Result<PreparedMediaRequest> {
    if credential_preflight.mode == CredentialPreflightMode::Off {
        return Ok(PreparedMediaRequest {
            credentials: credential_runtime.load()?,
            access_key_provider: credential_runtime.selected_access_key_provider()?,
            parsed_input: None,
            media_preflight_context: None,
            deferred_preflight: None,
        });
    }

    let media_preflight_context = media_credential_preflight_context_for_input(
        client_runtime,
        request.raw_input,
        request.selection,
        request.requires_media_streams,
        request.intl_access_key_may_run,
    )
    .await?;
    let mut report = credential_preflight_report(
        credential_runtime,
        client_runtime,
        credential_preflight,
        &media_preflight_context,
    )?;
    let defer_renewal = request.renewal_timing == CredentialPreflightRenewalTiming::Deferred
        && report.should_attempt_access_key_renewal();
    if report.should_attempt_access_key_renewal() && !defer_renewal {
        let profiles = credential_runtime
            .store
            .load_profiles()
            .context("failed to load credential profiles")?;
        if try_access_key_auto_refresh_for_preflight(
            credential_runtime,
            client_runtime,
            &profiles,
            &report.access_key_renewal,
            request.emit_diagnostics,
        )
        .await?
        {
            report = credential_preflight_report(
                credential_runtime,
                client_runtime,
                credential_preflight,
                &media_preflight_context,
            )?;
        }
    }

    if !defer_renewal && request.emit_diagnostics {
        emit_credential_preflight_warnings(&report);
    }
    if !defer_renewal && report.has_blocking_issues() {
        let messages = report
            .issues
            .iter()
            .filter(|issue| issue.blocking)
            .map(|issue| issue.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        bail!("credential preflight failed: {messages}");
    }
    let deferred_preflight = defer_renewal.then(|| DeferredMediaCredentialPreflight {
        context: media_preflight_context.clone(),
        report,
    });
    Ok(PreparedMediaRequest {
        credentials: credential_runtime.load()?,
        access_key_provider: credential_runtime.selected_access_key_provider()?,
        parsed_input: Some(media_preflight_context.input.clone()),
        media_preflight_context: Some(media_preflight_context),
        deferred_preflight,
    })
}

#[derive(Clone, Copy, Debug)]
struct MediaCredentialPreflightRequest<'a> {
    raw_input: &'a str,
    selection: Option<&'a Selection>,
    requires_media_streams: bool,
    intl_access_key_may_run: bool,
    renewal_timing: CredentialPreflightRenewalTiming,
    emit_diagnostics: bool,
}

struct PreparedMediaRequest {
    credentials: Credentials,
    access_key_provider: Option<AccessKeyProvider>,
    parsed_input: Option<Input>,
    media_preflight_context: Option<MediaCredentialPreflightContext>,
    deferred_preflight: Option<DeferredMediaCredentialPreflight>,
}

impl PreparedMediaRequest {
    fn client_config(&self, client_runtime: &ClientRuntimeConfig) -> ClientConfig {
        client_runtime.client_config_with_access_key_provider(
            self.credentials.clone(),
            self.access_key_provider,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CredentialPreflightRenewalTiming {
    Immediate,
    Deferred,
}

struct DeferredMediaCredentialPreflight {
    context: MediaCredentialPreflightContext,
    report: CredentialPreflightReport,
}

async fn complete_deferred_media_preflight_renewal(
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    credential_preflight: &CredentialPreflightRuntimeConfig,
    prepared: &mut PreparedMediaRequest,
    failure: Option<&bbdown_core::Error>,
    emit_diagnostics: bool,
) -> anyhow::Result<bool> {
    let Some(deferred) = prepared.deferred_preflight.take() else {
        return Ok(false);
    };
    let mut refreshed = false;
    let mut report = deferred.report;
    if report.should_attempt_access_key_renewal()
        && failure.is_none_or(|failure| {
            media_preflight_report_can_refresh_generic_access_key_for_failure(
                &deferred.context,
                &report,
                failure,
            )
        })
    {
        let profiles = credential_runtime
            .store
            .load_profiles()
            .context("failed to load credential profiles")?;
        refreshed = try_access_key_auto_refresh_for_preflight(
            credential_runtime,
            client_runtime,
            &profiles,
            &report.access_key_renewal,
            emit_diagnostics,
        )
        .await?;
        if refreshed {
            report = credential_preflight_report(
                credential_runtime,
                client_runtime,
                credential_preflight,
                &deferred.context,
            )?;
            prepared.credentials = credential_runtime.load()?;
            prepared.access_key_provider = credential_runtime.selected_access_key_provider()?;
        }
    }

    if emit_diagnostics {
        emit_credential_preflight_warnings(&report);
    }
    if report.has_blocking_issues() {
        let messages = report
            .issues
            .iter()
            .filter(|issue| issue.blocking)
            .map(|issue| issue.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        bail!("credential preflight failed: {messages}");
    }
    Ok(refreshed)
}

async fn try_forced_access_key_refresh_for_archive_retry(
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    credential_preflight: &CredentialPreflightRuntimeConfig,
    prepared: &mut PreparedMediaRequest,
    failure: &bbdown_core::Error,
    emit_diagnostics: bool,
) -> anyhow::Result<bool> {
    if credential_preflight.mode != CredentialPreflightMode::Renew {
        return Ok(false);
    }
    let Some(context) = prepared.media_preflight_context.as_ref() else {
        return Ok(false);
    };
    let policy = credential_preflight.policy();
    let (profiles, _selected_profile, mut statuses) =
        lifecycle_statuses_for_selection(credential_runtime, false, &policy)?;
    let status = statuses
        .pop()
        .context("failed to evaluate selected credential profile")?;
    if !media_preflight_context_can_refresh_generic_access_key(
        context,
        client_runtime,
        &status,
        failure,
    ) {
        return Ok(false);
    }
    let decision = AccessKeyRenewalDecision::from_profile_status(&status, true);
    if decision.automatic_refresh_readiness != AccessKeyAutomaticRefreshReadiness::Ready {
        return Ok(false);
    }
    let refreshed = try_access_key_auto_refresh_for_preflight(
        credential_runtime,
        client_runtime,
        &profiles,
        &decision,
        emit_diagnostics,
    )
    .await?;
    if refreshed {
        prepared.credentials = credential_runtime.load()?;
        prepared.access_key_provider = credential_runtime.selected_access_key_provider()?;
    }
    Ok(refreshed)
}

fn media_preflight_context_can_refresh_generic_access_key(
    context: &MediaCredentialPreflightContext,
    client_runtime: &ClientRuntimeConfig,
    status: &CredentialProfileLifecycleStatus,
    failure: &bbdown_core::Error,
) -> bool {
    let report = CredentialPreflightReport::evaluate(
        CredentialPreflightMode::Warn,
        status,
        credential_preflight_requirements_for_context(context, client_runtime),
    );
    media_preflight_report_can_refresh_generic_access_key_for_failure(context, &report, failure)
}

fn media_preflight_report_can_refresh_generic_access_key_for_failure(
    context: &MediaCredentialPreflightContext,
    report: &CredentialPreflightReport,
    failure: &bbdown_core::Error,
) -> bool {
    if authenticated_web_api_failure_may_have_used_cookie(context, failure) {
        return false;
    }
    if authenticated_web_api_cookie_missing(report) {
        return false;
    }
    if app_playurl_selected_tv_access_key(report)
        && app_playurl_auth_failure_may_have_used_selected_tv_access_key(failure)
    {
        return false;
    }
    report.requirements.iter().any(|requirement| {
        requirement.selected_kind == Some(CredentialKind::AccessKey)
            && requirement.selected_status != CredentialLifecycleStatus::Missing
    })
}

fn authenticated_web_api_failure_may_have_used_cookie(
    context: &MediaCredentialPreflightContext,
    failure: &bbdown_core::Error,
) -> bool {
    context.web_cookie_required
        && matches!(
            failure,
            bbdown_core::Error::Api { code, .. } if *code == -101
        )
}

fn authenticated_web_api_cookie_missing(report: &CredentialPreflightReport) -> bool {
    report.requirements.iter().any(|requirement| {
        requirement.request_path == CredentialPreflightRequestPath::AuthenticatedWebApi
            && requirement.selected_status == CredentialLifecycleStatus::Missing
    })
}

fn app_playurl_selected_tv_access_key(report: &CredentialPreflightReport) -> bool {
    report.requirements.iter().any(|requirement| {
        requirement.request_path == CredentialPreflightRequestPath::AppPlayurl
            && requirement.selected_kind == Some(CredentialKind::TvAccessKey)
    })
}

fn app_playurl_auth_failure_may_have_used_selected_tv_access_key(
    failure: &bbdown_core::Error,
) -> bool {
    matches!(
        failure,
        bbdown_core::Error::Api { code, message }
            if matches!(*code, 7 | 16) && auth_like_failure_message(message)
    )
}

fn credential_preflight_report(
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    credential_preflight: &CredentialPreflightRuntimeConfig,
    media_preflight_context: &MediaCredentialPreflightContext,
) -> anyhow::Result<CredentialPreflightReport> {
    let policy = credential_preflight.policy();
    let (_profiles, _selected_profile, mut statuses) =
        lifecycle_statuses_for_selection(credential_runtime, false, &policy)?;
    let status = statuses
        .pop()
        .context("failed to evaluate selected credential profile")?;
    Ok(CredentialPreflightReport::evaluate(
        credential_preflight.mode,
        &status,
        credential_preflight_requirements_for_context(media_preflight_context, client_runtime),
    ))
}

fn credential_preflight_requirements_for_context(
    context: &MediaCredentialPreflightContext,
    client_runtime: &ClientRuntimeConfig,
) -> Vec<CredentialPreflightRequirement> {
    let mut requirements = credential_preflight_requirements_for_media_paths(
        context.playurl_mode,
        &client_runtime.restricted_area,
        context.restricted_area_proxy_may_run,
        context.intl_access_key_may_run,
    );
    if context.web_cookie_required {
        requirements.push(CredentialPreflightRequirement::authenticated_web_api_cookie());
    }
    requirements
}

#[derive(Clone, Debug)]
struct MediaCredentialPreflightContext {
    input: Input,
    playurl_mode: Option<PlayurlMode>,
    restricted_area_proxy_may_run: bool,
    intl_access_key_may_run: bool,
    web_cookie_required: bool,
}

async fn media_credential_preflight_context_for_input(
    client_runtime: &ClientRuntimeConfig,
    raw_input: &str,
    selection: Option<&Selection>,
    requires_media_streams: bool,
    intl_access_key_may_run: bool,
) -> anyhow::Result<MediaCredentialPreflightContext> {
    let client = BiliClient::new(client_runtime.client_config(Credentials::default()));
    let input = client.parse_input(raw_input).await?;
    ensure_input_selection_for_media_preflight(&input, selection)?;
    let playurl_mode = if requires_media_streams {
        input_media_preflight_playurl_mode(&input, client_runtime.playurl_mode)
    } else {
        None
    };
    Ok(MediaCredentialPreflightContext {
        input: input.clone(),
        playurl_mode,
        restricted_area_proxy_may_run: requires_media_streams
            && !client_runtime.restricted_area.proxies.is_empty()
            && input_may_use_restricted_area_proxy(&input),
        intl_access_key_may_run: intl_access_key_may_run && input_may_use_intl_access_key(&input),
        web_cookie_required: input_requires_web_cookie(&input),
    })
}

fn ensure_input_selection_for_media_preflight(
    input: &Input,
    selection: Option<&Selection>,
) -> anyhow::Result<()> {
    if selection.is_none()
        && let Some(input_kind) = selection_required_input_kind(input)
    {
        return Err(bbdown_core::Error::SelectionRequired { input_kind }.into());
    }
    Ok(())
}

fn selection_required_input_kind(input: &Input) -> Option<&'static str> {
    match input {
        Input::Season(_) => Some("season"),
        Input::Media(_) => Some("media"),
        Input::CheeseSeason(_) => Some("cheese season"),
        _ => None,
    }
}

fn input_media_preflight_playurl_mode(
    input: &Input,
    configured: PlayurlMode,
) -> Option<PlayurlMode> {
    match input {
        Input::IntlEpisode(_) => None,
        Input::CheeseEpisode(_) | Input::CheeseSeason(_) => Some(PlayurlMode::Web),
        _ => Some(configured),
    }
}

fn input_may_use_restricted_area_proxy(input: &Input) -> bool {
    matches!(
        input,
        Input::Episode(_) | Input::Season(_) | Input::Media(_) | Input::ShortLink(_)
    )
}

fn input_may_use_intl_access_key(input: &Input) -> bool {
    matches!(input, Input::IntlEpisode(_))
}

fn input_requires_web_cookie(input: &Input) -> bool {
    matches!(
        input,
        Input::FollowingFeed | Input::History | Input::WatchLater
    )
}

fn download_mode_may_use_intl_access_key(mode: DownloadMode) -> bool {
    matches!(
        mode,
        DownloadMode::All
            | DownloadMode::VideoOnly
            | DownloadMode::AudioOnly
            | DownloadMode::SubtitleOnly
    )
}

async fn try_access_key_auto_refresh_for_preflight(
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    profiles: &CredentialProfiles,
    decision: &AccessKeyRenewalDecision,
    emit_diagnostics: bool,
) -> anyhow::Result<bool> {
    let refresh = match access_key_refresh_request_from_profiles(profiles, &decision.profile) {
        Ok(refresh) => refresh,
        Err(error) => {
            if emit_diagnostics {
                eprintln!(
                    "credential preflight warning: automatic access_key refresh setup failed: {}",
                    display_human_text(&error.to_string())
                );
            }
            return Ok(false);
        }
    };
    let client = BiliClient::new(client_runtime.client_config(Credentials::default()));
    match client.refresh_access_key(&refresh.request).await {
        Ok(refreshed) => {
            let _summary =
                save_refreshed_access_key_silent(credential_runtime, &refresh, &refreshed)?;
            if emit_diagnostics {
                eprintln!("credential preflight: access_key refreshed");
            }
            Ok(true)
        }
        Err(error) => {
            let message = redact_access_key_refresh_error(&error, &refresh.request);
            if emit_diagnostics {
                eprintln!(
                    "credential preflight warning: automatic access_key refresh failed: {}",
                    display_human_text(&message)
                );
            }
            Ok(false)
        }
    }
}

fn emit_credential_preflight_warnings(report: &CredentialPreflightReport) {
    for issue in report.issues.iter().filter(|issue| !issue.blocking) {
        eprintln!("credential preflight warning: {}", issue.message);
    }
}

async fn handle_plan(
    credentials: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    credential_preflight: &CredentialPreflightRuntimeConfig,
    url: String,
    select: Option<Selection>,
    json: bool,
) -> anyhow::Result<()> {
    let prepared = prepare_credentials_for_media_request(
        credentials,
        client_runtime,
        credential_preflight,
        MediaCredentialPreflightRequest {
            raw_input: &url,
            selection: select.as_ref(),
            requires_media_streams: true,
            intl_access_key_may_run: true,
            renewal_timing: CredentialPreflightRenewalTiming::Immediate,
            emit_diagnostics: true,
        },
    )
    .await?;
    let client = BiliClient::new(prepared.client_config(client_runtime));
    let plan = match prepared.parsed_input {
        Some(input) => client.plan(input, select).await?,
        None => client.plan_download(&url, select).await?,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print_plan_summary(&plan);
    }
    Ok(())
}

async fn handle_playback(
    credentials: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    credential_preflight: &CredentialPreflightRuntimeConfig,
    url: String,
    select: Option<Selection>,
    json: bool,
) -> anyhow::Result<()> {
    let prepared = prepare_credentials_for_media_request(
        credentials,
        client_runtime,
        credential_preflight,
        MediaCredentialPreflightRequest {
            raw_input: &url,
            selection: select.as_ref(),
            requires_media_streams: true,
            intl_access_key_may_run: true,
            renewal_timing: CredentialPreflightRenewalTiming::Immediate,
            emit_diagnostics: true,
        },
    )
    .await?;
    let client = BiliClient::new(prepared.client_config(client_runtime));
    let plan = match prepared.parsed_input {
        Some(input) => client.plan_playback_input(input, select).await?,
        None => client.plan_playback(&url, select).await?,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print_playback_summary(&plan);
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
        for subtitle in &entry.subtitles {
            println!("    - {}", subtitle_summary(subtitle));
        }
        println!("  danmaku: {}", entry.danmaku.xml_url);
    }
}

fn print_playback_summary(plan: &PlaybackPlan) {
    println!("title: {}", plan.title);
    println!("entries: {}", plan.entries.len());
    for entry in &plan.entries {
        println!(
            "- P{} aid={} cid={} title={}",
            entry.index, entry.aid, entry.cid, entry.title
        );
        println!("  qualities: {}", quality_list(&entry.qualities));
        println!("  variants: {}", entry.variants.len());
        for variant in &entry.variants {
            let mut parts = vec![format!("id={}", variant.id)];
            parts.push(format!("kind={:?}", variant.kind).to_ascii_lowercase());
            if let (Some(width), Some(height)) = (variant.width, variant.height) {
                parts.push(format!("{width}x{height}"));
            }
            if let Some(frame_rate) = variant.frame_rate.as_deref() {
                parts.push(format!("{frame_rate}fps"));
            }
            if let Some(audio_language) = variant
                .audio
                .as_ref()
                .and_then(|audio| audio.language.as_deref())
            {
                parts.push(format!("audio_lang={audio_language}"));
            }
            if !variant.codecs.is_empty() {
                parts.push(variant.codecs.join("+"));
            }
            if let Some(bandwidth) = variant.bandwidth {
                parts.push(format!("{bandwidth}bps"));
            }
            if let Some(abr) = &variant.abr {
                parts.push(format!(
                    "abr={}/{} switchable={}",
                    abr.level_index.saturating_add(1),
                    abr.level_count,
                    abr.switchable
                ));
            }
            let avplayer_hint = &variant.selection_hints.avplayer;
            parts.push(format!("format={}", avplayer_hint.format_key));
            let avplayer_status = if avplayer_hint.preferred {
                "preferred"
            } else if avplayer_hint.playable {
                "playable"
            } else {
                "avoid"
            };
            parts.push(format!("avplayer={avplayer_status}"));
            println!("    - {}", parts.join(" "));
        }
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

fn subtitle_summary(subtitle: &bbdown_core::SubtitleTrack) -> String {
    let mut parts = vec![format!("lang={}", subtitle.language)];
    if let Some(language_doc) = subtitle.language_doc.as_deref() {
        parts.push(format!("lang_doc={language_doc}"));
    }
    parts.push(format!("{:?}", subtitle.format).to_ascii_lowercase());
    if subtitle.is_ai_generated {
        parts.push("ai=true".to_owned());
    }
    if let Some(ai_type) = subtitle.ai_type {
        parts.push(format!("ai_type={ai_type}"));
    }
    if let Some(ai_status) = subtitle.ai_status {
        parts.push(format!("ai_status={ai_status}"));
    }
    parts.join(" ")
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
    if let Some(language) = stream.language.as_deref() {
        parts.push(format!("lang={language}"));
    }
    if let Some(language_doc) = stream.language_doc.as_deref() {
        parts.push(format!("lang_doc={language_doc}"));
    }
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
    credentials: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    credential_preflight: &CredentialPreflightRuntimeConfig,
    args: DownloadCommandArgs,
) -> anyhow::Result<()> {
    let progress = CliProgressReporter {
        json: args.progress_json,
    };
    let renewal_timing = if args.archive_file.is_some() {
        CredentialPreflightRenewalTiming::Deferred
    } else {
        CredentialPreflightRenewalTiming::Immediate
    };
    let prepared = match prepare_credentials_for_media_request(
        credentials,
        client_runtime,
        credential_preflight,
        MediaCredentialPreflightRequest {
            raw_input: &args.url,
            selection: args.select.as_ref(),
            requires_media_streams: args.options.mode.requires_media_streams(),
            intl_access_key_may_run: download_mode_may_use_intl_access_key(args.options.mode),
            renewal_timing,
            emit_diagnostics: !args.progress_json,
        },
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(error) => {
            emit_cli_plan_failed(
                progress,
                &args.url,
                &args.options.output_dir,
                0,
                error.to_string(),
            );
            return Err(error);
        }
    };
    let cancellation = DownloadCancellationToken::new();
    let duplicate_prompt_active = Arc::new(AtomicBool::new(false));
    let _cancellation_guard = install_download_cancellation_handler(
        cancellation.clone(),
        Arc::clone(&duplicate_prompt_active),
    );
    let json = args.json;
    let report = if let Some(archive_file) = args.archive_file.clone() {
        let Some(report) = handle_archive_download(
            ArchiveDownloadRuntime {
                credential_runtime: credentials,
                client_runtime,
                credential_preflight,
                progress,
                cancellation: &cancellation,
                duplicate_prompt_active: &duplicate_prompt_active,
            },
            prepared,
            args,
            archive_file,
        )
        .await?
        else {
            return Ok(());
        };
        report
    } else {
        let client = BiliClient::new(prepared.client_config(client_runtime));
        let input_title = args.url.clone();
        let output_dir = args.options.output_dir.clone();
        let plan = plan_download_or_report(
            &client,
            DownloadPlanningRequest {
                raw: &args.url,
                parsed_input: prepared.parsed_input,
                selection: args.select,
                mode: args.options.mode,
                input_title: &input_title,
                output_dir: &output_dir,
                progress,
                cancellation: &cancellation,
            },
        )
        .await?;
        client
            .download_plan_with_progress_and_cancellation(
                &plan,
                args.options,
                &progress,
                &cancellation,
            )
            .await?
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_download_report(&report);
    }
    Ok(())
}

struct ArchiveDownloadRuntime<'a> {
    credential_runtime: &'a CredentialRuntime,
    client_runtime: &'a ClientRuntimeConfig,
    credential_preflight: &'a CredentialPreflightRuntimeConfig,
    progress: CliProgressReporter,
    cancellation: &'a DownloadCancellationToken,
    duplicate_prompt_active: &'a AtomicBool,
}

async fn handle_archive_download(
    runtime: ArchiveDownloadRuntime<'_>,
    mut prepared: PreparedMediaRequest,
    args: DownloadCommandArgs,
    archive_file: PathBuf,
) -> anyhow::Result<Option<DownloadReport>> {
    let progress = runtime.progress;
    let cancellation = runtime.cancellation;
    let duplicate_prompt_active = runtime.duplicate_prompt_active;
    let mut client = BiliClient::new(prepared.client_config(runtime.client_runtime));
    let input_title = args.url.clone();
    let output_dir = args.options.output_dir.clone();
    let mut plan = plan_archive_download_with_deferred_retry_or_report(
        &runtime,
        &mut client,
        &mut prepared,
        &args,
        &input_title,
        &output_dir,
    )
    .await?;
    let mut archive = load_archive_or_report(
        &archive_file,
        progress,
        &plan.title,
        &args.options.output_dir,
    )?;
    let mut preflight =
        inspect_download_preflight_or_report(&plan, &args.options, &archive, progress)?;
    let mut duplicate_decision = archive_duplicate_decision_or_report(
        &args,
        &plan,
        &preflight,
        progress,
        cancellation,
        duplicate_prompt_active,
    )?;
    if duplicate_decision == ArchiveDuplicateDecision::Cancelled {
        return Ok(None);
    }
    let refreshed = complete_deferred_archive_preflight_renewal_or_report(
        &runtime,
        &mut prepared,
        &args,
        &plan,
    )
    .await?;
    if refreshed {
        (client, plan) = replan_archive_download_after_refresh_or_report(
            &runtime,
            &prepared,
            &args,
            &input_title,
            &output_dir,
        )
        .await?;
        let previous_preflight = preflight;
        preflight = inspect_download_preflight_or_report(&plan, &args.options, &archive, progress)?;
        duplicate_decision = archive_duplicate_decision_after_refresh_or_report(
            &runtime,
            &args,
            &plan,
            &previous_preflight,
            &preflight,
            duplicate_decision,
        )?;
        if duplicate_decision == ArchiveDuplicateDecision::Cancelled {
            return Ok(None);
        }
    }
    let Some(execution_decision) =
        archive_execution_duplicate_decision(&args, &preflight, duplicate_decision)
    else {
        return Ok(None);
    };
    let decision_output_dir =
        decision_output_dir_or_report(&preflight, execution_decision, progress, &plan.title)?;
    ensure_archive_file_is_not_decision_output_root_or_report(
        &archive_file,
        &decision_output_dir,
        progress,
        &plan.title,
    )?;
    let archive_progress = DeferredPlanCompletedProgress::new(progress);
    let report = client
        .download_plan_with_archive_preflight_decision_with_progress_and_cancellation(
            &plan,
            args.options,
            &mut archive,
            &preflight,
            execution_decision,
            &archive_progress,
            cancellation,
        )
        .await?;
    save_archive_or_report(&archive, &archive_file, &report, progress, &plan.title)?;
    archive_progress.flush_plan_completed();
    Ok(Some(report))
}

async fn replan_archive_download_after_refresh_or_report(
    runtime: &ArchiveDownloadRuntime<'_>,
    prepared: &PreparedMediaRequest,
    args: &DownloadCommandArgs,
    input_title: &str,
    output_dir: &Path,
) -> anyhow::Result<(BiliClient, DownloadPlan)> {
    let client = BiliClient::new(prepared.client_config(runtime.client_runtime));
    let plan = plan_archive_download_or_report(
        &client,
        args,
        prepared.parsed_input.clone(),
        input_title,
        output_dir,
        runtime.progress,
        runtime.cancellation,
    )
    .await?;
    Ok((client, plan))
}

async fn complete_deferred_archive_preflight_renewal_or_report(
    runtime: &ArchiveDownloadRuntime<'_>,
    prepared: &mut PreparedMediaRequest,
    args: &DownloadCommandArgs,
    plan: &DownloadPlan,
) -> anyhow::Result<bool> {
    complete_deferred_archive_preflight_renewal_for_target(
        runtime,
        prepared,
        args,
        &plan.title,
        &args.options.output_dir,
        None,
    )
    .await
}

fn archive_duplicate_decision_after_refresh_or_report(
    runtime: &ArchiveDownloadRuntime<'_>,
    args: &DownloadCommandArgs,
    plan: &DownloadPlan,
    previous_preflight: &DownloadPreflight,
    preflight: &DownloadPreflight,
    current_decision: ArchiveDuplicateDecision,
) -> anyhow::Result<ArchiveDuplicateDecision> {
    if preflight.requires_decision() && preflight != previous_preflight {
        archive_duplicate_decision_or_report(
            args,
            plan,
            preflight,
            runtime.progress,
            runtime.cancellation,
            runtime.duplicate_prompt_active,
        )
    } else {
        Ok(current_decision)
    }
}

fn archive_execution_duplicate_decision(
    args: &DownloadCommandArgs,
    preflight: &DownloadPreflight,
    duplicate_decision: ArchiveDuplicateDecision,
) -> Option<DuplicateDecision> {
    match duplicate_decision {
        ArchiveDuplicateDecision::Decision(decision)
            if args.on_duplicate.is_some() || preflight.requires_decision() =>
        {
            Some(decision)
        }
        ArchiveDuplicateDecision::Decision(_) | ArchiveDuplicateDecision::NoDecisionRequired => {
            Some(DuplicateDecision::Cancel)
        }
        ArchiveDuplicateDecision::Cancelled => None,
    }
}

async fn complete_deferred_archive_preflight_renewal_for_target(
    runtime: &ArchiveDownloadRuntime<'_>,
    prepared: &mut PreparedMediaRequest,
    args: &DownloadCommandArgs,
    title: &str,
    output_dir: &Path,
    failure: Option<&bbdown_core::Error>,
) -> anyhow::Result<bool> {
    match complete_deferred_media_preflight_renewal(
        runtime.credential_runtime,
        runtime.client_runtime,
        runtime.credential_preflight,
        prepared,
        failure,
        !args.progress_json,
    )
    .await
    {
        Ok(refreshed) => Ok(refreshed),
        Err(error) => {
            emit_cli_plan_failed(runtime.progress, title, output_dir, 0, error.to_string());
            Err(error)
        }
    }
}

async fn plan_archive_download_with_deferred_retry_or_report(
    runtime: &ArchiveDownloadRuntime<'_>,
    client: &mut BiliClient,
    prepared: &mut PreparedMediaRequest,
    args: &DownloadCommandArgs,
    input_title: &str,
    output_dir: &Path,
) -> anyhow::Result<DownloadPlan> {
    let request = DownloadPlanningRequest {
        raw: &args.url,
        parsed_input: prepared.parsed_input.clone(),
        selection: args.select.clone(),
        mode: args.options.mode,
        input_title,
        output_dir,
        progress: runtime.progress,
        cancellation: runtime.cancellation,
    };
    match plan_download(client, &request).await {
        Ok(plan) => Ok(plan),
        Err(error) if plan_failure_may_be_credential_related(&error) => {
            let refreshed = if prepared.deferred_preflight.is_some() {
                complete_deferred_archive_preflight_renewal_for_target(
                    runtime,
                    prepared,
                    args,
                    input_title,
                    output_dir,
                    Some(&error),
                )
                .await?
            } else {
                match try_forced_access_key_refresh_for_archive_retry(
                    runtime.credential_runtime,
                    runtime.client_runtime,
                    runtime.credential_preflight,
                    prepared,
                    &error,
                    !args.progress_json,
                )
                .await
                {
                    Ok(refreshed) => refreshed,
                    Err(refresh_error) => {
                        emit_cli_plan_failed(
                            runtime.progress,
                            input_title,
                            output_dir,
                            0,
                            refresh_error.to_string(),
                        );
                        return Err(refresh_error);
                    }
                }
            };
            if refreshed {
                *client = BiliClient::new(prepared.client_config(runtime.client_runtime));
                match plan_download(client, &request).await {
                    Ok(plan) => Ok(plan),
                    Err(error) => {
                        emit_plan_error_for_request(&request, &error);
                        Err(error.into())
                    }
                }
            } else {
                emit_plan_error_for_request(&request, &error);
                Err(error.into())
            }
        }
        Err(error) => {
            emit_plan_error_for_request(&request, &error);
            Err(error.into())
        }
    }
}

fn plan_failure_may_be_credential_related(error: &bbdown_core::Error) -> bool {
    match error {
        bbdown_core::Error::Api { code, message } => {
            api_failure_may_be_credential_related(*code, message)
        }
        bbdown_core::Error::Http(error) => error.status().is_some_and(|status| {
            http_status_failure_may_be_credential_related(status.as_u16(), &error.to_string())
        }),
        bbdown_core::Error::AccessRestricted(message) => {
            restricted_area_resolver_failure_may_be_credential_related(message)
        }
        bbdown_core::Error::Cancelled { .. }
        | bbdown_core::Error::InvalidInput(_)
        | bbdown_core::Error::SelectionRequired { .. }
        | bbdown_core::Error::Unsupported(_)
        | bbdown_core::Error::MissingField(_)
        | bbdown_core::Error::Url(_)
        | bbdown_core::Error::Json(_)
        | bbdown_core::Error::Io(_)
        | bbdown_core::Error::MuxFailed { .. } => false,
    }
}

fn api_failure_may_be_credential_related(code: i64, message: &str) -> bool {
    match code {
        -101 => {
            access_key_specific_failure_message(message)
                || bili_account_not_logged_in_message(message)
        }
        -400 | -403 | 7 => access_key_specific_failure_message(message),
        16 => {
            access_key_specific_failure_message(message)
                || message == "APP playurl gRPC request failed"
        }
        _ => false,
    }
}

fn http_status_failure_may_be_credential_related(status: u16, message: &str) -> bool {
    matches!(status, 401 | 403) && access_key_refresh_failure_message(message)
}

fn access_key_refresh_failure_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    access_key_specific_failure_message(&lower)
        || lower.contains("not login")
        || lower.contains("no login")
}

fn access_key_specific_failure_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("access_key") || lower.contains("access key") || lower.contains("access token")
}

fn bili_account_not_logged_in_message(message: &str) -> bool {
    message.contains("账号未登录")
        || message.contains("账号未登陆")
        || message.contains("帳號未登錄")
        || message.contains("帳號未登入")
}

fn auth_like_failure_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("access_key")
        || lower.contains("access key")
        || lower.contains("credential")
        || lower.contains("token")
        || lower.contains("unauthorized")
        || lower.contains("not login")
        || lower.contains("no login")
        || contains_auth_word(&lower)
}

fn contains_auth_word(message: &str) -> bool {
    message
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| {
            matches!(
                word,
                "auth"
                    | "oauth"
                    | "authenticate"
                    | "authenticated"
                    | "authentication"
                    | "authorize"
                    | "authorized"
                    | "authorization"
            )
        })
}

fn restricted_area_resolver_failure_may_be_credential_related(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("restricted-area resolver failed")
        && ((lower.contains("api code -101")
            && (access_key_specific_failure_message(&lower) || lower.contains("pgcproxy")))
            || ((lower.contains("api code -403")
                || lower.contains("api code -400")
                || lower.contains("api code 7")
                || lower.contains("api code 16"))
                && access_key_specific_failure_message(&lower))
            || ((lower.contains("401") || lower.contains("403") || lower.contains("unauthorized"))
                && access_key_refresh_failure_message(&lower)))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArchiveDuplicateDecision {
    Decision(DuplicateDecision),
    NoDecisionRequired,
    Cancelled,
}

fn archive_duplicate_decision_or_report(
    args: &DownloadCommandArgs,
    plan: &DownloadPlan,
    preflight: &DownloadPreflight,
    progress: CliProgressReporter,
    cancellation: &DownloadCancellationToken,
    duplicate_prompt_active: &AtomicBool,
) -> anyhow::Result<ArchiveDuplicateDecision> {
    let stdin_is_terminal = io::stdin().is_terminal();
    let suppress_human_preflight = args.json || args.progress_json;
    let duplicate_prompt_printed_preflight = should_prompt_duplicate_decision(
        args.on_duplicate,
        suppress_human_preflight,
        preflight,
        stdin_is_terminal,
    );
    let decision = duplicate_decision_or_report(DuplicateDecisionRequest {
        on_duplicate: args.on_duplicate,
        suppress_human_preflight,
        stdin_is_terminal,
        preflight,
        progress,
        title: &plan.title,
        cancellation,
        duplicate_prompt_active,
    })?;
    let Some(decision) = decision else {
        return Ok(ArchiveDuplicateDecision::NoDecisionRequired);
    };
    if preflight.requires_decision() && decision == DuplicateDecision::Cancel {
        report_archive_duplicate_cancel(
            args.json,
            suppress_human_preflight,
            duplicate_prompt_printed_preflight,
            progress,
            &plan.title,
            preflight,
        )?;
        return Ok(ArchiveDuplicateDecision::Cancelled);
    }
    Ok(ArchiveDuplicateDecision::Decision(decision))
}

async fn plan_archive_download_or_report(
    client: &BiliClient,
    args: &DownloadCommandArgs,
    parsed_input: Option<Input>,
    input_title: &str,
    output_dir: &Path,
    progress: CliProgressReporter,
    cancellation: &DownloadCancellationToken,
) -> anyhow::Result<DownloadPlan> {
    plan_download_or_report(
        client,
        DownloadPlanningRequest {
            raw: &args.url,
            parsed_input,
            selection: args.select.clone(),
            mode: args.options.mode,
            input_title,
            output_dir,
            progress,
            cancellation,
        },
    )
    .await
}

fn inspect_download_preflight_or_report(
    plan: &DownloadPlan,
    options: &DownloadOptions,
    archive: &DownloadArchive,
    progress: CliProgressReporter,
) -> anyhow::Result<DownloadPreflight> {
    match DownloadPreflight::inspect(plan, options, Some(archive)) {
        Ok(preflight) => Ok(preflight),
        Err(error) => {
            emit_cli_plan_failed(
                progress,
                &plan.title,
                &options.output_dir,
                0,
                error.to_string(),
            );
            Err(error.into())
        }
    }
}

#[derive(Clone)]
struct DownloadPlanningRequest<'a> {
    raw: &'a str,
    parsed_input: Option<Input>,
    selection: Option<Selection>,
    mode: DownloadMode,
    input_title: &'a str,
    output_dir: &'a Path,
    progress: CliProgressReporter,
    cancellation: &'a DownloadCancellationToken,
}

async fn plan_download_or_report(
    client: &BiliClient,
    request: DownloadPlanningRequest<'_>,
) -> anyhow::Result<DownloadPlan> {
    match plan_download(client, &request).await {
        Ok(plan) => Ok(plan),
        Err(error) => {
            emit_plan_error_for_request(&request, &error);
            Err(error.into())
        }
    }
}

async fn plan_download(
    client: &BiliClient,
    request: &DownloadPlanningRequest<'_>,
) -> bbdown_core::Result<DownloadPlan> {
    let plan_result = tokio::select! {
        result = async {
            match request.parsed_input.clone() {
                Some(input) => client
                    .plan_with_download_mode(input, request.selection.clone(), request.mode)
                    .await,
                None => client
                    .plan_download_with_mode(request.raw, request.selection.clone(), request.mode)
                    .await,
            }
        } => result,
        () = request.cancellation.cancelled() => Err(request.cancellation.cancelled_error()),
    };
    plan_result
}

fn emit_plan_error_for_request(request: &DownloadPlanningRequest<'_>, error: &bbdown_core::Error) {
    if error.is_cancelled() {
        emit_cli_plan_cancelled(
            request.progress,
            request.input_title,
            request.output_dir,
            0,
            error.to_string(),
        );
    } else {
        emit_cli_plan_failed(
            request.progress,
            request.input_title,
            request.output_dir,
            0,
            error.to_string(),
        );
    }
}

fn load_archive_or_report(
    archive_file: &Path,
    progress: CliProgressReporter,
    title: &str,
    output_dir: &Path,
) -> anyhow::Result<DownloadArchive> {
    match DownloadArchive::load(archive_file)
        .with_context(|| format!("failed to load archive {}", archive_file.display()))
    {
        Ok(archive) => Ok(archive),
        Err(error) => {
            emit_cli_plan_failed(progress, title, output_dir, 0, error.to_string());
            Err(error)
        }
    }
}

#[derive(Clone, Copy)]
struct DuplicateDecisionRequest<'a> {
    on_duplicate: Option<DuplicateDecision>,
    suppress_human_preflight: bool,
    stdin_is_terminal: bool,
    preflight: &'a DownloadPreflight,
    progress: CliProgressReporter,
    title: &'a str,
    cancellation: &'a DownloadCancellationToken,
    duplicate_prompt_active: &'a AtomicBool,
}

fn duplicate_decision_or_report(
    request: DuplicateDecisionRequest<'_>,
) -> anyhow::Result<Option<DuplicateDecision>> {
    match duplicate_decision(
        request.on_duplicate,
        request.suppress_human_preflight,
        request.stdin_is_terminal,
        request.preflight,
        request.cancellation,
        request.duplicate_prompt_active,
    ) {
        Ok(decision) => Ok(decision),
        Err(error) => {
            if request.cancellation.is_cancelled() {
                let error = request.cancellation.cancelled_error();
                emit_cli_plan_cancelled(
                    request.progress,
                    request.title,
                    &request.preflight.planned_output_dir,
                    0,
                    error.to_string(),
                );
                return Err(error.into());
            }
            emit_cli_plan_failed(
                request.progress,
                request.title,
                &request.preflight.planned_output_dir,
                0,
                error.to_string(),
            );
            Err(error)
        }
    }
}

fn decision_output_dir_or_report(
    preflight: &DownloadPreflight,
    decision: DuplicateDecision,
    progress: CliProgressReporter,
    title: &str,
) -> anyhow::Result<PathBuf> {
    match preflight.output_dir_for_decision(decision) {
        Ok(output_dir) => Ok(output_dir),
        Err(error) => {
            emit_cli_plan_failed(
                progress,
                title,
                &preflight.planned_output_dir,
                0,
                error.to_string(),
            );
            Err(error.into())
        }
    }
}

fn ensure_archive_file_is_not_decision_output_root_or_report(
    archive_file: &Path,
    decision_output_dir: &Path,
    progress: CliProgressReporter,
    title: &str,
) -> anyhow::Result<()> {
    if let Err(error) = ensure_archive_file_is_not_output_root(archive_file, decision_output_dir) {
        emit_cli_plan_failed(progress, title, decision_output_dir, 0, error.to_string());
        return Err(error);
    }
    Ok(())
}

fn save_archive_or_report(
    archive: &DownloadArchive,
    archive_file: &Path,
    report: &DownloadReport,
    progress: CliProgressReporter,
    title: &str,
) -> anyhow::Result<()> {
    if let Err(error) = ensure_archive_file_is_not_output_root(archive_file, &report.output_dir) {
        emit_cli_plan_failed(
            progress,
            title,
            &report.output_dir,
            report.entries.len(),
            error.to_string(),
        );
        return Err(error);
    }
    if let Err(error) = archive
        .save(archive_file)
        .with_context(|| format!("failed to save archive {}", archive_file.display()))
    {
        emit_cli_plan_failed(
            progress,
            title,
            &report.output_dir,
            report.entries.len(),
            error.to_string(),
        );
        return Err(error);
    }
    Ok(())
}

fn report_archive_duplicate_cancel(
    json: bool,
    suppress_human_preflight: bool,
    duplicate_prompt_printed_preflight: bool,
    progress: CliProgressReporter,
    title: &str,
    preflight: &DownloadPreflight,
) -> anyhow::Result<()> {
    progress.on_download_progress(&DownloadProgressEvent::PlanCancelled {
        title: title.to_owned(),
        output_dir: preflight.planned_output_dir.clone(),
        completed_entries: 0,
        error: "archive or output conflict requires a decision".to_owned(),
    });
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "canceled",
                "preflight": preflight,
            }))?
        );
    } else {
        if !duplicate_prompt_printed_preflight && !suppress_human_preflight {
            print_duplicate_preflight(preflight);
        }
        println!("download canceled");
    }
    Ok(())
}

async fn handle_danmaku(
    command: DanmakuCommand,
    credentials: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
) -> anyhow::Result<()> {
    match command {
        DanmakuCommand::Update(args) => {
            handle_danmaku_update(credentials, client_runtime, args).await?;
        }
    }
    Ok(())
}

async fn handle_danmaku_update(
    credentials: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    args: DanmakuUpdateCliArgs,
) -> anyhow::Result<()> {
    let options = danmaku_update_options_from_cli(&args)?;
    let client = BiliClient::new(client_runtime.client_config(credentials.load()?));
    let plan = client
        .plan_download_with_mode(&args.url, args.select, DownloadMode::DanmakuOnly)
        .await?;
    let mut archive = DownloadArchive::load(&args.archive_file)
        .with_context(|| format!("failed to load archive {}", args.archive_file.display()))?;
    ensure_archive_file_does_not_overlap_danmaku_update_targets(
        &args.archive_file,
        &plan,
        &archive,
        &options,
    )?;
    let report = client
        .update_danmaku_for_archive(&plan, &mut archive, options)
        .await?;
    ensure_archive_file_does_not_overlap_danmaku_update(&args.archive_file, &report)?;
    archive
        .save(&args.archive_file)
        .with_context(|| format!("failed to save archive {}", args.archive_file.display()))?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_danmaku_update_report(&report);
    }
    Ok(())
}

fn ensure_archive_file_does_not_overlap_danmaku_update_targets(
    archive_file: &Path,
    plan: &bbdown_core::DownloadPlan,
    archive: &DownloadArchive,
    options: &DanmakuUpdateOptions,
) -> anyhow::Result<()> {
    let mut target_paths = Vec::new();
    for record in &archive.records {
        for archive_entry in &record.entries {
            if !archive_entry_allows_danmaku_update(&record.content_key, archive_entry) {
                continue;
            }
            if !plan.entries.iter().any(|plan_entry| {
                plan_entry.aid == archive_entry.aid && plan_entry.cid == archive_entry.cid
            }) {
                continue;
            }
            let xml_path = archive_entry.directory.join("danmaku.xml");
            push_danmaku_update_source_paths(&mut target_paths, &xml_path);
            push_danmaku_update_generated_paths(&mut target_paths, &xml_path);
            if options.danmaku_formats.contains(DanmakuFormat::Ass) {
                let ass_path = archive_entry.directory.join("danmaku.ass");
                push_danmaku_update_generated_paths(&mut target_paths, &ass_path);
            }
        }
    }
    ensure_archive_file_does_not_overlap_paths(archive_file, &target_paths)
}

fn push_danmaku_update_source_paths(paths: &mut Vec<PathBuf>, path: &Path) {
    let source_path = danmaku_update_temporary_path(path, ".bbdown-source");
    push_unique_path(paths, source_path.clone());
    push_unique_path(
        paths,
        danmaku_update_temporary_path(&source_path, ".bbdown-download"),
    );
    push_unique_path(
        paths,
        danmaku_update_temporary_path(&source_path, ".bbdown-replace"),
    );
}

fn push_danmaku_update_generated_paths(paths: &mut Vec<PathBuf>, path: &Path) {
    push_unique_path(paths, path.to_path_buf());
    push_unique_path(
        paths,
        danmaku_update_temporary_path(path, ".bbdown-generated"),
    );
    push_unique_path(
        paths,
        danmaku_update_temporary_path(path, ".bbdown-replace"),
    );
}

fn danmaku_update_temporary_path(path: &Path, suffix: &str) -> PathBuf {
    let base = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("danmaku");
    path.with_file_name(format!("{base}{suffix}"))
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn ensure_archive_file_does_not_overlap_paths(
    archive_file: &Path,
    target_paths: &[PathBuf],
) -> anyhow::Result<()> {
    let archive_paths = archive_write_paths(archive_file)?;
    for archive_path in archive_paths {
        let archive_lexical = lexical_path_components(&archive_path)?;
        let archive_canonical = canonical_path_components(&archive_path)?;
        for target_path in target_paths {
            ensure!(
                archive_lexical != lexical_path_components(target_path)?
                    && archive_canonical != canonical_path_components(target_path)?,
                "--archive-file and its sidecar files must not overwrite updated danmaku sidecars ({})",
                target_path.display()
            );
        }
    }
    Ok(())
}

fn ensure_archive_file_does_not_overlap_danmaku_update(
    archive_file: &Path,
    report: &bbdown_core::DanmakuUpdateReport,
) -> anyhow::Result<()> {
    ensure_archive_file_does_not_overlap_paths(
        archive_file,
        &report
            .entries
            .iter()
            .flat_map(|entry| entry.files.iter().map(|file| file.path.clone()))
            .collect::<Vec<_>>(),
    )
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
    suppress_human_preflight: bool,
    stdin_is_terminal: bool,
    preflight: &DownloadPreflight,
    cancellation: &DownloadCancellationToken,
    duplicate_prompt_active: &AtomicBool,
) -> anyhow::Result<Option<DuplicateDecision>> {
    if cancellation.is_cancelled() {
        return Err(cancellation.cancelled_error().into());
    }
    if let Some(decision) = explicit {
        return Ok(Some(decision));
    }
    if !preflight.requires_decision() {
        return Ok(None);
    }
    if suppress_human_preflight || !stdin_is_terminal {
        bail!(
            "download archive found an existing record or output conflict; pass --on-duplicate replace, keep-both, or cancel"
        );
    }
    prompt_duplicate_decision(preflight, cancellation, duplicate_prompt_active).map(Some)
}

fn should_prompt_duplicate_decision(
    explicit: Option<DuplicateDecision>,
    json: bool,
    preflight: &DownloadPreflight,
    stdin_is_terminal: bool,
) -> bool {
    explicit.is_none() && preflight.requires_decision() && !json && stdin_is_terminal
}

fn prompt_duplicate_decision(
    preflight: &DownloadPreflight,
    cancellation: &DownloadCancellationToken,
    duplicate_prompt_active: &AtomicBool,
) -> anyhow::Result<DuplicateDecision> {
    let _prompt_active = DuplicatePromptActiveGuard::new(duplicate_prompt_active);
    print_duplicate_preflight(preflight);
    eprintln!("Choose action: [r]eplace, [k]eep-both, [c]ancel");
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("failed to read duplicate decision")?;
    if cancellation.is_cancelled() {
        return Err(cancellation.cancelled_error().into());
    }
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

async fn handle_auth(
    command: AuthCommand,
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
) -> anyhow::Result<()> {
    match command {
        AuthCommand::Status(args) => {
            handle_auth_status(&args, credential_runtime)?;
        }
        AuthCommand::Health(args) => {
            handle_auth_health(&args, credential_runtime, client_runtime).await?;
        }
        AuthCommand::ImportCookie(args) => {
            let mut stored = credential_runtime.load()?;
            let cookie = read_secret(args, "BBDOWN_COOKIE", "cookie")?;
            stored.cookie = Some(cookie);
            credential_runtime.save(&stored)?;
            println!("cookie imported");
        }
        AuthCommand::ImportAccessKey(args) => {
            let mut stored = credential_runtime.load()?;
            let access_key = read_secret(args, "BBDOWN_ACCESS_KEY", "access key")?;
            stored.access_key = Some(access_key);
            credential_runtime.save(&stored)?;
            println!("access key imported");
        }
        AuthCommand::LoginWeb(args) => {
            handle_qr_login(QrLoginKind::Web, args, credential_runtime, client_runtime).await?;
        }
        AuthCommand::LoginTv(args) => {
            handle_qr_login(QrLoginKind::Tv, args, credential_runtime, client_runtime).await?;
        }
        AuthCommand::LoginAccessKey(args) => {
            handle_access_key_login(&args, credential_runtime)?;
        }
        AuthCommand::RenewAccessKey(args) => {
            handle_access_key_renewal(&args, credential_runtime, client_runtime).await?;
        }
        AuthCommand::Logout => {
            credential_runtime.logout()?;
            println!("credentials cleared");
        }
    }
    Ok(())
}

fn handle_auth_status(
    args: &AuthStatusArgs,
    credential_runtime: &CredentialRuntime,
) -> anyhow::Result<()> {
    if !args.profiles && !args.all_profiles {
        println!(
            "{}",
            serde_json::to_string_pretty(&credential_runtime.load()?.redacted_summary())?
        );
        return Ok(());
    }

    let policy = lifecycle_policy_from_seconds(
        args.stale_after_seconds,
        args.expiring_within_seconds,
        current_unix_millis(),
    );
    let (_profiles, selected_profile, statuses) =
        lifecycle_statuses_for_selection(credential_runtime, args.all_profiles, &policy)?;
    let profile_values = statuses
        .iter()
        .map(|status| profile_lifecycle_status_json(status, &selected_profile))
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "selected_profile": selected_profile,
            "profiles": profile_values,
        }))?
    );
    Ok(())
}

async fn handle_auth_health(
    args: &AuthHealthArgs,
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
) -> anyhow::Result<()> {
    if !args.all_profiles {
        let client = BiliClient::new(client_runtime.client_config(credential_runtime.load()?));
        let report = client.check_credential_health().await;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_credential_health_report(&report)?;
            let policy = lifecycle_policy_from_seconds(
                args.stale_after_seconds,
                args.expiring_within_seconds,
                current_unix_millis(),
            );
            let (_profiles, _selected_profile, mut statuses) =
                lifecycle_statuses_for_selection(credential_runtime, false, &policy)?;
            if let Some(status) = statuses.pop() {
                print_credential_guidance(&credential_profile_guidance(&status, Some(&report)))?;
            }
        }
        return Ok(());
    }

    let policy = lifecycle_policy_from_seconds(
        args.stale_after_seconds,
        args.expiring_within_seconds,
        current_unix_millis(),
    );
    let (profiles, selected_profile, statuses) =
        lifecycle_statuses_for_selection(credential_runtime, true, &policy)?;
    let mut profile_reports = Vec::new();
    for status in statuses {
        let credentials = profiles.profile(&status.profile)?;
        let client = BiliClient::new(client_runtime.client_config(credentials));
        let report = client.check_credential_health().await;
        let health_summary = report.summary();
        let guidance = credential_profile_guidance(&status, Some(&report));
        if args.json {
            profile_reports.push(serde_json::json!({
                "profile": &status.profile,
                "is_default_profile": status.is_default_profile,
                "is_selected_profile": status.profile == selected_profile,
                "credentials": &status.credentials,
                "lifecycle": &status,
                "health": &report,
                "health_summary": health_summary,
                "guidance": &guidance,
            }));
        } else {
            print_profile_health_header(&status, &selected_profile, &report)?;
            print_credential_health_report_with_indent(&report, "  ")?;
            print_credential_guidance(&guidance)?;
        }
    }

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "selected_profile": selected_profile,
                "profiles": profile_reports,
            }))?
        );
    }
    Ok(())
}

fn lifecycle_policy_from_seconds(
    stale_after_seconds: u64,
    expiring_within_seconds: u64,
    now_unix_millis: u64,
) -> CredentialLifecyclePolicy {
    CredentialLifecyclePolicy::at_unix_millis(now_unix_millis)
        .with_stale_after_millis(Some(stale_after_seconds.saturating_mul(1_000)))
        .with_expiring_within_millis(Some(expiring_within_seconds.saturating_mul(1_000)))
}

fn lifecycle_statuses_for_selection(
    credential_runtime: &CredentialRuntime,
    all_profiles: bool,
    policy: &CredentialLifecyclePolicy,
) -> anyhow::Result<(
    CredentialProfiles,
    String,
    Vec<CredentialProfileLifecycleStatus>,
)> {
    let profiles = credential_runtime
        .store
        .load_profiles()
        .context("failed to load credential profiles")?;
    let selected_profile = credential_runtime.selected_profile_name(&profiles);
    let statuses = if all_profiles {
        let mut statuses = profiles
            .lifecycle_statuses(policy)
            .context("failed to evaluate credential profile lifecycle status")?;
        if !statuses
            .iter()
            .any(|status| status.profile == selected_profile)
        {
            statuses.push(
                profiles
                    .profile_lifecycle_status(&selected_profile, policy)
                    .context("failed to evaluate credential profile lifecycle status")?,
            );
            statuses.sort_by(|left, right| left.profile.cmp(&right.profile));
        }
        statuses
    } else {
        vec![
            profiles
                .profile_lifecycle_status(&selected_profile, policy)
                .context("failed to evaluate credential profile lifecycle status")?,
        ]
    };
    Ok((profiles, selected_profile, statuses))
}

fn profile_lifecycle_status_json(
    status: &CredentialProfileLifecycleStatus,
    selected_profile: &str,
) -> serde_json::Value {
    serde_json::json!({
        "profile": &status.profile,
        "is_default_profile": status.is_default_profile,
        "is_selected_profile": status.profile == selected_profile,
        "credentials": &status.credentials,
        "status": status.status,
        "credential_statuses": &status.credential_statuses,
        "guidance": credential_profile_guidance(status, None),
    })
}

fn print_credential_health_report(report: &CredentialHealthReport) -> anyhow::Result<()> {
    print_credential_health_report_with_indent(report, "")
}

fn print_credential_health_report_with_indent(
    report: &CredentialHealthReport,
    indent: &str,
) -> anyhow::Result<()> {
    use std::fmt::Write as _;

    for probe in &report.probes {
        let mut line = format!(
            "{indent}{} ({}): {}",
            credential_kind_label(probe.kind),
            credential_health_scope_label(probe.scope),
            credential_health_status_label(probe.status)
        );
        if let Some(endpoint) = &probe.endpoint {
            line.push_str(" via ");
            line.push_str(endpoint);
        }
        if let Some(code) = probe.api_code {
            let _ = write!(&mut line, " code={code}");
        }
        if let Some(message) = &probe.message {
            line.push_str(" - ");
            line.push_str(&display_human_text(message));
        }
        print_human_line(line)?;
    }
    Ok(())
}

fn print_profile_health_header(
    status: &CredentialProfileLifecycleStatus,
    selected_profile: &str,
    report: &CredentialHealthReport,
) -> anyhow::Result<()> {
    let mut markers = Vec::new();
    if status.is_default_profile {
        markers.push("default");
    }
    if status.profile == selected_profile {
        markers.push("selected");
    }
    let suffix = if markers.is_empty() {
        String::new()
    } else {
        format!(" ({})", markers.join(", "))
    };
    print_human_line(format_args!(
        "profile {}{}: lifecycle={} health={}",
        display_profile_name(&status.profile),
        suffix,
        credential_lifecycle_status_label(status.status),
        credential_health_summary_status_label(report.summary().status)
    ))
}

fn display_profile_name(profile: &str) -> String {
    display_human_text(profile)
}

fn display_human_text(value: &str) -> String {
    if value.chars().any(char::is_control) {
        format!("{value:?}")
    } else {
        value.to_owned()
    }
}

fn print_credential_guidance(guidance: &[String]) -> anyhow::Result<()> {
    for item in guidance {
        print_human_line(format_args!("guidance: {item}"))?;
    }
    Ok(())
}

fn credential_profile_guidance(
    status: &CredentialProfileLifecycleStatus,
    report: Option<&CredentialHealthReport>,
) -> Vec<String> {
    let mut guidance = Vec::new();
    if !status.credentials.has_cookie
        && !status.credentials.has_access_key
        && !status.credentials.has_tv_access_key
    {
        push_unique(
            &mut guidance,
            "profile has no credentials; run auth login-web, auth login-tv, auth login-access-key, or import a credential".to_owned(),
        );
    }

    for credential in &status.credential_statuses {
        if !credential.present {
            continue;
        }
        match credential.status {
            CredentialLifecycleStatus::Expired => push_unique(
                &mut guidance,
                format!(
                    "{} lifecycle metadata is expired; {}",
                    credential_kind_label(credential.kind),
                    credential_relogin_hint(credential.kind)
                ),
            ),
            CredentialLifecycleStatus::Expiring => push_unique(
                &mut guidance,
                format!(
                    "{} lifecycle metadata expires soon; renew it before relying on restricted downloads",
                    credential_kind_label(credential.kind)
                ),
            ),
            CredentialLifecycleStatus::Stale => push_unique(
                &mut guidance,
                format!(
                    "{} lifecycle metadata is stale; run auth health or re-login if requests fail",
                    credential_kind_label(credential.kind)
                ),
            ),
            CredentialLifecycleStatus::Unknown => push_unique(
                &mut guidance,
                format!(
                    "{} has no lifecycle metadata; run auth health or re-login if requests fail",
                    credential_kind_label(credential.kind)
                ),
            ),
            CredentialLifecycleStatus::Missing | CredentialLifecycleStatus::Fresh => {}
        }
    }

    if let Some(report) = report {
        for probe in &report.probes {
            match probe.status {
                CredentialHealthStatus::Rejected => push_unique(
                    &mut guidance,
                    format!(
                        "{} was rejected by Bilibili for {}; {}",
                        credential_kind_label(probe.kind),
                        credential_health_scope_label(probe.scope),
                        credential_relogin_hint(probe.kind)
                    ),
                ),
                CredentialHealthStatus::RequestFailed => push_unique(
                    &mut guidance,
                    format!(
                        "{} health check for {} failed; retry after checking network, proxy, or endpoint configuration",
                        credential_kind_label(probe.kind),
                        credential_health_scope_label(probe.scope)
                    ),
                ),
                CredentialHealthStatus::Missing | CredentialHealthStatus::Valid => {}
            }
        }
    }
    guidance
}

fn push_unique(items: &mut Vec<String>, item: String) {
    if !items.contains(&item) {
        items.push(item);
    }
}

fn credential_relogin_hint(kind: CredentialKind) -> &'static str {
    match kind {
        CredentialKind::Cookie => "run auth login-web or auth import-cookie",
        CredentialKind::AccessKey => {
            "run auth renew-access-key, auth login-access-key, or auth import-access-key"
        }
        CredentialKind::TvAccessKey => "run auth login-tv",
    }
}

fn credential_kind_label(kind: CredentialKind) -> &'static str {
    match kind {
        CredentialKind::Cookie => "cookie",
        CredentialKind::AccessKey => "access_key",
        CredentialKind::TvAccessKey => "tv_access_key",
    }
}

fn credential_lifecycle_status_label(status: CredentialLifecycleStatus) -> &'static str {
    match status {
        CredentialLifecycleStatus::Missing => "missing",
        CredentialLifecycleStatus::Unknown => "unknown",
        CredentialLifecycleStatus::Fresh => "fresh",
        CredentialLifecycleStatus::Stale => "stale",
        CredentialLifecycleStatus::Expiring => "expiring",
        CredentialLifecycleStatus::Expired => "expired",
    }
}

fn credential_health_summary_status_label(status: CredentialHealthSummaryStatus) -> &'static str {
    match status {
        CredentialHealthSummaryStatus::Unknown => "unknown",
        CredentialHealthSummaryStatus::Healthy => "healthy",
        CredentialHealthSummaryStatus::Degraded => "degraded",
        CredentialHealthSummaryStatus::Missing => "missing",
        CredentialHealthSummaryStatus::Rejected => "rejected",
        CredentialHealthSummaryStatus::RequestFailed => "request_failed",
    }
}

fn credential_health_scope_label(scope: CredentialHealthScope) -> &'static str {
    match scope {
        CredentialHealthScope::WebCookie => "web",
        CredentialHealthScope::IntlBstar => "intl/bstar",
        CredentialHealthScope::Tv => "tv",
    }
}

fn credential_health_status_label(status: CredentialHealthStatus) -> &'static str {
    match status {
        CredentialHealthStatus::Missing => "missing",
        CredentialHealthStatus::Valid => "valid",
        CredentialHealthStatus::Rejected => "rejected",
        CredentialHealthStatus::RequestFailed => "request_failed",
    }
}

async fn handle_qr_login(
    kind: QrLoginKind,
    args: QrLoginArgs,
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
) -> anyhow::Result<()> {
    ensure!(
        args.timeout_seconds > 0,
        "--timeout-seconds must be greater than 0"
    );
    ensure!(
        args.poll_interval_seconds > 0,
        "--poll-interval-seconds must be greater than 0"
    );
    let client = BiliClient::new(client_runtime.client_config(Credentials::default()));
    let ticket = match kind {
        QrLoginKind::Web => client.create_web_qr_login().await?,
        QrLoginKind::Tv => client.create_tv_qr_login().await?,
    };
    let output = ticket.output();
    if args.json {
        print_qr_ticket_json(&output)?;
    } else {
        print_human_line(format_args!("scan: {}", output.url))?;
    }
    let credentials = wait_for_qr_login(&client, &ticket, &args).await?;
    let summary = save_credentials_with_lifecycle(
        credential_runtime,
        credentials,
        [qr_login_lifecycle_metadata(kind, current_unix_millis())],
    )?;
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

fn handle_access_key_login(
    args: &AccessKeyLoginArgs,
    credential_runtime: &CredentialRuntime,
) -> anyhow::Result<()> {
    let ticket = AccessKeyLoginConfig::new(&args.auth_base, &args.callback_origin)?.ticket()?;
    let output = ticket.output();
    if args.json {
        print_access_key_ticket_json(&output)?;
    } else {
        print_human_line(format_args!("authorization: {}", output.url))?;
        print_human_line(format_args!("qr_payload: {}", output.qr_payload))?;
    }
    let input = read_access_key_login_input(args)?;
    let credentials =
        parse_access_key_login_input(&output, args.message_origin.as_deref(), &input)?;
    let acquired_at_unix_millis = current_unix_millis();
    let summary = save_credentials_with_lifecycle_and_secrets(
        credential_runtime,
        credentials.credentials(),
        [(
            CredentialKind::AccessKey,
            access_key_lifecycle_metadata(&credentials, acquired_at_unix_millis),
        )],
        [access_key_provider_secret(&credentials)],
    )?;
    if args.json {
        print_json_line(&serde_json::json!({
            "event": "saved",
            "kind": "access_key",
            "saved": summary,
        }))?;
    } else {
        print_human_line("access key saved")?;
    }
    Ok(())
}

async fn handle_access_key_renewal(
    args: &AccessKeyRenewalArgs,
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
) -> anyhow::Result<()> {
    let policy = lifecycle_policy_from_seconds(
        args.stale_after_seconds,
        args.expiring_within_seconds,
        current_unix_millis(),
    );
    let (profiles, _selected_profile, mut statuses) =
        lifecycle_statuses_for_selection(credential_runtime, false, &policy)?;
    let status = statuses
        .pop()
        .context("failed to evaluate selected credential profile")?;
    let has_input = args.stdin || args.file.is_some();
    let mut decision = AccessKeyRenewalDecision::from_profile_status(&status, args.force);
    if has_input && decision.action == AccessKeyRenewalAction::NoAction {
        decision = AccessKeyRenewalDecision::from_profile_status(&status, true);
    }

    if args.json {
        print_access_key_renewal_decision_json(&decision)?;
    } else {
        print_access_key_renewal_decision(&decision)?;
    }

    if decision.action == AccessKeyRenewalAction::NoAction {
        return Ok(());
    }

    if try_access_key_auto_refresh(
        args,
        credential_runtime,
        client_runtime,
        &profiles,
        &decision,
        has_input,
    )
    .await?
    {
        return Ok(());
    }

    let ticket = AccessKeyLoginConfig::new(&args.auth_base, &args.callback_origin)?.ticket()?;
    let output = ticket.output();
    if args.json {
        print_access_key_ticket_json(&output)?;
    } else {
        print_human_line(format_args!("authorization: {}", output.url))?;
        print_human_line(format_args!("qr_payload: {}", output.qr_payload))?;
    }

    let Some(input) = read_access_key_renewal_input(args)? else {
        if !args.json {
            print_human_line(
                "complete the browser handoff, then rerun auth renew-access-key with --stdin or --file to save the callback",
            )?;
        }
        return Ok(());
    };
    let credentials =
        parse_access_key_login_input(&output, args.message_origin.as_deref(), &input)?;
    let acquired_at_unix_millis = current_unix_millis();
    let summary = save_credentials_with_lifecycle_and_secrets(
        credential_runtime,
        credentials.credentials(),
        [(
            CredentialKind::AccessKey,
            access_key_lifecycle_metadata(&credentials, acquired_at_unix_millis),
        )],
        [access_key_provider_secret(&credentials)],
    )?;
    if args.json {
        print_json_line(&serde_json::json!({
            "event": "saved",
            "kind": "access_key",
            "saved": summary,
        }))?;
    } else {
        print_human_line("access key saved")?;
    }
    Ok(())
}

async fn try_access_key_auto_refresh(
    args: &AccessKeyRenewalArgs,
    credential_runtime: &CredentialRuntime,
    client_runtime: &ClientRuntimeConfig,
    profiles: &CredentialProfiles,
    decision: &AccessKeyRenewalDecision,
    has_input: bool,
) -> anyhow::Result<bool> {
    if !should_attempt_access_key_auto_refresh(decision, args, has_input) {
        return Ok(false);
    }
    let refresh = match access_key_refresh_request_from_profiles(profiles, &decision.profile) {
        Ok(refresh) => refresh,
        Err(error) => {
            print_access_key_auto_refresh_setup_failure(args.json, &error)?;
            return Ok(false);
        }
    };
    let client = BiliClient::new(client_runtime.client_config(Credentials::default()));
    match client.refresh_access_key(&refresh.request).await {
        Ok(refreshed) => {
            save_refreshed_access_key(credential_runtime, &refresh, &refreshed, args.json)?;
            Ok(true)
        }
        Err(error) => {
            print_access_key_auto_refresh_failure(args.json, &refresh.request, &error)?;
            Ok(false)
        }
    }
}

fn print_access_key_auto_refresh_setup_failure(
    json: bool,
    error: &anyhow::Error,
) -> anyhow::Result<()> {
    let message = error.to_string();
    if json {
        print_json_line(&serde_json::json!({
            "event": "refresh_failed",
            "kind": "access_key",
            "message": message,
        }))
    } else {
        print_human_line(format_args!(
            "automatic refresh failed: {}",
            display_human_text(&message)
        ))
    }
}

fn save_refreshed_access_key(
    credential_runtime: &CredentialRuntime,
    refresh: &StoredAccessKeyRefreshRequest,
    refreshed: &AccessKeyLoginCredentials,
    json: bool,
) -> anyhow::Result<()> {
    let summary = save_refreshed_access_key_silent(credential_runtime, refresh, refreshed)?;
    if json {
        print_json_line(&serde_json::json!({
            "event": "refreshed",
            "kind": "access_key",
            "refresh_provider": refresh.request.refresh_provider,
            "refresh_keypair": refresh.request.refresh_keypair,
        }))?;
        print_json_line(&serde_json::json!({
            "event": "saved",
            "kind": "access_key",
            "saved": summary,
        }))
    } else {
        print_human_line("access key refreshed")?;
        print_human_line("access key saved")
    }
}

fn save_refreshed_access_key_silent(
    credential_runtime: &CredentialRuntime,
    refresh: &StoredAccessKeyRefreshRequest,
    refreshed: &AccessKeyLoginCredentials,
) -> anyhow::Result<bbdown_core::CredentialSource> {
    let acquired_at_unix_millis = current_unix_millis();
    let refreshed_secret = refreshed_access_key_provider_secret(
        refresh.access_key_provider,
        &refresh.request,
        refreshed,
    );
    save_credentials_with_lifecycle_and_secrets(
        credential_runtime,
        refreshed.credentials(),
        [(
            CredentialKind::AccessKey,
            access_key_lifecycle_metadata_with_provider(
                refreshed,
                acquired_at_unix_millis,
                refresh.access_key_provider,
                refreshed_secret.1.has_refresh_token(),
            ),
        )],
        [refreshed_secret],
    )
}

fn print_access_key_auto_refresh_failure(
    json: bool,
    request: &AccessKeyRefreshRequest,
    error: &bbdown_core::Error,
) -> anyhow::Result<()> {
    let message = redact_access_key_refresh_error(error, request);
    if json {
        print_json_line(&serde_json::json!({
            "event": "refresh_failed",
            "kind": "access_key",
            "message": message,
        }))
    } else {
        print_human_line(format_args!(
            "automatic refresh failed: {}",
            display_human_text(&message)
        ))
    }
}

fn redact_access_key_refresh_error(
    error: &bbdown_core::Error,
    request: &AccessKeyRefreshRequest,
) -> String {
    let mut message = error.to_string();
    let mut secrets = [request.access_key.as_str(), request.refresh_token.as_str()];
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.trim().len()));
    for secret in secrets {
        message = redact_exact_secret(&message, secret);
    }
    message
}

fn redact_exact_secret(message: &str, secret: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        message.to_owned()
    } else {
        message.replace(secret, "<redacted>")
    }
}

fn print_qr_ticket_json(output: &QrLoginTicketOutput) -> anyhow::Result<()> {
    print_json_line(&serde_json::json!({
        "event": "ticket",
        "kind": output.kind,
        "url": output.url,
        "qr_payload": output.qr_payload,
    }))
}

fn print_access_key_ticket_json(output: &AccessKeyLoginTicketOutput) -> anyhow::Result<()> {
    print_json_line(&serde_json::json!({
        "event": "ticket",
        "kind": "access_key",
        "url": output.url,
        "qr_payload": output.qr_payload,
        "message_origin": output.message_origin,
        "callback_origin": output.callback_origin,
    }))
}

fn print_access_key_renewal_decision_json(
    decision: &AccessKeyRenewalDecision,
) -> anyhow::Result<()> {
    print_json_line(&serde_json::json!({
        "event": "decision",
        "kind": "access_key",
        "decision": decision,
    }))
}

fn print_access_key_renewal_decision(decision: &AccessKeyRenewalDecision) -> anyhow::Result<()> {
    print_human_line(format_args!(
        "access_key renewal: {} ({})",
        access_key_renewal_action_label(decision.action),
        access_key_renewal_reason_label(decision.reason)
    ))?;
    print_human_line(format_args!(
        "automatic_refresh: {}",
        access_key_automatic_refresh_readiness_label(decision.automatic_refresh_readiness)
    ))
}

fn access_key_renewal_action_label(action: AccessKeyRenewalAction) -> &'static str {
    match action {
        AccessKeyRenewalAction::NoAction => "no_action",
        AccessKeyRenewalAction::Reauthorize => "reauthorize",
        _ => "unknown",
    }
}

fn access_key_renewal_reason_label(reason: AccessKeyRenewalReason) -> &'static str {
    match reason {
        AccessKeyRenewalReason::CredentialMissing => "credential_missing",
        AccessKeyRenewalReason::LifecycleFresh => "lifecycle_fresh",
        AccessKeyRenewalReason::LifecycleUnknown => "lifecycle_unknown",
        AccessKeyRenewalReason::LifecycleStale => "lifecycle_stale",
        AccessKeyRenewalReason::LifecycleExpiring => "lifecycle_expiring",
        AccessKeyRenewalReason::LifecycleExpired => "lifecycle_expired",
        AccessKeyRenewalReason::Forced => "forced",
        _ => "unknown",
    }
}

fn access_key_automatic_refresh_readiness_label(
    readiness: AccessKeyAutomaticRefreshReadiness,
) -> &'static str {
    match readiness {
        AccessKeyAutomaticRefreshReadiness::CredentialMissing => "credential_missing",
        AccessKeyAutomaticRefreshReadiness::Ready => "ready",
        AccessKeyAutomaticRefreshReadiness::UnsupportedSource => "unsupported_source",
        AccessKeyAutomaticRefreshReadiness::MissingRefreshToken => "missing_refresh_token",
        AccessKeyAutomaticRefreshReadiness::MetadataOnlyRefreshToken => {
            "metadata_only_refresh_token"
        }
        AccessKeyAutomaticRefreshReadiness::MissingRefreshProvider => "missing_refresh_provider",
        AccessKeyAutomaticRefreshReadiness::MissingRefreshKeypair => "missing_refresh_keypair",
        AccessKeyAutomaticRefreshReadiness::UnsupportedRefreshProvider => {
            "unsupported_refresh_provider"
        }
        _ => "unknown",
    }
}

#[cfg(test)]
fn save_credentials(
    credential_runtime: &CredentialRuntime,
    credentials: Credentials,
) -> anyhow::Result<bbdown_core::CredentialSource> {
    let mut stored = credential_runtime.load()?;
    merge_credentials(&mut stored, credentials);
    credential_runtime.save(&stored)?;
    Ok(stored.redacted_summary())
}

fn save_credentials_with_lifecycle(
    credential_runtime: &CredentialRuntime,
    credentials: Credentials,
    lifecycle_metadata: impl IntoIterator<Item = (CredentialKind, CredentialLifecycleMetadata)>,
) -> anyhow::Result<bbdown_core::CredentialSource> {
    save_credentials_with_lifecycle_and_secrets(
        credential_runtime,
        credentials,
        lifecycle_metadata,
        std::iter::empty::<(AccessKeyProvider, AccessKeyProviderSecret)>(),
    )
}

fn save_credentials_with_lifecycle_and_secrets(
    credential_runtime: &CredentialRuntime,
    credentials: Credentials,
    lifecycle_metadata: impl IntoIterator<Item = (CredentialKind, CredentialLifecycleMetadata)>,
    access_key_secrets: impl IntoIterator<Item = (AccessKeyProvider, AccessKeyProviderSecret)>,
) -> anyhow::Result<bbdown_core::CredentialSource> {
    let mut profiles = credential_runtime
        .store
        .load_profiles()
        .context("failed to load credential profiles")?;
    let profile_name = credential_runtime
        .selection
        .profile_name()
        .map_or_else(|| profiles.default_profile.clone(), str::to_owned);
    let mut stored = profiles
        .profile(&profile_name)
        .context("failed to load credential profile")?;
    merge_credentials(&mut stored, credentials);
    profiles
        .set_profile(&profile_name, stored.clone())
        .context("failed to update credential profile")?;

    let mut profile_metadata = profiles
        .profile_metadata(&profile_name)
        .context("failed to load credential profile metadata")?;
    for (kind, metadata) in lifecycle_metadata {
        profile_metadata.set_credential(kind, metadata);
    }
    profiles
        .set_profile_metadata(&profile_name, profile_metadata)
        .context("failed to update credential lifecycle metadata")?;

    let mut profile_secrets = profiles
        .profile_secrets(&profile_name)
        .context("failed to load credential profile secrets")?;
    for (provider, secret) in access_key_secrets {
        profile_secrets.set_access_key_provider(provider, secret);
    }
    profiles
        .set_profile_secrets(&profile_name, profile_secrets)
        .context("failed to update credential provider secrets")?;
    credential_runtime
        .store
        .save_profiles(&profiles)
        .context("failed to save credentials")?;
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

fn current_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

fn qr_login_lifecycle_metadata(
    kind: QrLoginKind,
    acquired_at_unix_millis: u64,
) -> (CredentialKind, CredentialLifecycleMetadata) {
    match kind {
        QrLoginKind::Web => (
            CredentialKind::Cookie,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::WebQrLogin)
                .with_acquired_at_unix_millis(acquired_at_unix_millis),
        ),
        QrLoginKind::Tv => (
            CredentialKind::TvAccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::TvQrLogin)
                .with_acquired_at_unix_millis(acquired_at_unix_millis),
        ),
    }
}

fn access_key_lifecycle_metadata(
    credentials: &AccessKeyLoginCredentials,
    acquired_at_unix_millis: u64,
) -> CredentialLifecycleMetadata {
    access_key_lifecycle_metadata_with_provider(
        credentials,
        acquired_at_unix_millis,
        AccessKeyProvider::BalhBiliplus,
        credentials.refresh_token.is_some(),
    )
}

fn access_key_lifecycle_metadata_with_provider(
    credentials: &AccessKeyLoginCredentials,
    acquired_at_unix_millis: u64,
    access_key_provider: AccessKeyProvider,
    refresh_token_present: bool,
) -> CredentialLifecycleMetadata {
    let expires_at_unix_millis = credentials.oauth_expires_at.or_else(|| {
        credentials.expires_in.map(|expires_in| {
            acquired_at_unix_millis.saturating_add(expires_in.saturating_mul(1_000))
        })
    });
    let mut metadata = CredentialLifecycleMetadata::default()
        .with_source(CredentialLifecycleSource::AccessKeyLogin)
        .with_access_key_provider(access_key_provider)
        .with_acquired_at_unix_millis(acquired_at_unix_millis)
        .with_refresh_token_present(refresh_token_present);
    if let Some(expires_at_unix_millis) = expires_at_unix_millis {
        metadata = metadata.with_expires_at_unix_millis(expires_at_unix_millis);
    }
    metadata
}

#[derive(Clone, Debug)]
struct StoredAccessKeyRefreshRequest {
    access_key_provider: AccessKeyProvider,
    request: AccessKeyRefreshRequest,
}

fn should_attempt_access_key_auto_refresh(
    decision: &AccessKeyRenewalDecision,
    args: &AccessKeyRenewalArgs,
    has_input: bool,
) -> bool {
    decision.action == AccessKeyRenewalAction::Reauthorize
        && decision.automatic_refresh_readiness == AccessKeyAutomaticRefreshReadiness::Ready
        && !args.force
        && !has_input
}

fn access_key_refresh_request_from_profiles(
    profiles: &CredentialProfiles,
    profile_name: &str,
) -> anyhow::Result<StoredAccessKeyRefreshRequest> {
    let credentials = profiles
        .profile(profile_name)
        .context("failed to load credential profile")?;
    let access_key = credentials
        .access_key
        .filter(|value| !value.trim().is_empty())
        .context("selected profile has no access_key")?;
    let metadata = profiles
        .profile_metadata(profile_name)
        .context("failed to load credential profile metadata")?;
    let access_key_metadata = metadata
        .credential(CredentialKind::AccessKey)
        .context("selected profile has no access_key lifecycle metadata")?;
    let access_key_provider = access_key_metadata
        .access_key_provider
        .context("selected profile has no access_key provider metadata")?;
    let secrets = profiles
        .profile_secrets(profile_name)
        .context("failed to load credential profile secrets")?;
    let secret = secrets
        .access_key_provider(access_key_provider)
        .context("selected profile has no access_key refresh secret for its provider")?;
    let refresh_token = secret
        .refresh_token
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .context("selected profile has no access_key refresh token secret")?;
    let refresh_provider = secret
        .refresh_provider
        .context("selected profile has no access_key refresh provider")?;
    let mut request =
        AccessKeyRefreshRequest::new(access_key, refresh_token.clone(), refresh_provider)?;
    if refresh_provider == AccessKeyRefreshProvider::BilibiliMainOauth2 {
        request = request.with_refresh_keypair(
            secret
                .refresh_keypair
                .context("selected profile has no access_key refresh keypair")?,
        );
    } else if let Some(refresh_keypair) = secret.refresh_keypair {
        request = request.with_refresh_keypair(refresh_keypair);
    }
    Ok(StoredAccessKeyRefreshRequest {
        access_key_provider,
        request,
    })
}

fn access_key_provider_secret(
    credentials: &AccessKeyLoginCredentials,
) -> (AccessKeyProvider, AccessKeyProviderSecret) {
    let secret = credentials.refresh_token.as_ref().map_or_else(
        AccessKeyProviderSecret::default,
        |refresh_token| {
            AccessKeyProviderSecret::default()
                .with_refresh_token(refresh_token.clone())
                .with_refresh_provider(AccessKeyRefreshProvider::BilibiliMainOauth2)
                .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv)
        },
    );
    (AccessKeyProvider::BalhBiliplus, secret)
}

fn refreshed_access_key_provider_secret(
    provider: AccessKeyProvider,
    request: &AccessKeyRefreshRequest,
    credentials: &AccessKeyLoginCredentials,
) -> (AccessKeyProvider, AccessKeyProviderSecret) {
    let mut secret = AccessKeyProviderSecret::default()
        .with_refresh_token(
            credentials
                .refresh_token
                .clone()
                .unwrap_or_else(|| request.refresh_token.clone()),
        )
        .with_refresh_provider(request.refresh_provider);
    if let Some(refresh_keypair) = request.refresh_keypair {
        secret = secret.with_refresh_keypair(refresh_keypair);
    }
    (provider, secret)
}

fn read_access_key_login_input(args: &AccessKeyLoginArgs) -> anyhow::Result<String> {
    let raw = if args.stdin {
        ensure_access_key_login_stdin_is_safe(io::stdin().is_terminal())?;
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("failed to read access-key login data from stdin")?;
        buffer
    } else if let Some(path) = &args.file {
        let mut file = fs::File::open(path).with_context(|| {
            format!(
                "failed to open access-key login data from {}",
                path.display()
            )
        })?;
        ensure_access_key_login_file_is_safe(path, file.is_terminal())?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer).with_context(|| {
            format!(
                "failed to read access-key login data from {}",
                path.display()
            )
        })?;
        buffer
    } else {
        bail!("provide access-key login data through --stdin with piped input or --file");
    };
    let input = raw.trim_end_matches(['\r', '\n']).to_owned();
    ensure!(!input.trim().is_empty(), "access-key login input is empty");
    Ok(input)
}

fn read_access_key_renewal_input(args: &AccessKeyRenewalArgs) -> anyhow::Result<Option<String>> {
    let raw = if args.stdin {
        ensure_access_key_login_stdin_is_safe(io::stdin().is_terminal())?;
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("failed to read access-key renewal data from stdin")?;
        Some(buffer)
    } else if let Some(path) = &args.file {
        let mut file = fs::File::open(path).with_context(|| {
            format!(
                "failed to open access-key renewal data from {}",
                path.display()
            )
        })?;
        ensure_access_key_login_file_is_safe(path, file.is_terminal())?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer).with_context(|| {
            format!(
                "failed to read access-key renewal data from {}",
                path.display()
            )
        })?;
        Some(buffer)
    } else {
        None
    };
    raw.map(|raw| {
        let input = raw.trim_end_matches(['\r', '\n']).to_owned();
        ensure!(
            !input.trim().is_empty(),
            "access-key renewal input is empty"
        );
        Ok(input)
    })
    .transpose()
}

fn ensure_access_key_login_stdin_is_safe(stdin_is_terminal: bool) -> anyhow::Result<()> {
    if stdin_is_terminal {
        bail!("--stdin requires piped or redirected input to avoid terminal echoing secrets");
    }
    Ok(())
}

fn ensure_access_key_login_file_is_safe(path: &Path, file_is_terminal: bool) -> anyhow::Result<()> {
    if file_is_terminal {
        bail!(
            "--file must not point to a terminal for access-key login data: {}",
            path.display()
        );
    }
    Ok(())
}

fn parse_access_key_login_input(
    ticket: &AccessKeyLoginTicketOutput,
    message_origin: Option<&str>,
    input: &str,
) -> anyhow::Result<AccessKeyLoginCredentials> {
    let input = input.trim();
    if input.starts_with(BALH_LOGIN_CREDENTIALS_PREFIX) {
        return if let Some(origin) = message_origin {
            Ok(ticket.credentials_from_message(origin, input)?)
        } else {
            Ok(AccessKeyLoginCredentials::from_balh_message(input)?)
        };
    }
    ensure!(
        message_origin.is_none(),
        "--message-origin can only be used with balh-login-credentials message input"
    );
    Ok(AccessKeyLoginCredentials::from_balh_payload(input)?)
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
        .with_intl_passport_base(cli.intl_passport_base.clone())
        .with_comment_base(cli.comment_base.clone())
        .with_passport_base(cli.passport_base.clone())
        .with_tv_api_base(cli.tv_api_base.clone())
        .with_app_grpc_base(cli.app_grpc_base.clone())
        .with_app_pgc_grpc_base(cli.app_pgc_grpc_base.clone())
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

fn print_danmaku_update_report(report: &bbdown_core::DanmakuUpdateReport) {
    println!("updated entries: {}", report.entries.len());
    for entry in &report.entries {
        println!(
            "- P{} aid={} cid={} appended={} fetched={} existing={} dir={}",
            entry.index,
            entry.aid,
            entry.cid,
            entry.appended_comments,
            entry.fetched_comments,
            entry.existing_comments,
            entry.directory.display()
        );
        for file in &entry.files {
            println!(
                "  - {:?}: {} bytes -> {}",
                file.kind,
                file.bytes_written,
                file.path.display()
            );
        }
    }
}

#[allow(dead_code)]
fn _assert_credentials_send_sync(_: Credentials) {}

#[cfg(test)]
mod tests {
    use super::{
        Cli, CliProgressReporter, CredentialRuntime, DownloadCtrlCAction, DownloadOnlyArg,
        DuplicateDecisionRequest, DuplicatePromptActiveGuard, SingleDownloadValidationArgs,
        SubtitleAiPolicyArg, access_key_lifecycle_metadata, access_key_provider_secret,
        archive_sidecar_path, credential_profile_selection, download_ctrl_c_action,
        download_mode_may_use_intl_access_key, duplicate_decision_or_report, endpoints_from_cli,
        ensure_access_key_login_file_is_safe, ensure_access_key_login_stdin_is_safe,
        ensure_archive_file_is_not_output_root, http_status_failure_may_be_credential_related,
        input_may_use_intl_access_key, input_may_use_restricted_area_proxy,
        input_media_preflight_playurl_mode, input_requires_web_cookie, next_poll_sleep,
        parse_access_key_login_input, plan_failure_may_be_credential_related,
        qr_login_lifecycle_metadata, remaining_until, restricted_area_from_cli_with_args,
        restricted_area_from_cli_with_env_values, save_credentials,
        save_credentials_with_lifecycle, save_credentials_with_lifecycle_and_secrets,
        should_prompt_duplicate_decision, validate_media_host_spec, validate_single_download_args,
    };
    use bbdown_core::{
        AccessKeyLoginConfig, AccessKeyLoginCredentials, AccessKeyProvider,
        AccessKeyRefreshKeypair, AccessKeyRefreshProvider, CredentialKind,
        CredentialLifecycleMetadata, CredentialLifecycleSource, CredentialProfileSelection,
        CredentialStore, Credentials, DownloadCancellationToken, DownloadMode,
        DownloadOutputConflict, DownloadPreflight, DuplicateDecision, EndpointConfig, Input,
        PlayurlMode, QrLoginKind,
    };
    use clap::Parser as _;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
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
    fn restricted_area_proxy_input_classifier_matches_pgc_planner_inputs() {
        assert!(input_may_use_restricted_area_proxy(&Input::Episode(1000)));
        assert!(input_may_use_restricted_area_proxy(&Input::Season(123)));
        assert!(input_may_use_restricted_area_proxy(&Input::Media(456)));
        assert!(input_may_use_restricted_area_proxy(&Input::ShortLink(
            "https://b23.tv/example".to_owned()
        )));
        assert!(!input_may_use_restricted_area_proxy(&Input::Aid(170_001)));
        assert!(!input_may_use_restricted_area_proxy(&Input::Bvid(
            "BV1xx411c7mD".to_owned()
        )));
        assert!(!input_may_use_restricted_area_proxy(&Input::FavoriteList {
            media_id: Some(456),
            owner_mid: None,
        }));
        assert!(!input_may_use_restricted_area_proxy(
            &Input::RecommendationFeed
        ));
        assert!(!input_may_use_restricted_area_proxy(&Input::IntlEpisode(
            341_736
        )));
        assert!(input_may_use_intl_access_key(&Input::IntlEpisode(341_736)));
        assert!(!input_may_use_intl_access_key(&Input::Episode(1000)));
        assert!(!input_may_use_restricted_area_proxy(&Input::CheeseEpisode(
            101
        )));
    }

    #[test]
    fn web_cookie_input_classifier_matches_account_scoped_feed_inputs() {
        assert!(input_requires_web_cookie(&Input::History));
        assert!(input_requires_web_cookie(&Input::WatchLater));
        assert!(input_requires_web_cookie(&Input::FollowingFeed));
        assert!(!input_requires_web_cookie(&Input::SpaceDynamic(123)));
        assert!(!input_requires_web_cookie(&Input::RecommendationFeed));
        assert!(!input_requires_web_cookie(&Input::FavoriteList {
            media_id: Some(456),
            owner_mid: None,
        }));
        assert!(!input_requires_web_cookie(&Input::Aid(170_001)));
        assert!(!input_requires_web_cookie(&Input::IntlEpisode(341_736)));
    }

    #[test]
    fn media_preflight_playurl_mode_uses_fixed_source_for_intl_and_cheese_inputs() {
        assert_eq!(
            input_media_preflight_playurl_mode(&Input::IntlEpisode(341_736), PlayurlMode::App),
            None
        );
        assert_eq!(
            input_media_preflight_playurl_mode(&Input::CheeseEpisode(101), PlayurlMode::Tv),
            Some(PlayurlMode::Web)
        );
        assert_eq!(
            input_media_preflight_playurl_mode(&Input::Aid(170_001), PlayurlMode::App),
            Some(PlayurlMode::App)
        );
    }

    #[test]
    fn only_media_and_subtitle_download_modes_may_need_intl_access_key() {
        for mode in [
            DownloadMode::All,
            DownloadMode::VideoOnly,
            DownloadMode::AudioOnly,
            DownloadMode::SubtitleOnly,
        ] {
            assert!(download_mode_may_use_intl_access_key(mode));
        }
        for mode in [DownloadMode::DanmakuOnly, DownloadMode::CoverOnly] {
            assert!(!download_mode_may_use_intl_access_key(mode));
        }
    }

    #[test]
    fn plan_failure_classifier_only_treats_auth_like_bad_request_as_credentials() {
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -101,
                message: "not logged in".to_owned(),
            }
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -101,
                message: "账号未登录".to_owned(),
            }
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -101,
                message: "expired access_key".to_owned(),
            }
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -400,
                message: "expired access_key".to_owned(),
            }
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: 16,
                message: "access key expired".to_owned(),
            }
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: 16,
                message: "APP playurl gRPC request failed".to_owned(),
            }
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -403,
                message: "unauthorized access token".to_owned(),
            }
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -403,
                message: "unauthorized".to_owned(),
            }
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -400,
                message: "auth failed".to_owned(),
            }
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -400,
                message: "invalid parameter: qn".to_owned(),
            }
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -400,
                message: "author id invalid".to_owned(),
            }
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: 7,
                message: "area restricted".to_owned(),
            }
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::Api {
                code: -403,
                message: "area restricted".to_owned(),
            }
        ));
    }

    #[test]
    fn plan_failure_classifier_requires_access_key_evidence_for_http_statuses() {
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: API code 7: area restricted".to_owned(),
            )
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: API code -400: author id invalid"
                    .to_owned(),
            )
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: API code -101: not logged in".to_owned(),
            )
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: API code -403: unauthorized".to_owned(),
            )
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: API code -403: invalid proxy credential"
                    .to_owned(),
            )
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed: PgcProxy area=hk Failed (API code -101: redacted diagnostic message)"
                    .to_owned(),
            )
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: HTTP error: HTTP status client error (403 Forbidden)"
                    .to_owned(),
            )
        ));
        assert!(!plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: HTTP error: HTTP status client error (401 Unauthorized)"
                    .to_owned(),
            )
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: API code 16: access key expired"
                    .to_owned(),
            )
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: HTTP error: HTTP status client error (403 Forbidden): access token expired"
                    .to_owned(),
            )
        ));
        assert!(plan_failure_may_be_credential_related(
            &bbdown_core::Error::AccessRestricted(
                "restricted-area resolver failed for hk: HTTP error: HTTP status client error (401 Unauthorized): access_key expired"
                    .to_owned(),
            )
        ));
        assert!(!http_status_failure_may_be_credential_related(
            401,
            "HTTP status client error (401 Unauthorized)"
        ));
        assert!(http_status_failure_may_be_credential_related(
            401,
            "HTTP status client error (401 Unauthorized): expired access token"
        ));
        assert!(!http_status_failure_may_be_credential_related(
            403,
            "HTTP status client error (403 Forbidden)"
        ));
        assert!(http_status_failure_may_be_credential_related(
            403,
            "HTTP status client error (403 Forbidden): expired access_key"
        ));
    }

    #[test]
    fn download_arg_validation_rejects_audio_language_for_video_only() -> anyhow::Result<()> {
        let error = match validate_single_download_args(SingleDownloadValidationArgs {
            only: Some(DownloadOnlyArg::Video),
            no_cover: false,
            no_subtitles: false,
            no_danmaku: false,
            subtitle_ai: SubtitleAiPolicyArg::Include,
            video_quality: None,
            audio_quality: None,
            audio_language: Some("ja-JP"),
        }) {
            Ok(()) => anyhow::bail!("audio language should conflict with video-only mode"),
            Err(error) => error,
        };

        assert_eq!(
            error.to_string(),
            "--only video conflicts with --audio-language"
        );
        Ok(())
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
    fn validate_media_host_spec_accepts_host_or_host_port_only() {
        assert!(validate_media_host_spec("upos.example").is_ok());
        assert!(validate_media_host_spec("upos.example:8443").is_ok());
        assert!(validate_media_host_spec("[::1]:8080").is_ok());

        for invalid in [
            "",
            "https://upos.example",
            "http://127.0.0.1:8080",
            "ftp://upos.example",
            "user@upos.example",
            "upos.example/path",
            "upos.example?query=1",
            "upos.example#fragment",
        ] {
            assert!(
                validate_media_host_spec(invalid).is_err(),
                "{invalid} should be rejected"
            );
        }
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
    fn duplicate_prompt_active_guard_restores_state() {
        let active = AtomicBool::new(false);
        {
            let _guard = DuplicatePromptActiveGuard::new(&active);
            assert!(active.load(Ordering::SeqCst));
        }
        assert!(!active.load(Ordering::SeqCst));
    }

    #[test]
    fn download_ctrl_c_action_forces_exit_for_prompt_or_second_signal() {
        let active = AtomicBool::new(false);

        assert_eq!(
            download_ctrl_c_action(&active, false),
            DownloadCtrlCAction::GracefulCancel
        );
        assert_eq!(
            download_ctrl_c_action(&active, true),
            DownloadCtrlCAction::ForceExit
        );

        active.store(true, Ordering::SeqCst);

        assert_eq!(
            download_ctrl_c_action(&active, false),
            DownloadCtrlCAction::ForceExit
        );
        assert_eq!(
            download_ctrl_c_action(&active, true),
            DownloadCtrlCAction::ForceExit
        );
    }

    #[test]
    fn duplicate_decision_reports_cancelled_before_prompt() {
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
        let cancellation = DownloadCancellationToken::new();
        cancellation.cancel_with_reason("test duplicate prompt cancellation");
        let duplicate_prompt_active = AtomicBool::new(false);

        let result = duplicate_decision_or_report(DuplicateDecisionRequest {
            on_duplicate: None,
            suppress_human_preflight: false,
            stdin_is_terminal: true,
            preflight: &preflight,
            progress: CliProgressReporter { json: false },
            title: "Mock video",
            cancellation: &cancellation,
            duplicate_prompt_active: &duplicate_prompt_active,
        });

        assert!(result.is_err(), "pre-cancelled duplicate prompt succeeded");
        if let Err(error) = result {
            assert!(
                error
                    .to_string()
                    .contains("test duplicate prompt cancellation")
            );
        }
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
    fn app_grpc_cli_defaults_match_core_defaults() {
        let cli = Cli::parse_from(["bbdown", "auth", "status"]);
        let endpoints = endpoints_from_cli(&cli);
        let defaults = EndpointConfig::default();

        assert_eq!(endpoints.app_grpc_base, defaults.app_grpc_base);
        assert_eq!(endpoints.app_pgc_grpc_base, defaults.app_pgc_grpc_base);
        assert_eq!(endpoints.intl_passport_base, defaults.intl_passport_base);
    }

    #[test]
    fn intl_passport_base_cli_arg_overrides_default() {
        let cli = Cli::parse_from([
            "bbdown",
            "--intl-passport-base",
            "http://127.0.0.1:8082",
            "auth",
            "status",
        ]);
        let endpoints = endpoints_from_cli(&cli);

        assert_eq!(endpoints.intl_passport_base, "http://127.0.0.1:8082");
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
    fn credential_profile_cli_arg_builds_named_selection() -> anyhow::Result<()> {
        let cli = Cli::parse_from(["bbdown", "--credential-profile", "intl", "auth", "status"]);
        let selection = credential_profile_selection(cli.credential_profile)?;

        assert_eq!(
            selection,
            CredentialProfileSelection::Named("intl".to_owned())
        );
        Ok(())
    }

    #[test]
    fn blank_credential_profile_cli_arg_is_rejected() {
        let cli = Cli::parse_from(["bbdown", "--credential-profile", " ", "auth", "status"]);

        assert!(credential_profile_selection(cli.credential_profile).is_err());
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
    fn parse_access_key_login_input_accepts_message_origins() -> anyhow::Result<()> {
        let ticket = AccessKeyLoginConfig::biliplus("https://www.bilibili.com/video/BV1")?
            .ticket()?
            .output();
        let message = r#"balh-login-credentials: {"access_key":"AK"}"#;

        let from_auth_origin =
            parse_access_key_login_input(&ticket, Some("https://www.biliplus.com/login"), message)?;
        let from_callback_origin =
            parse_access_key_login_input(&ticket, Some("https://www.bilibili.com/watch"), message)?;
        let trusted_manual_message = parse_access_key_login_input(&ticket, None, message)?;

        assert_eq!(from_auth_origin.access_key, "AK");
        assert_eq!(from_callback_origin.access_key, "AK");
        assert_eq!(trusted_manual_message.access_key, "AK");
        Ok(())
    }

    #[test]
    fn parse_access_key_login_input_rejects_untrusted_or_mismatched_shapes() -> anyhow::Result<()> {
        let ticket = AccessKeyLoginConfig::biliplus("https://www.bilibili.com")?
            .ticket()?
            .output();
        let message = r#"balh-login-credentials: {"access_key":"AK"}"#;

        let bad_origin =
            parse_access_key_login_input(&ticket, Some("https://attacker.example"), message)
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
        let raw_payload_with_origin = parse_access_key_login_input(
            &ticket,
            Some("https://www.biliplus.com"),
            "access_key=AK",
        )
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();

        assert!(bad_origin.contains("access-key login message origin does not match ticket"));
        assert!(raw_payload_with_origin.contains("--message-origin can only be used"));
        Ok(())
    }

    #[test]
    fn access_key_login_stdin_guard_rejects_terminal_input() {
        let explicit_error = ensure_access_key_login_stdin_is_safe(true)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();

        assert!(explicit_error.contains("--stdin requires piped or redirected input"));
        assert!(ensure_access_key_login_stdin_is_safe(false).is_ok());
    }

    #[test]
    fn access_key_login_file_guard_rejects_terminal_input() {
        let error = ensure_access_key_login_file_is_safe(Path::new("/dev/tty"), true)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();

        assert!(error.contains("--file must not point to a terminal"));
        assert!(ensure_access_key_login_file_is_safe(Path::new("credentials.txt"), false).is_ok());
    }

    #[test]
    fn save_credentials_merges_with_current_store() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        store.save(&Credentials {
            cookie: Some("SESSDATA=fresh".to_owned()),
            access_key: Some("BSTAR".to_owned()),
            tv_access_key: None,
        })?;
        let runtime =
            CredentialRuntime::new(store.clone(), CredentialProfileSelection::default_profile());

        let summary = save_credentials(
            &runtime,
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

    #[test]
    fn save_credentials_merges_with_selected_profile() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        store.save(&Credentials {
            cookie: Some("SESSDATA=default".to_owned()),
            access_key: None,
            tv_access_key: None,
        })?;
        let runtime =
            CredentialRuntime::new(store.clone(), CredentialProfileSelection::named("tv")?);

        let summary = save_credentials(
            &runtime,
            Credentials {
                cookie: None,
                access_key: None,
                tv_access_key: Some("TV".to_owned()),
            },
        )?;

        assert_eq!(
            store.load()?,
            Credentials {
                cookie: Some("SESSDATA=default".to_owned()),
                access_key: None,
                tv_access_key: None,
            }
        );
        assert_eq!(
            store.load_profile("tv")?,
            Credentials {
                cookie: None,
                access_key: None,
                tv_access_key: Some("TV".to_owned()),
            }
        );
        assert!(!summary.has_cookie);
        assert!(!summary.has_access_key);
        assert!(summary.has_tv_access_key);
        Ok(())
    }

    #[test]
    fn access_key_lifecycle_metadata_records_absolute_and_relative_expiry() -> anyhow::Result<()> {
        let now = 1_700_000_000_000;
        let absolute_credentials = AccessKeyLoginCredentials::from_balh_payload(
            "access_key=ACCESS&refresh_token=REFRESH&oauth_expires_at=1700000120000&expires_in=60",
        )?;
        let absolute = access_key_lifecycle_metadata(&absolute_credentials, now);
        assert_eq!(
            absolute.source,
            Some(CredentialLifecycleSource::AccessKeyLogin)
        );
        assert_eq!(
            absolute.access_key_provider,
            Some(AccessKeyProvider::BalhBiliplus)
        );
        assert_eq!(absolute.acquired_at_unix_millis, Some(now));
        assert_eq!(absolute.expires_at_unix_millis, Some(now + 120_000));
        assert_eq!(absolute.refresh_token_present, Some(true));
        let (_, secret) = access_key_provider_secret(&absolute_credentials);
        assert_eq!(
            secret.refresh_provider,
            Some(AccessKeyRefreshProvider::BilibiliMainOauth2)
        );
        assert_eq!(
            secret.refresh_keypair,
            Some(AccessKeyRefreshKeypair::BiliTv)
        );
        assert_eq!(secret.refresh_token.as_deref(), Some("REFRESH"));

        let relative_credentials =
            AccessKeyLoginCredentials::from_balh_payload("access_key=ACCESS&expires_in=60")?;
        let relative = access_key_lifecycle_metadata(&relative_credentials, now);
        assert_eq!(relative.expires_at_unix_millis, Some(now + 60_000));
        assert_eq!(relative.refresh_token_present, Some(false));
        let (_, relative_secret) = access_key_provider_secret(&relative_credentials);
        assert!(relative_secret.is_empty());
        Ok(())
    }

    #[test]
    fn save_credentials_with_lifecycle_records_default_profile_metadata() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        store.save(&Credentials {
            cookie: Some("SESSDATA=default".to_owned()),
            access_key: None,
            tv_access_key: None,
        })?;
        let runtime =
            CredentialRuntime::new(store.clone(), CredentialProfileSelection::default_profile());
        let now = 1_700_000_000_000;

        let summary = save_credentials_with_lifecycle(
            &runtime,
            Credentials {
                cookie: None,
                access_key: Some("ACCESS".to_owned()),
                tv_access_key: None,
            },
            [(
                CredentialKind::AccessKey,
                CredentialLifecycleMetadata::default()
                    .with_source(CredentialLifecycleSource::AccessKeyLogin)
                    .with_acquired_at_unix_millis(now)
                    .with_expires_at_unix_millis(now + 60_000)
                    .with_refresh_token_present(true),
            )],
        )?;

        assert!(summary.has_cookie);
        assert!(summary.has_access_key);
        let saved = store.load()?;
        assert_eq!(saved.cookie.as_deref(), Some("SESSDATA=default"));
        assert_eq!(saved.access_key.as_deref(), Some("ACCESS"));
        let profiles = store.load_profiles()?;
        let metadata = profiles.profile_metadata("default")?;
        let access_key_metadata = metadata
            .credential(CredentialKind::AccessKey)
            .ok_or_else(|| anyhow::anyhow!("missing access-key lifecycle metadata"))?;
        assert_eq!(
            access_key_metadata.source,
            Some(CredentialLifecycleSource::AccessKeyLogin)
        );
        assert_eq!(access_key_metadata.acquired_at_unix_millis, Some(now));
        assert_eq!(
            access_key_metadata.expires_at_unix_millis,
            Some(now + 60_000)
        );
        assert_eq!(access_key_metadata.refresh_token_present, Some(true));
        Ok(())
    }

    #[test]
    fn save_credentials_with_lifecycle_and_secrets_records_access_key_provider_secret()
    -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        let runtime =
            CredentialRuntime::new(store.clone(), CredentialProfileSelection::default_profile());
        let credentials = AccessKeyLoginCredentials::from_balh_payload(
            "access_key=ACCESS&refresh_token=REFRESH&expires_in=60",
        )?;
        let now = 1_700_000_000_000;

        let summary = save_credentials_with_lifecycle_and_secrets(
            &runtime,
            credentials.credentials(),
            [(
                CredentialKind::AccessKey,
                access_key_lifecycle_metadata(&credentials, now),
            )],
            [access_key_provider_secret(&credentials)],
        )?;

        assert!(summary.has_access_key);
        let profiles = store.load_profiles()?;
        let secrets = profiles.profile_secrets("default")?;
        let secret = secrets
            .access_key_provider(AccessKeyProvider::BalhBiliplus)
            .ok_or_else(|| anyhow::anyhow!("missing BALH/BiliPlus provider secret"))?;
        assert_eq!(secret.refresh_token.as_deref(), Some("REFRESH"));
        assert_eq!(
            secret.refresh_provider,
            Some(AccessKeyRefreshProvider::BilibiliMainOauth2)
        );
        assert_eq!(
            secret.refresh_keypair,
            Some(AccessKeyRefreshKeypair::BiliTv)
        );
        Ok(())
    }

    #[test]
    fn save_credentials_with_lifecycle_and_secrets_clears_stale_provider_secret()
    -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CredentialStore::new(temp.path().join("credentials.json"));
        let runtime =
            CredentialRuntime::new(store.clone(), CredentialProfileSelection::default_profile());
        let initial_credentials = AccessKeyLoginCredentials::from_balh_payload(
            "access_key=ACCESS&refresh_token=REFRESH&expires_in=60",
        )?;
        let now = 1_700_000_000_000;
        save_credentials_with_lifecycle_and_secrets(
            &runtime,
            initial_credentials.credentials(),
            [(
                CredentialKind::AccessKey,
                access_key_lifecycle_metadata(&initial_credentials, now),
            )],
            [access_key_provider_secret(&initial_credentials)],
        )?;

        let replacement_credentials =
            AccessKeyLoginCredentials::from_balh_payload("access_key=ACCESS&expires_in=60")?;
        save_credentials_with_lifecycle_and_secrets(
            &runtime,
            replacement_credentials.credentials(),
            [(
                CredentialKind::AccessKey,
                access_key_lifecycle_metadata(&replacement_credentials, now + 1_000),
            )],
            [access_key_provider_secret(&replacement_credentials)],
        )?;

        let profiles = store.load_profiles()?;
        let metadata = profiles.profile_metadata("default")?;
        let access_key_metadata = metadata
            .credential(CredentialKind::AccessKey)
            .ok_or_else(|| anyhow::anyhow!("missing access-key lifecycle metadata"))?;
        assert_eq!(access_key_metadata.refresh_token_present, Some(false));
        let secrets = profiles.profile_secrets("default")?;
        assert!(
            secrets
                .access_key_provider(AccessKeyProvider::BalhBiliplus)
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn qr_login_lifecycle_metadata_records_source_and_acquisition_time() {
        let now = 1_700_000_000_000;
        let (web_kind, web_metadata) = qr_login_lifecycle_metadata(QrLoginKind::Web, now);
        assert_eq!(web_kind, CredentialKind::Cookie);
        assert_eq!(
            web_metadata.source,
            Some(CredentialLifecycleSource::WebQrLogin)
        );
        assert_eq!(web_metadata.acquired_at_unix_millis, Some(now));
        assert_eq!(web_metadata.expires_at_unix_millis, None);

        let (tv_kind, tv_metadata) = qr_login_lifecycle_metadata(QrLoginKind::Tv, now);
        assert_eq!(tv_kind, CredentialKind::TvAccessKey);
        assert_eq!(
            tv_metadata.source,
            Some(CredentialLifecycleSource::TvQrLogin)
        );
        assert_eq!(tv_metadata.acquired_at_unix_millis, Some(now));
        assert_eq!(tv_metadata.expires_at_unix_millis, None);
    }
}

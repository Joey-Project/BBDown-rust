#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

use anyhow::{Context, bail, ensure};
use bbdown::{
    BiliClient, ClientConfig, CredentialStore, Credentials, DownloadOptions, DownloadReport,
    EndpointConfig, MuxOptions, QrLoginKind, QrLoginState, QrLoginTicket, ResolvedContent,
    RestrictedArea, RestrictedAreaConfig, RestrictedAreaProxy, RestrictedAreaProxyKind,
    RetryPolicy, Selection,
};
use clap::{Args, Parser, Subcommand};
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Parser)]
#[command(name = "bbdown")]
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
        #[arg(long)]
        no_subtitles: bool,
        #[arg(long)]
        no_danmaku: bool,
        #[arg(long)]
        no_mux: bool,
        #[arg(long, default_value = "ffmpeg")]
        ffmpeg: PathBuf,
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
    let cli = Cli::parse();
    ensure!(
        cli.request_timeout_seconds > 0,
        "--request-timeout-seconds must be greater than 0"
    );
    let endpoints = endpoints_from_cli(&cli);
    let restricted_area = restricted_area_from_cli(&cli)?;
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
            no_subtitles,
            no_danmaku,
            no_mux,
            ffmpeg,
        } => {
            ensure!(
                retry_attempts > 0,
                "--retry-attempts must be greater than 0"
            );
            let args = DownloadCommandArgs {
                url,
                select,
                json,
                options: DownloadOptions {
                    output_dir,
                    retry: RetryPolicy {
                        max_attempts: retry_attempts,
                        backoff: Duration::from_millis(retry_backoff_ms),
                    },
                    download_idle_timeout: if download_idle_timeout_seconds == 0 {
                        None
                    } else {
                        Some(Duration::from_secs(download_idle_timeout_seconds))
                    },
                    resume: !no_resume,
                    include_subtitles: !no_subtitles,
                    include_danmaku: !no_danmaku,
                    mux: if no_mux {
                        MuxOptions::Disabled
                    } else {
                        MuxOptions::Ffmpeg { binary: ffmpeg }
                    },
                },
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
        println!("title: {}", plan.title);
        println!("entries: {}", plan.entries.len());
        for entry in plan.entries {
            println!(
                "- P{} aid={} cid={} title={} video={} audio={} flv={} subtitles={} danmaku={}",
                entry.index,
                entry.aid,
                entry.cid,
                entry.title,
                entry.streams.videos.len(),
                entry.streams.audios.len(),
                entry.streams.flv_segments.len(),
                entry.subtitles.len(),
                entry.danmaku.xml_url
            );
        }
    }
    Ok(())
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
    let report = client
        .download_input(&args.url, args.select, args.options)
        .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_download_report(&report);
    }
    Ok(())
}

fn client_config(
    endpoints: EndpointConfig,
    restricted_area: RestrictedAreaConfig,
    request_timeout: Duration,
    credentials: Credentials,
) -> ClientConfig {
    ClientConfig {
        endpoints,
        credentials,
        restricted_area,
        user_agent: "bbdown-rs/0.1".to_owned(),
        request_timeout,
    }
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
) -> anyhow::Result<bbdown::CredentialSource> {
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
        .unwrap_or(default_endpoints.tv_passport_base);
    let tv_passport_poll_base = cli.tv_passport_poll_base.clone().unwrap_or_else(|| {
        if cli.tv_passport_base.is_some() {
            tv_passport_base.clone()
        } else {
            default_endpoints.tv_passport_poll_base
        }
    });
    EndpointConfig {
        api_base: cli.api_base.clone(),
        pgc_base: cli.pgc_base.clone(),
        intl_base: cli.intl_base.clone(),
        comment_base: cli.comment_base.clone(),
        passport_base: cli.passport_base.clone(),
        tv_passport_base,
        tv_passport_poll_base,
    }
}

fn restricted_area_from_cli(cli: &Cli) -> anyhow::Result<RestrictedAreaConfig> {
    let area_hint = cli
        .restricted_area
        .as_deref()
        .map(parse_restricted_area)
        .transpose()?;
    let mut proxies = Vec::new();
    for spec in &cli.restricted_area_proxy {
        proxies.push(parse_restricted_proxy_spec(
            spec,
            RestrictedAreaProxyKind::PlayUrl,
        )?);
    }
    for spec in &cli.restricted_api_proxy {
        proxies.push(parse_restricted_proxy_spec(
            spec,
            RestrictedAreaProxyKind::BilibiliApi,
        )?);
    }
    Ok(RestrictedAreaConfig { area_hint, proxies })
}

fn parse_restricted_proxy_spec(
    spec: &str,
    kind: RestrictedAreaProxyKind,
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
    url::Url::parse(base_url)
        .with_context(|| format!("failed to parse restricted-area proxy URL from `{trimmed}`"))?;
    Ok(match kind {
        RestrictedAreaProxyKind::PlayUrl => RestrictedAreaProxy::playurl(base_url, area),
        RestrictedAreaProxyKind::BilibiliApi => RestrictedAreaProxy::bilibili_api(base_url, area),
    })
}

fn parse_area_prefixed_proxy(spec: &str) -> anyhow::Result<Option<(&str, &str)>> {
    let Some((area, base_url)) = spec.split_once('=') else {
        return Ok(None);
    };
    if spec.starts_with("http://") || spec.starts_with("https://") {
        return Ok(None);
    }
    match area.trim().to_ascii_lowercase().as_str() {
        "cn" | "th" | "hk" | "tw" => Ok(Some((area, base_url))),
        other => bail!("unsupported restricted area `{other}`; expected cn, th, hk, or tw"),
    }
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
        Cli, endpoints_from_cli, next_poll_sleep, remaining_until, restricted_area_from_cli,
        save_qr_credentials,
    };
    use bbdown::{CredentialStore, Credentials, EndpointConfig};
    use clap::Parser as _;
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
        let cli = Cli::parse_from([
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
        ]);
        let config = restricted_area_from_cli(&cli)?;
        let ordered = config.ordered_proxies();

        assert_eq!(ordered[0].base_url, "https://hk.example/api");
        assert_eq!(ordered[1].base_url, "https://generic.example/playurl");
        assert_eq!(ordered[2].base_url, "https://tw.example/api");
        Ok(())
    }

    #[test]
    fn restricted_area_proxy_bare_url_may_contain_query_equals() -> anyhow::Result<()> {
        let cli = Cli::parse_from([
            "bbdown",
            "--restricted-area-proxy",
            "https://generic.example/playurl?token=a=b",
            "auth",
            "status",
        ]);
        let config = restricted_area_from_cli(&cli)?;

        assert_eq!(config.proxies[0].area, None);
        assert_eq!(
            config.proxies[0].base_url,
            "https://generic.example/playurl?token=a=b"
        );
        Ok(())
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

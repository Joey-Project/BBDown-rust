#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

use anyhow::{Context, bail, ensure};
use bbdown::{
    BiliClient, ClientConfig, CredentialStore, Credentials, DownloadOptions, DownloadReport,
    EndpointConfig, MuxOptions, QrLoginKind, QrLoginState, ResolvedContent, RetryPolicy, Selection,
};
use clap::{Args, Parser, Subcommand};
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Duration;

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
    #[arg(
        long,
        env = "BBDOWN_TV_PASSPORT_BASE",
        default_value = "https://passport.snm0516.aisee.tv"
    )]
    tv_passport_base: String,
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
    let endpoints = EndpointConfig {
        api_base: cli.api_base.clone(),
        pgc_base: cli.pgc_base.clone(),
        intl_base: cli.intl_base.clone(),
        comment_base: cli.comment_base.clone(),
        passport_base: cli.passport_base.clone(),
        tv_passport_base: cli.tv_passport_base.clone(),
    };
    let request_timeout = Duration::from_secs(cli.request_timeout_seconds);
    let store = CredentialStore::new(credential_path(cli.credential_file)?);
    match cli.command {
        Command::Info { url, select, json } => {
            handle_info(
                &store,
                endpoints.clone(),
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
            handle_download(&store, endpoints, request_timeout, args).await?;
        }
        Command::Auth { command } => {
            handle_auth(command, &store, endpoints, request_timeout).await?;
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
    request_timeout: Duration,
    url: String,
    select: Option<Selection>,
    json: bool,
) -> anyhow::Result<()> {
    let credentials = store.load().context("failed to load credentials")?;
    let client = BiliClient::new(client_config(endpoints, request_timeout, credentials));
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
    request_timeout: Duration,
    url: String,
    select: Option<Selection>,
    json: bool,
) -> anyhow::Result<()> {
    let credentials = store.load().context("failed to load credentials")?;
    let client = BiliClient::new(client_config(endpoints, request_timeout, credentials));
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
    request_timeout: Duration,
    args: DownloadCommandArgs,
) -> anyhow::Result<()> {
    let credentials = store.load().context("failed to load credentials")?;
    let client = BiliClient::new(client_config(endpoints, request_timeout, credentials));
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
    request_timeout: Duration,
    credentials: Credentials,
) -> ClientConfig {
    ClientConfig {
        endpoints,
        credentials,
        user_agent: "bbdown-rs/0.1".to_owned(),
        request_timeout,
    }
}

async fn handle_auth(
    command: AuthCommand,
    store: &CredentialStore,
    endpoints: EndpointConfig,
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
            handle_qr_login(QrLoginKind::Web, args, store, endpoints, request_timeout).await?;
        }
        AuthCommand::LoginTv(args) => {
            handle_qr_login(QrLoginKind::Tv, args, store, endpoints, request_timeout).await?;
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
        request_timeout,
        store.load().context("failed to load credentials")?,
    ));
    let ticket = match kind {
        QrLoginKind::Web => client.create_web_qr_login().await?,
        QrLoginKind::Tv => client.create_tv_qr_login().await?,
    };
    if !args.json {
        println!("scan: {}", ticket.url);
    }
    let credentials = wait_for_qr_login(&client, kind, &ticket.key, &args).await?;
    let mut stored = store.load().context("failed to load credentials")?;
    if credentials.cookie.is_some() {
        stored.cookie = credentials.cookie;
    }
    if credentials.access_key.is_some() {
        stored.access_key = credentials.access_key;
    }
    store.save(&stored).context("failed to save credentials")?;
    let summary = stored.redacted_summary();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "kind": kind,
                "saved": summary,
            }))?
        );
    } else {
        println!("credentials saved");
    }
    Ok(())
}

async fn wait_for_qr_login(
    client: &BiliClient,
    kind: QrLoginKind,
    key: &str,
    args: &QrLoginArgs,
) -> anyhow::Result<Credentials> {
    let interval = Duration::from_secs(args.poll_interval_seconds);
    let max_attempts = args.timeout_seconds / args.poll_interval_seconds + 1;
    let mut last_waiting_state: Option<&'static str> = None;
    for attempt in 0..max_attempts {
        let state = match kind {
            QrLoginKind::Web => client.poll_web_qr_login(key).await?,
            QrLoginKind::Tv => client.poll_tv_qr_login(key).await?,
        };
        match state {
            QrLoginState::WaitingForScan => {
                if !args.json && last_waiting_state != Some("waiting_for_scan") {
                    println!("waiting for scan");
                }
                last_waiting_state = Some("waiting_for_scan");
            }
            QrLoginState::WaitingForConfirm => {
                if !args.json && last_waiting_state != Some("waiting_for_confirm") {
                    println!("waiting for confirmation");
                }
                last_waiting_state = Some("waiting_for_confirm");
            }
            QrLoginState::Expired => bail!("QR code expired"),
            QrLoginState::Succeeded { credentials } => return Ok(credentials),
        }
        if attempt + 1 < max_attempts {
            tokio::time::sleep(interval).await;
        }
    }
    bail!("QR login timed out")
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

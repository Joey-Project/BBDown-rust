#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

use anyhow::{Context, bail, ensure};
use bbdown::{
    BiliClient, ClientConfig, CredentialStore, Credentials, DownloadOptions, DownloadReport,
    EndpointConfig, MuxOptions, ResolvedContent, RetryPolicy, Selection,
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
    Logout,
}

#[derive(Debug, Args)]
struct SecretImportArgs {
    #[arg(long, conflicts_with = "file")]
    stdin: bool,
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,
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
        Command::Auth { command } => handle_auth(command, &store)?,
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

fn handle_auth(command: AuthCommand, store: &CredentialStore) -> anyhow::Result<()> {
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
        AuthCommand::Logout => {
            store.clear().context("failed to clear credentials")?;
            println!("credentials cleared");
        }
    }
    Ok(())
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

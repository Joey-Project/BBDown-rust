#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

use anyhow::{Context, bail, ensure};
use bbdown::{
    BiliClient, ClientConfig, CredentialStore, Credentials, EndpointConfig, ResolvedContent,
    Selection,
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
    let store = CredentialStore::new(credential_path(cli.credential_file)?);
    match cli.command {
        Command::Info { url, select, json } => {
            let credentials = store.load().context("failed to load credentials")?;
            let client = BiliClient::new(ClientConfig {
                endpoints: EndpointConfig {
                    api_base: cli.api_base,
                    pgc_base: cli.pgc_base,
                    intl_base: cli.intl_base,
                },
                credentials,
                user_agent: "bbdown-rs/0.1".to_owned(),
                request_timeout: Duration::from_secs(cli.request_timeout_seconds),
            });
            let resolved = client.resolve_input(&url, select).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&resolved)?);
            } else {
                print_human_summary(&resolved);
            }
        }
        Command::Plan { url, select, json } => {
            let credentials = store.load().context("failed to load credentials")?;
            let client = BiliClient::new(ClientConfig {
                endpoints: EndpointConfig {
                    api_base: cli.api_base,
                    pgc_base: cli.pgc_base,
                    intl_base: cli.intl_base,
                },
                credentials,
                user_agent: "bbdown-rs/0.1".to_owned(),
                request_timeout: Duration::from_secs(cli.request_timeout_seconds),
            });
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
        }
        Command::Auth { command } => handle_auth(command, &store)?,
    }
    Ok(())
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

#[allow(dead_code)]
fn _assert_credentials_send_sync(_: Credentials) {}

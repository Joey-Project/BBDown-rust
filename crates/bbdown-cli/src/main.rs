#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

use anyhow::Context;
use bbdown::{
    BiliClient, ClientConfig, CredentialStore, Credentials, EndpointConfig, ResolvedContent,
    Selection,
};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    Status,
    ImportCookie { cookie: String },
    ImportAccessKey { access_key: String },
    Logout,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
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
            });
            let resolved = client.resolve_input(&url, select).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&resolved)?);
            } else {
                print_human_summary(&resolved);
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
        AuthCommand::ImportCookie { cookie } => {
            let mut credentials = store.load().context("failed to load credentials")?;
            credentials.cookie = Some(cookie);
            store
                .save(&credentials)
                .context("failed to save credentials")?;
            println!("cookie imported");
        }
        AuthCommand::ImportAccessKey { access_key } => {
            let mut credentials = store.load().context("failed to load credentials")?;
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

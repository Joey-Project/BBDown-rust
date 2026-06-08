# Embedding Guide

## Scope

The `bbdown` crate is the integration surface for Rust projects that need typed Bilibili metadata,
download plans, media downloads, subtitle sidecars, danmaku sidecars, QR login state, and
restricted-area proxy diagnostics without shelling out to the CLI.

The crate is still pre-release. Prefer constructors and builder-style APIs for configuration, and
treat metadata and plan structs as read-only output surfaces. This keeps embedding code resilient
when new fields are added before the first crates.io release.

## Planning Only

Use `BiliClient::plan_download` for raw CLI-style inputs, or parse an `Input` yourself and call
`BiliClient::plan`.

```rust,no_run
use bbdown::{BiliClient, ClientConfig, Selection};

#[tokio::main]
async fn main() -> bbdown::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download("https://www.bilibili.com/video/BV1qt4y1X7TW", Some(Selection::Current))
        .await?;

    for entry in &plan.entries {
        println!("{}: {} streams", entry.title, entry.streams.videos.len());
    }

    Ok(())
}
```

Season and media inputs require an explicit `Selection` unless the input itself identifies an
episode. This is intentional so libraries cannot accidentally plan a full season.

## Credentials

Embedding projects can either use `CredentialStore` or inject credentials from their own storage.
Do not log raw credential values. `Credentials` debug output is redacted, but application logs should
still treat credentials as secrets.

```rust,no_run
use bbdown::{ClientConfig, Credentials};

let credentials = Credentials::default()
    .with_cookie("SESSDATA=...")
    .with_access_key("...");

let config = ClientConfig::default().with_credentials(credentials);
```

## Restricted-Area PGC Planning

The crate never ships public proxy defaults. Configure only proxy hosts you operate or trust.
Restricted-area fallback is attempted for PGC playurl region errors, not for arbitrary official API
failures.

```rust,no_run
use bbdown::{
    BiliClient, ClientConfig, RestrictedArea, RestrictedAreaConfig, RestrictedAreaProxy, Selection,
};

#[tokio::main]
async fn main() -> bbdown::Result<()> {
    let restricted_area = RestrictedAreaConfig::default()
        .with_area_hint(RestrictedArea::Hk)
        .with_proxy(RestrictedAreaProxy::playurl(
            "https://proxy.example/playurl",
            Some(RestrictedArea::Hk),
        ))
        .with_proxy(RestrictedAreaProxy::bilibili_api(
            "https://api-proxy.example",
            Some(RestrictedArea::Tw),
        ));

    let client = BiliClient::new(ClientConfig::default().with_restricted_area(restricted_area));
    let plan = client.plan_download("ep664928", Some(Selection::Current)).await?;

    println!("planned {} entries", plan.entries.len());
    Ok(())
}
```

When fallback succeeds, entries report `StreamSource::PgcProxy` and include resolver diagnostics.
Diagnostic endpoints are reduced to origins and diagnostic messages redact common secret patterns.

## Download Execution

Downloads are explicit. The library default keeps muxing disabled so embedding applications do not
spawn `ffmpeg` unless they opt in.

```rust,no_run
use bbdown::{BiliClient, ClientConfig, DownloadOptions, MuxOptions, RetryPolicy, StreamSelection};
use std::time::Duration;

#[tokio::main]
async fn main() -> bbdown::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::video(80))
        .with_retry_policy(RetryPolicy::new(3, Duration::from_millis(250)))
        .with_download_idle_timeout(Some(Duration::from_secs(30)))
        .with_subtitles(true)
        .with_danmaku(true)
        .with_mux(MuxOptions::ffmpeg("ffmpeg"));

    let report = client
        .download_input("BV1qt4y1X7TW", None, options)
        .await?;

    println!("wrote {} entries", report.entries.len());
    Ok(())
}
```

Use `bbdown plan` or `BiliClient::plan_download` first when a UI needs to present quality choices.
`StreamSelection::video`, `StreamSelection::audio`, and `StreamSelection::new` select exact DASH
stream ids from the plan.

## Download Archive And Duplicate Decisions

Embedding applications should keep duplicate handling explicit. Inspect a plan with
`DownloadPreflight`, show the existing archive records or output conflict to the user, then call the
executor with the chosen `DuplicateDecision`. The crate does not prompt.

```rust,no_run
use bbdown::{
    BiliClient, ClientConfig, DownloadArchive, DownloadOptions, DownloadPreflight,
    DuplicateDecision, MuxOptions,
};

#[tokio::main]
async fn main() -> bbdown::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client.plan_download("BV1qt4y1X7TW", None).await?;
    let options = DownloadOptions::new("downloads").with_mux(MuxOptions::Disabled);
    let archive_path = "downloads/archive.json";
    let mut archive = DownloadArchive::load(archive_path)?;
    let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive));

    if preflight.requires_decision() {
        println!(
            "{} possible duplicate records",
            preflight.archived_records.len()
        );
        if let Some(conflict) = &preflight.output_conflict {
            println!("output exists: {}", conflict.path.display());
        }
    }

    let decision = if preflight.requires_decision() {
        DuplicateDecision::KeepBoth
    } else {
        DuplicateDecision::Replace
    };
    let report = client
        .download_plan_with_archive_decision(&plan, options, &mut archive, decision)
        .await?;
    archive.save(archive_path)?;

    println!("wrote {}", report.output_dir.display());
    Ok(())
}
```

`DuplicateDecision::Replace` writes to the planned output root and disables resume when that root
already exists. `DuplicateDecision::KeepBoth` writes to the next suffixed output root and keeps prior
archive records. If a UI chooses to cancel, stop after preflight and do not call the download
executor. Archive records contain content identity, output paths, entry ids, sidecar paths, mux
output paths, and completion timestamps; they do not contain media URLs or credentials.

## Endpoint Overrides

Use endpoint builders for tests, local mocks, or controlled gateway deployments.

```rust,no_run
use bbdown::{ClientConfig, EndpointConfig};

let endpoints = EndpointConfig::default()
    .with_api_base("http://127.0.0.1:8080")
    .with_pgc_base("http://127.0.0.1:8080")
    .with_comment_base("http://127.0.0.1:8081");

let config = ClientConfig::default().with_endpoints(endpoints);
```

## Compatibility Guidance

- Build configuration values with `Default`, `new`, and `with_*` methods rather than struct
  literals.
- Read output models by field access or serde serialization; avoid constructing output structs in
  downstream code unless a test really needs a local fixture.
- Keep credentials and QR login scan URLs out of logs and crash reports.
- Treat restricted-area proxy hosts as trusted infrastructure because media URLs and access keys may
  pass through them.

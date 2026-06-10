[ English | [简体中文](embedding.zh-CN.md) ]

# Embedding Guide

## Scope

The `bbdown_core` crate, published as the `bbdown-core` package, is the integration surface for Rust
projects that need typed Bilibili metadata, download plans, media downloads, subtitle sidecars,
danmaku sidecars, QR login state, batch collection parsing, and restricted-area proxy diagnostics
without shelling out to the CLI.

The crate is still in the `0.1` compatibility phase. Prefer constructors and builder-style APIs for
configuration, and treat metadata and plan structs as read-only output surfaces. This keeps
embedding code resilient when new fields are added while the crate matures.

## Planning Only

Use `BiliClient::plan_download` for raw CLI-style inputs, or parse an `Input` yourself and call
`BiliClient::plan`.

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, Selection};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
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

## Batch And Collection Inputs

`BiliClient::resolve_input` accepts CLI-style raw inputs such as B23 short links, `fav...`,
`mid...`, `collection...`, `series...`, canonical favorite `/list/ml...` URLs, path-based
`/medialist/.../ml...` URLs, space collection URLs, and space series URLs. Batch inputs resolve to
`ResolvedContent::Collection`, which carries full collection metadata plus the selected items.
Owner-scoped space list URLs keep the uploader mid so the resolver can use newer space collection
and series APIs. Without a selector, collection-like inputs select all parsed items; pass
`Selection::Page(index)` for one item or `Selection::Latest` for the newest parsed item. Empty
collections are represented as empty item lists, not as missing-field errors.

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, ResolvedContent, Selection};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let resolved = client
        .resolve_input("fav456", Some(Selection::Page(1)))
        .await?;

    if let ResolvedContent::Collection(collection) = resolved {
        println!(
            "{} selected from {}",
            collection.selected_items.len(),
            collection.collection.title
        );
    }

    Ok(())
}
```

Download planning maps selected collection items back to normal video entries, so downstream
download execution, archive duplicate checks, stream selection, subtitles, and danmaku sidecars use
the same APIs as normal video downloads. Planning may fetch only the selected batch item set because
`DownloadPlan` does not expose collection metadata. PUGV/cheese episode inputs resolve as seasons,
follow paginated episode lists when the API reports additional pages, and plan through
`StreamSource::PugvWeb`.

## Credentials

Embedding projects can either use `CredentialStore` or inject credentials from their own storage.
Do not log raw credential values. `Credentials` debug output is redacted, but application logs should
still treat credentials as secrets.

```rust,no_run
use bbdown_core::{ClientConfig, Credentials};

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
use bbdown_core::{
    BiliClient, ClientConfig, RestrictedArea, RestrictedAreaConfig, RestrictedAreaProxy, Selection,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
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
use bbdown_core::{BiliClient, ClientConfig, DownloadOptions, MuxOptions, RetryPolicy, StreamSelection};
use std::time::Duration;

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
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
executor with the same preflight and chosen `DuplicateDecision`. The crate does not prompt. If the
application serializes a preflight between display and execution, store the full preflight object so
`KeepBoth` keeps avoiding archive-only output directories that were reserved during inspection. The
executor validates that the preflight still matches the current archive before applying the decision,
so callers should reinspect when another process may have updated the archive.

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadArchive, DownloadOptions, DownloadPreflight,
    DuplicateDecision, MuxOptions,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client.plan_download("BV1qt4y1X7TW", None).await?;
    let options = DownloadOptions::new("downloads").with_mux(MuxOptions::Disabled);
    let archive_path = "downloads/archive.json";
    let mut archive = DownloadArchive::load(archive_path)?;
    let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;

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
        DuplicateDecision::Cancel
    };
    let report = client
        .download_plan_with_archive_preflight_decision(
            &plan,
            options,
            &mut archive,
            &preflight,
            decision,
        )
        .await?;
    archive.save(archive_path)?;

    println!("wrote {}", report.output_dir.display());
    Ok(())
}
```

`DuplicateDecision::Replace` removes the existing planned output root before a fresh download when
that root already exists, then the completed record replaces any stale archive record that pointed at
the same output path. `DuplicateDecision::KeepBoth` writes to the next suffixed output root and keeps
prior archive records, including archive-only records whose old output directory has been removed.
If a UI chooses to cancel after a duplicate preflight, stop after preflight and do not call the
download executor. Passing `DuplicateDecision::Cancel` with a no-conflict preflight is a safe
continue path: if an output conflict appears before execution, the executor reports it instead of
implicitly replacing the new output root. Archive records contain content identity, absolute output
paths, entry ids, absolute sidecar paths, absolute mux output paths, and completion timestamps; they
do not contain media URLs or credentials. Entry identities use aid/cid media ids instead of optional
BVID or episode ids, so a PGC episode planned through an episode URL can match a later BV/av plan for
the same media even when one plan lacks a BVID.
`DownloadPreflight::inspect` also treats archive records with the same planned output path as
duplicates, even when the content identity differs and the old output directory is no longer on
disk. Store the archive at a JSON file path outside the chosen output root and any archive save
sidecar paths; `DownloadArchive::save` rejects directory targets. If the archive path is a symlink,
`DownloadArchive::save` updates the symlink target instead of replacing the link itself.

## Endpoint Overrides

Use endpoint builders for tests, local mocks, or controlled gateway deployments.

```rust,no_run
use bbdown_core::{ClientConfig, EndpointConfig};

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

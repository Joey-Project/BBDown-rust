[ English | [简体中文](embedding.zh-CN.md) ]

# Embedding Guide

## Scope

The `bbdown_core` crate, published as the `bbdown-core` package, is the integration surface for Rust
projects that need typed Bilibili metadata, download plans, media downloads, subtitle sidecars,
danmaku sidecars, QR login state, batch collection parsing, and restricted-area proxy diagnostics
without shelling out to the CLI.

The current crate version is `0.4.0`, a post-`0.3.0` development line focused on credential
lifecycle improvements, access-key acquisition, unified login QR output, and append-only danmaku
updates. Prefer constructors and builder-style APIs for configuration, and treat metadata and plan
structs as read-only output surfaces. This keeps
embedding code resilient when new fields are added while the crate matures.

## Planning Only

Use `BiliClient::plan_download` for raw CLI-style inputs, or parse an `Input` yourself and call
`BiliClient::plan`. When a UI or archive preflight is tied to a single-output mode, use
`BiliClient::plan_download_with_mode` or `BiliClient::plan_with_download_mode` so sidecar-only modes
do not require media stream resolution.

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

## Playback Request Specs

Use `BiliClient::plan_playback` when a player, cache server, or HTTP proxy needs selected media
request data instead of filesystem download execution. The returned `PlaybackPlan` is derived from
the same resolver path as `DownloadPlan`, so input parsing, selection, restricted-area fallback,
intl access, and selected stream source reporting remain aligned.

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, Selection};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let playback = client
        .plan_playback("BV1qt4y1X7TW", Some(Selection::Current))
        .await?;

    for entry in &playback.entries {
        for variant in &entry.variants {
            if let Some(video) = &variant.video {
                println!("{} {}", variant.id, video.url);
                println!("headers: {}", video.headers.len());
                println!("cache key: {}", video.cache_key.source_hash);
            }
        }
    }

    Ok(())
}
```

Each `PlaybackVariant` contains selected DASH video/audio request specs or FLV segment request specs.
`MediaRequestSpec` includes primary and backup URLs, HTTP headers, mime type, exact codec string
when the upstream surface provides one, codec-family metadata, bandwidth, dimensions, duration, size,
and a cache key. `PlaybackVariant.selection_hints` includes an `avplayer` profile with `playable`,
`preferred`, `score`, exact `video_codec` / `audio_codec` strings when known, codec-family fields, a
`format_key`, and machine-readable reason codes. Downstream clients can use
`PlaybackCodecPreference` to rank variants by their own H.264, HEVC, AV1, or other codec order, then
verify exact codec strings when present before handing a request to a platform player. The cache
key hashes the source URL without exposing it in plaintext while preserving query-string resource
identity. `PlaybackEntry.cache_key` identifies the selected content, `PlaybackVariant.cache_key`
groups the media cache keys that make up one playable variant, `PlaybackEntry.abr.groups` lists
codec/mime-compatible switching groups in low-to-high level order, and `PlaybackVariant.abr` points
to the variant's group and level. A cache server can store fetched media by `MediaCacheKey`, keep
completed variants by `PlaybackVariantCacheKey`, and retain lower or previously visited compatible
levels while ABR policy moves up or down. The crate does not implement playback task state, HLS playlist generation,
segment serving, retention, cleanup, AVPlayer event/VOD playlist switching, or library
registration. Downstream players and cache servers own those responsibilities and can use
`PlaybackPlan` as their stable HTTP request contract.

For BBDown-compatible TV HTTP playurl resolution, set
`ClientConfig::with_playurl_mode(PlayurlMode::Tv)`, configure `EndpointConfig::with_tv_api_base`
when a mock or proxy is needed, and provide `Credentials::tv_access_key` when the TV endpoint
requires account access. TV mode currently applies to normal videos and PGC episodes.
For BBDown-compatible APP gRPC playurl resolution, set
`ClientConfig::with_playurl_mode(PlayurlMode::App)`. Configure
`EndpointConfig::with_app_grpc_base` for normal-video mocks or proxies and
`EndpointConfig::with_app_pgc_grpc_base` for PGC mocks or proxies. The normal-video default uses
`https://grpc.biliapi.net`; the PGC default uses the same gRPC host. APP mode uses
`Credentials::tv_access_key` first and falls back to `Credentials::access_key`, emits
`StreamSource::NormalApp` or `StreamSource::PgcApp`, and normalizes protobuf DASH/FLV media into
the same `StreamSet` and `PlaybackPlan` surfaces as the HTTP modes. PGC APP gRPC restricted or
preview-only signals still enter the configured restricted-area HTTP playurl proxy fallback when
they are carried by region-limit messages, APP permission-denied gRPC status, or PGC response-body
metadata. Proxy fallback
URLs only receive the generic `Credentials::access_key`. Non-zero gRPC status is checked from
initial headers and trailing metadata. APP DASH resolution and frame-rate metadata is preserved on
`MediaStream` / `PlaybackPlan` output. APP numeric codec ids are exposed as `codec_family` metadata
without fabricating exact MP4 codec strings. Multiple APP legacy FLV segment
qualities are reduced to one highest-quality segment set because the current `StreamSet` schema
represents legacy FLV as a single ordered segment list.

## Batch And Collection Inputs

`BiliClient::resolve_input` accepts CLI-style raw inputs such as B23 short links, `fav...`,
`mid...`, `collection...`, `series...`, `recommendations`, `history`, `watch-later`, `following`, canonical
favorite `/list/ml...` URLs, path-based `/medialist/.../ml...` URLs, space collection URLs, space
series URLs, the Bilibili homepage, the authenticated `/account/history`, `/watchlater`, and
`/list/watchlater` pages, and dynamic feed pages. Batch inputs resolve to
`ResolvedContent::Collection`, which carries full collection metadata plus the selected items.
Owner-scoped space list URLs keep the uploader mid so the resolver can use newer space collection
and series APIs. Without a selector, collection-like inputs select all parsed items; pass
`Selection::Page(index)` for one item, `Selection::Indices(...)` for index lists and ranges, or
`Selection::Latest` for the first parsed item in the upstream list order. Empty collections are
represented as empty item lists, not as missing-field errors.

Recommendation input uses the web homepage recommendation endpoint. It accepts the
`recommendations`, `recommendation`, and `recommend` shorthands plus the Bilibili homepage URL. The
current implementation emits normal-video `av` cards; non-video recommendation cards are skipped,
and explicit index selection may walk additional `fresh_idx` refresh batches within a safety cap to
cover the filtered video cards.

History input uses the web history cursor endpoint and therefore requires a cookie on
`ClientConfig::credentials`. The current history collection emits normal-video `archive` records
that can be mapped back to the normal video planning path; other history business types such as
PGC, live, or article records are skipped until those item shapes have dedicated collection planning
support.

Watch-later input uses the web toview endpoint and also requires a cookie on
`ClientConfig::credentials`. It accepts `watchlater`, `watch-later`, `watch_later`, `later`,
`toview`, `https://www.bilibili.com/watchlater`, and `https://www.bilibili.com/list/watchlater`,
then emits normal videos from the authenticated account's watch-later list.

Following input uses the web dynamic feed endpoint and therefore also requires a cookie on
`ClientConfig::credentials`. It accepts the `following` shorthand plus the dynamic home URLs.
Space dynamic input accepts `https://space.bilibili.com/<mid>/dynamic`. Dynamic feed inputs
currently emit normal-video archive cards and skip non-video cards.

The current collection inputs keep their existing `ResolvedContent::Collection` JSON and Rust
surface. Internally they now use the shared feed/list selection layer, so embedders can use the same
index, range, latest, and empty-list semantics across favorites, space uploads, collections, series,
homepage recommendations, history, watch-later, following feeds, and space dynamic feeds.

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, IndexSelection, IndexSelector, ResolvedContent, Selection,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let selection = Selection::Indices(IndexSelection::new([
        IndexSelector::index(1),
        IndexSelector::range(3, 5),
    ])?);
    let resolved = client
        .resolve_input("fav456", Some(selection))
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

The same index selection type applies to normal video pages and season episode indexes. The CLI
parser accepts equivalent strings such as `1`, `page:1`, `1,3-5`, and `page:2-4,7`.
`Selection::Episode(epid)` remains an exact PGC episode-id selector.

Download planning maps selected collection items back to normal video entries, so downstream
download execution, archive duplicate checks, stream selection, cover, subtitles, and danmaku
sidecars use the same APIs as normal video downloads. Planning may fetch only the selected batch item
set because `DownloadPlan` does not expose collection metadata. PUGV/cheese episode inputs resolve
as seasons, follow paginated episode lists when the API reports additional pages, and plan through
`StreamSource::PugvWeb`.

## Credentials

Embedding projects can either use `CredentialStore` or inject credentials from their own storage.
Do not log raw credential values. `Credentials` debug output is redacted, but application logs should
still treat credentials as secrets.
For QR login, convert `QrLoginTicket` to `QrLoginTicketOutput` when a downstream application needs a
stable serialized scan URL and `qr_payload`; current WEB and TV login flows use the scan URL itself
as the QR payload.
Call `BiliClient::check_credential_health()` when an embedding project needs a redacted diagnostic
report before deciding whether to prompt for login, import a token, or continue with anonymous
requests. The report includes one probe each for the WEB cookie, generic `access_key`, and TV
`tv_access_key`; probe messages are sanitized before they are serialized.

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, Credentials};

let credentials = Credentials::default()
    .with_cookie("SESSDATA=...")
    .with_access_key("...");

let config = ClientConfig::default().with_credentials(credentials);
let client = BiliClient::new(config);
let health = client.check_credential_health().await;
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
use bbdown_core::{
    BiliClient, ClientConfig, DanmakuFormat, DownloadMode, DownloadOptions, DownloadPathTemplates,
    MuxOptions, RetryPolicy, StreamSelection,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::video(80))
        .with_download_mode(DownloadMode::All)
        .with_retry_policy(RetryPolicy::new(3, Duration::from_millis(250)))
        .with_download_idle_timeout(Some(Duration::from_secs(30)))
        .with_cover(true)
        .with_subtitles(true)
        .with_danmaku(true)
        .with_danmaku_format(DanmakuFormat::Ass)
        .with_path_templates(
            DownloadPathTemplates::new()
                .with_output_dir("{title}-{entry_count:02}")
                .with_entry_dir("{index:03}-{entry_title}-{content_id}")
                .with_mux_file_stem("{index:03}-{entry_title}"),
        )
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
Use `DownloadMode::VideoOnly`, `AudioOnly`, `SubtitleOnly`, `DanmakuOnly`, or `CoverOnly` when an
embedding caller wants one output kind. Sidecar-only modes do not require media streams and never
spawn muxing; video-only and audio-only modes select only the matching DASH stream. Use the
mode-aware planning APIs before `DownloadPreflight::inspect` when the later download options use a
non-default mode.
Danmaku sidecars default to `DanmakuFormat::Xml`; use `DanmakuFormat::Ass` for ASS-only output, or
`DownloadOptions::with_danmaku_formats([DanmakuFormat::Xml, DanmakuFormat::Ass])` when the
embedding UI needs to keep both XML and ASS sidecars.
`DownloadPathTemplates` customizes the output root, entry directory, and muxed file stem while
keeping the media and sidecar filenames stable. Template strings render one path component and are
sanitized after expansion. Output templates can use `{title}` and `{entry_count}`; entry and mux
templates can use `{title}`, `{entry_title}` or `{page_title}`, `{index}` or `{page}`, `{aid}`,
`{bvid}`, `{cid}`, `{epid}`, and `{content_id}`. Numeric placeholders accept zero padding such as
`{index:03}`. Entry templates must render a unique directory name for every selected entry; include
`{index}` or `{content_id}` when titles may repeat. If an embedding application shows archive
preflight results before downloading, build the preflight with the same `DownloadOptions` and
templates that will later be passed to execution.

The crate default preserves media URLs exactly as planned. Embedding applications that need
BBDown-like PCDN avoidance or a custom UPOS host can set `MediaHostOptions` on `DownloadOptions`.
The policy applies only to DASH and FLV media candidates; cover, subtitle, and danmaku sidecar URLs
are not rewritten.

```rust,no_run
use bbdown_core::{DownloadOptions, MediaHostOptions};

let options = DownloadOptions::new("downloads").with_media_hosts(
    MediaHostOptions::bbdown_cli_default()
        .with_upos_host("upos-sz-mirrorcoso1.bilivideo.com"),
);
```

## Download Archive And Duplicate Decisions

Embedding applications should keep duplicate handling explicit. Inspect a plan with
`DownloadPreflight`, show the existing archive records or output conflict to the user, then call the
executor with the same preflight and chosen `DuplicateDecision`. The crate does not prompt. If the
application serializes a preflight between display and execution, store the full preflight object so
`KeepBoth` keeps avoiding archive-only output directories that were reserved during inspection. The
executor validates that the preflight still matches the current archive before applying the decision,
so callers should reinspect when another process may have updated the archive.
Archive matching is output-aware for single-output modes and danmaku formats, so ASS-only or
multi-format danmaku downloads do not satisfy XML-only danmaku preflights.

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
Use `DownloadArchive::records_for_plan_with_mode` for mode-specific archive lookups, or
`records_for_plan` when a UI wants to show every archive record for the same content across all
download modes.
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
- Keep credentials, QR login scan URLs, QR payloads, and credential-health raw request details out
  of logs and crash reports.
- Treat restricted-area proxy hosts as trusted infrastructure because media URLs and access keys may
  pass through them.

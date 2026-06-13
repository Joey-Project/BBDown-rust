[ English | [简体中文](README.zh-CN.md) ]

# bbdown-core

`bbdown-core` is the reusable Rust library package behind BBDown Rust. Rust code imports it as
`bbdown_core`. It resolves Bilibili and Bilibili intl inputs into typed metadata, download plans,
media downloads, single-output download modes, cover/subtitle/XML-or-ASS danmaku sidecars, QR login
credentials, download archive preflight data, playback request specs, batch collection metadata,
and restricted-area proxy diagnostics. Download execution can also apply explicit UPOS host
replacement or BBDown-like PCDN avoidance through `MediaHostOptions`. Raw input parsing covers
normal videos, PGC and intl episodes, PUGV/cheese courses, B23 short links, favorite lists, space
videos, collections, and series.

Install with `cargo add bbdown-core`, then import with `bbdown_core`.

The current crate version is `0.2.0`, a post-`0.1.0` development line that adds batch collection
metadata through `ResolvedContent::Collection`. Embedding projects should prefer constructor and
builder-style APIs such as `ClientConfig::default().with_*()`, `EndpointConfig::default().with_*()`,
`RestrictedAreaConfig::default().with_*()`, `DownloadOptions::new(...).with_*()`,
`RetryPolicy::new(...)`, and `StreamSelection::new(...)` instead of struct literals for
configuration values that may grow while the crate matures. Public plan output containers are
consumed data surfaces and may gain fields.

## Example

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadMode, DownloadOptions, DownloadPathTemplates,
    MediaHostOptions, RetryPolicy, Selection, StreamSelection,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download_with_mode(
            "BV1qt4y1X7TW",
            Some(Selection::Current),
            DownloadMode::VideoOnly,
        )
        .await?;
    let options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::video(80))
        .with_download_mode(DownloadMode::VideoOnly)
        .with_path_templates(
            DownloadPathTemplates::new()
                .with_output_dir("{title}-{entry_count:02}")
                .with_entry_dir("{index:03}-{entry_title}-{content_id}"),
        )
        .with_media_hosts(
            MediaHostOptions::new().with_upos_host("upos-sz-mirrorcoso1.bilivideo.com"),
        )
        .with_retry_policy(RetryPolicy::new(3, Duration::from_millis(250)));

    println!("{} entries", plan.entries.len());
    println!("download output root: {}", options.output_dir.display());
    Ok(())
}
```

Batch inputs such as `fav456`, `mid123`, `collection456`, and `series456` resolve through
`ResolvedContent::Collection`; `resolve_input` keeps full parsed collection metadata while
`selected_items` carries the active subset. Selected items then plan and download through the normal
video pipeline. Use `Selection::Page(index)` for one item or `Selection::Indices(...)` with
`IndexSelection` / `IndexSelector` when an embedding application needs list and range selection such
as `1,3-5`.

The library default preserves planned media URLs. Set `MediaHostOptions` explicitly when an
embedding application wants a custom UPOS host, force-replace behavior, or CLI-compatible PCDN
fallback handling.
Use `BiliClient::plan_playback` when an embedding application needs serializable DASH video/audio
or FLV segment request specs for a downstream streaming/cache service. `PlaybackPlan` includes
primary and backup URLs, media headers, mime/codec metadata, duration, size, entry/variant/media
cache keys, and codec/mime-compatible ABR group/level metadata, but it does not implement player
state, HLS playlist generation, or HTTP segment serving. Each `PlaybackVariant` also includes
`selection_hints.avplayer` with exact codec strings, codec-family metadata, a `format_key`, and
ranking signals. Use `PlaybackCodecPreference` when an embedding client wants to prefer H.264,
HEVC, AV1, or another codec order.
Set `ClientConfig::with_playurl_mode(PlayurlMode::Tv)` and `EndpointConfig::with_tv_api_base` when
an embedding application needs BBDown-compatible TV HTTP playurl resolution for normal videos or
PGC episodes. TV mode uses `Credentials::tv_access_key` and does not reuse the generic intl access
key.
Set `DownloadPathTemplates` when an embedding application needs BBDown-style or application-specific
output names. Templates render sanitized filename components for the output root, entry directory,
and muxed file stem while media and sidecar filenames remain stable for resume and archive records.
Entry templates must render unique directory names across the selected entries.

See the repository embedding guide for restricted-area proxy, endpoint override, credential,
download archive, and download execution examples:
[English](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/embedding.md) /
[简体中文](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/embedding.zh-CN.md).

The CLI package is distributed through GitHub release archives. The crates.io dry-run target for this
workspace is the `bbdown-core` library package.

[ English | [简体中文](embedding.zh-CN.md) ]

# Embedding Guide

## Scope

The `bbdown_core` crate, published as the `bbdown-core` package, is the integration surface for Rust
projects that need typed Bilibili metadata, download plans, media downloads, subtitle sidecars,
danmaku sidecars, QR login state, batch collection parsing, and restricted-area proxy diagnostics
without shelling out to the CLI.

The current crate version is `0.5.0`, a post-`0.4.0` development line focused on downloader and
embedding polish: progress callbacks, cancellation-aware execution, chapter metadata muxing, audio
language selection, and AI subtitle filtering. Prefer constructors and builder-style APIs for configuration, and treat metadata and plan
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
        println!("chapters: {}", entry.chapters.len());
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
when the upstream surface provides one, codec-family metadata, optional audio `language` /
`language_doc` metadata, bandwidth, dimensions, duration, size, and a cache key.
`PlaybackVariant.selection_hints` includes an `avplayer` profile with `playable`,
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
`https://grpc.biliapi.net`; the PGC default uses the same gRPC host. APP mode uses Bilibili main/BALH
generic `Credentials::access_key` values before `Credentials::tv_access_key`; pass
`ClientConfig::with_access_key_provider(Some(AccessKeyProvider::BiliIntlOauth2))` when the generic
key came from intl OAuth so APP mode can prefer the TV key and only fall back to that generic key
when no TV key is available. Legacy credentials without provider metadata also keep the
TV-key-first APP behavior for compatibility. It emits `StreamSource::NormalApp` or
`StreamSource::PgcApp`, and
normalizes protobuf DASH/FLV media into
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
The default `CredentialStore::load()` and `CredentialStore::save()` methods preserve the legacy
single-profile JSON file shape. Use `CredentialProfiles` plus the `load_profiles`/`save_profiles`
and `load_profile`/`save_profile` helpers when an embedding project needs multiple accounts. A legacy
flat file loaded through the profile API appears as the `default` profile, and saving a named profile
migrates the store to the versioned profile document without dropping the default credentials.
`CredentialProfileSelection` and the selected-profile store helpers provide the same default-profile
versus named-profile routing used by the CLI, so embedders can bind user-selected accounts without
duplicating the migration behavior.
For incremental updates, prefer `CredentialStore::update_profile`,
`CredentialStore::update_selected_profile`, or `CredentialStore::update_profiles` over a separate
`load_profiles` then `save_profiles` round trip. These helpers take a cooperative per-store lock,
reload the latest on-disk profile document, apply only the requested mutation, and then write the
private file back, so unrelated profiles and provider refresh secrets are not overwritten by a stale
snapshot from another `bbdown` process. Stale lock files whose owner process can be confirmed gone,
plus older lock files without owner-pid metadata, are reclaimed after a short recovery window;
still-running or unverifiable owners are not reclaimed. Lock acquisition and stale reclaim are
serialized by the same companion guard. Writes check that their lock token is still current after the temporary file is written and
immediately before replacing the credential file, and lock release checks the same token before
deletion. The CLI's automatic access-key refresh path also validates
that the current selected profile name still matches the request profile, and that the profile still
matches the request's old access key, access-key provider metadata, refresh token, refresh provider,
and keypair before saving a refresh response; if not, it leaves the current store untouched.
Profile documents can also carry optional lifecycle metadata through `CredentialProfileMetadata` and
`CredentialLifecycleMetadata`. This metadata records provenance, checked-at/expiry timestamps, and
whether a refresh token was present without storing raw refresh-token values in the metadata map.
Legacy flat stores keep the old shape, empty metadata is omitted from serialized profile documents,
unknown or malformed optional metadata is ignored on load, and automatic refresh remains a separate
policy layer.
For QR login, convert `QrLoginTicket` to `QrLoginTicketOutput` when a downstream application needs a
stable serialized scan URL and `qr_payload`; current WEB and TV login flows use the scan URL itself
as the QR payload.
For generic access-key authorization, `AccessKeyLoginConfig::biliplus(callback_origin)` builds a
BiliPlus/BALH-compatible browser handoff URL whose `AccessKeyLoginTicketOutput::qr_payload` can be
rendered directly as a QR code. The parser accepts the historical `balh-login-credentials:` message
shape with either a JSON payload or URL/query callback, returning `AccessKeyLoginCredentials` with the
generic `access_key` plus optional refresh/expiration metadata. Calling `credentials()` converts only
the generic access key into the existing `Credentials` model, so embedding applications that own
storage should persist lifecycle metadata from `oauth_expires_at`, `expires_at`, `expires_in`, and
`refresh_token` explicitly. Refresh secrets should be kept separate from runtime `Credentials`:
use `CredentialProfileSecrets` with an `AccessKeyProviderSecret` under the provider that produced the
current access key. The CLI writes BiliPlus/BALH callbacks under the `balh_biliplus` provider and
marks their refresh provider as `bilibili_main_oauth2` with the `bili_tv` keypair family. The CLI
login path records lifecycle metadata in the selected credential profile: absolute expiry fields are
stored directly, relative `expires_in` values are converted from the acquisition time, and
refresh-token presence is stored without copying the token value into lifecycle metadata. In browser
`postMessage` flows, prefer
`AccessKeyLoginTicketOutput::credentials_from_message(event_origin, data)` so the sender origin is
validated against the ticket's trusted auth or callback origin before parsing. Use the raw
`AccessKeyLoginCredentials::from_balh_*` parsers only after an embedding application has already
validated message provenance.
For access-key lifecycle orchestration, evaluate
`AccessKeyRenewalDecision::from_profile_status(profile_status, force_reauthorization)` after loading
`CredentialProfiles::profile_lifecycle_status(...)`. A `NoAction` decision means the selected
profile's access-key metadata is still fresh under the caller's policy; `Reauthorize` means the UI
should either try provider-specific refresh or render a new `AccessKeyLoginTicketOutput` and collect
another BALH callback. The decision's `automatic_refresh_readiness` field is intentionally explicit:
`metadata_only_refresh_token` means a previous callback reported a refresh token before provider
secrets were stored; `ready` means the selected profile has a provider-scoped refresh secret,
refresh provider, and any provider keypair required for network refresh.
Build an `AccessKeyRefreshRequest` from the saved access key plus the matching
`AccessKeyProviderSecret`, then call `BiliClient::refresh_access_key(...)`. The client supports
Bilibili main OAuth2 refresh through `EndpointConfig::passport_base` and BiliIntl OAuth2 refresh
through `EndpointConfig::intl_passport_base`; `bili_tv` main-provider keypairs are routed to the TV
OAuth refresh path. It returns a fresh `AccessKeyLoginCredentials` value so callers can reuse the same
lifecycle/secret persistence path as initial access-key login. Treat
network or API refresh failures as non-destructive: keep the old credential and fall back to
reauthorization UI when policy requires user action.
Call `BiliClient::check_credential_health()` when an embedding project needs a redacted diagnostic
report before deciding whether to prompt for login, import a token, or continue with anonymous
requests. The report includes one probe each for the WEB cookie, generic `access_key`, and TV
`tv_access_key`; `kind` identifies the credential slot and `scope` identifies the checked consumer.
The generic `access_key` probe currently covers the intl/Bstar OAuth-info scope only, so downstream
applications that need APP gRPC or proxy-specific assurance should treat that as a separate policy
decision. Probe messages are sanitized before they are serialized. Use `CredentialHealthReport::summary()`
for a compact, JSON-friendly aggregate status and `CredentialHealthReport::probe(kind, scope)` when
UI or preflight policy needs one exact probe.
Profile documents can also be evaluated without network I/O through
`CredentialProfiles::profile_lifecycle_status(profile, policy)` or
`CredentialProfiles::lifecycle_statuses(policy)`. `CredentialLifecyclePolicy` requires an explicit
`now_unix_millis` value so embedders can make deterministic stale/expiring decisions in UI,
background jobs, or tests.
For plan/download preflight, build a `CredentialPreflightReport` from the selected profile lifecycle
status and the media request context. `CredentialPreflightReport::from_client_context(...)` is the
conservative client-config form, while `CredentialPreflightReport::from_media_request_context(...)`
lets embedders skip restricted-area proxy requirements for inputs that cannot use PGC proxy fallback.
Use `CredentialPreflightReport::from_media_paths_context(...)` when the resolved source has no
WEB/TV/APP playurl path, such as intl/Bstar inputs that should check only the intl generic
`access_key`. These forms mirror the credential the client would send: WEB playurl cookies are
optional, TV playurl requires `tv_access_key`, APP playurl accepts either generic `access_key` or
`tv_access_key` and uses provider metadata as the tie-breaker when both are stored: Bilibili
main/BALH generic keys are checked before TV keys, while `bili_intl_oauth2` keys and legacy profiles
with no provider metadata yield to TV keys. Stale optional WEB playurl cookies are warnings rather than blockers so public anonymous
requests can still proceed. Account-scoped feed inputs such as history, watch-later, and following
should add `CredentialPreflightRequirement::authenticated_web_api_cookie()` because they hit
authenticated WEB APIs before selecting media streams. Public space dynamic pages can run
anonymously and should not add that required-cookie preflight. Restricted-area proxy fallback
treats the generic `access_key` as optional: present keys are checked and may be forwarded by the
resolver, but missing keys do not block proxy URLs that authenticate themselves or allow anonymous fallback.
Intl/Bstar episode media and subtitle paths require the generic `access_key` used by official intl
metadata, playurl, and subtitle requests. Cover-only and danmaku-only intl episode paths should skip
that access-key requirement because they only need metadata and sidecar endpoints; metadata requests
can still include an access key when one is present.
Fixed-source inputs such as intl/Bstar and PUGV/cheese should not inherit a caller's global TV/APP
playurl credential requirements, and sidecar-only modes should skip media-stream preflight.
The report is a pure value: it lists requirement statuses, warnings/blockers, and the selected
profile's `AccessKeyRenewalDecision`, but it never mutates credential storage. When embedders accept
short links, normalize them with
`BiliClient::parse_input(...)` before deciding whether PGC proxy fallback or intl access-key
preflight may run.
Embedding projects can treat blockers as fail-fast UI, warnings as non-blocking banners, or call
`should_attempt_access_key_renewal()` before using `BiliClient::refresh_access_key(...)` and saving
the refreshed credentials through their own storage layer. The renewal predicate requires missing
non-access-key credentials to be fixed first, but present non-access-key credentials with stale,
expiring, expired, or unknown lifecycle metadata do not block a ready generic access-key refresh.
Whitespace-only stored credential strings are treated as missing when lifecycle status and redacted
presence booleans are computed. Request builders trim stored credential strings before use and
omit them when the trimmed value is empty.

```rust,no_run
use bbdown_core::{
    AccessKeyLoginConfig, AccessKeyLoginCredentials, AccessKeyLoginTicketOutput, BiliClient,
    ClientConfig, CredentialKind, CredentialLifecyclePolicy, CredentialLifecycleStatus,
    CredentialPreflightMode, CredentialPreflightReport, CredentialProfileSelection,
    CredentialStore, Credentials, PlayurlMode, RestrictedAreaConfig,
};

async fn check_credentials() {
    let credentials = Credentials::default()
        .with_cookie("SESSDATA=...")
        .with_access_key("...");

    let config = ClientConfig::default().with_credentials(credentials);
    let client = BiliClient::new(config);
    let health = client.check_credential_health().await;
    let _summary = health.summary();
}

fn load_named_profile(store: &CredentialStore, profile: &str) -> bbdown_core::Result<Credentials> {
    let selection = CredentialProfileSelection::named(profile)?;
    store.load_selected_profile(&selection)
}

fn access_key_lifecycle_status(
    store: &CredentialStore,
    profile: &str,
    now_unix_millis: u64,
) -> bbdown_core::Result<Option<CredentialLifecycleStatus>> {
    let profiles = store.load_profiles()?;
    let policy = CredentialLifecyclePolicy::at_unix_millis(now_unix_millis);
    let status = profiles.profile_lifecycle_status(profile, &policy)?;
    Ok(status
        .credential_statuses
        .into_iter()
        .find(|status| status.kind == CredentialKind::AccessKey)
        .map(|status| status.status))
}

fn plan_preflight_report(
    store: &CredentialStore,
    profile: &str,
    now_unix_millis: u64,
) -> bbdown_core::Result<CredentialPreflightReport> {
    let profiles = store.load_profiles()?;
    let policy = CredentialLifecyclePolicy::at_unix_millis(now_unix_millis);
    let status = profiles.profile_lifecycle_status(profile, &policy)?;
    Ok(CredentialPreflightReport::from_client_context(
        CredentialPreflightMode::Warn,
        &status,
        PlayurlMode::Web,
        &RestrictedAreaConfig::default(),
    ))
}

fn access_key_login_ticket() -> bbdown_core::Result<AccessKeyLoginTicketOutput> {
    let config = AccessKeyLoginConfig::biliplus("https://www.bilibili.com")?;
    Ok(config.ticket()?.output())
}

fn access_key_from_balh_message(
    ticket: &AccessKeyLoginTicketOutput,
    event_origin: &str,
    message: &str,
) -> bbdown_core::Result<Credentials> {
    Ok(ticket
        .credentials_from_message(event_origin, message)?
        .credentials())
}

fn access_key_from_trusted_payload(message: &str) -> bbdown_core::Result<Credentials> {
    Ok(AccessKeyLoginCredentials::from_balh_message(message)?.credentials())
}
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
Plan entries may include `ChapterTrack` values when the selected upstream player metadata exposes
usable chapter boundaries. When muxing through `MuxOptions::ffmpeg(...)`, those chapters are handed
to ffmpeg as temporary ffmetadata and the returned `MuxReport::chapter_count` records how many
chapters were included. With muxing disabled, chapters remain metadata on the plan entry and no
chapter sidecar is written.

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DanmakuFormat, DownloadMode, DownloadOptions, DownloadPathTemplates,
    MuxOptions, RetryPolicy, StreamSelection, SubtitleAiPolicy,
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
        .with_subtitle_ai_policy(SubtitleAiPolicy::PreferNonAi)
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

Use the `*_with_progress` download methods when an embedding application needs progress callbacks
without parsing CLI output. `DownloadProgressEvent` is emitted for plan start/completion, entry
start/completion/failure, file start/chunk/completion/failure, mux start/completion/failure, and
plan completion/failure/cancellation. The callback is synchronous and should stay lightweight; send
events into an application channel if UI updates or database writes may block the download task.

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadOptions, DownloadProgressEvent, DownloadProgressSink,
};

struct ProgressLogger;

impl DownloadProgressSink for ProgressLogger {
    fn on_download_progress(&self, event: &DownloadProgressEvent) {
        eprintln!("{event:?}");
    }
}

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let progress = ProgressLogger;
    let report = client
        .download_input_with_progress(
            "BV1qt4y1X7TW",
            None,
            DownloadOptions::new("downloads"),
            &progress,
        )
        .await?;

    let summary = report.summary();
    println!(
        "wrote {} files across {} entries ({} bytes newly written)",
        summary.file_count, summary.entry_count, summary.bytes_written
    );
    Ok(())
}
```

Archive-aware flows have matching `download_plan_with_archive_decision_with_progress` and
`download_plan_with_archive_preflight_decision_with_progress` methods. The existing non-progress
methods remain available and use a no-op sink. Treat `plan_completed`, `plan_failed`, and
`plan_cancelled` as mutually exclusive terminal states for a download task. `plan_cancelled` is
emitted for explicit archive duplicate cancellation and for downloads cancelled through a
`DownloadCancellationToken`. Token-triggered cancellation returns `Error::Cancelled`; use
`Error::is_cancelled()` when UI code needs to separate a user stop from ordinary failures. Explicit
archive duplicate cancellation is a preflight decision path, so callers should use the duplicate
decision/report state instead of treating every `plan_cancelled` event as `Error::Cancelled`.

```rust,no_run
use bbdown_core::DownloadProgressEvent;

fn apply_progress(event: &DownloadProgressEvent) {
    match event {
        DownloadProgressEvent::FileProgress {
            path,
            bytes_written,
            expected_size,
            ..
        } => {
            eprintln!("{path:?}: {bytes_written}/{expected_size:?}");
        }
        DownloadProgressEvent::PlanCompleted { .. } => eprintln!("download completed"),
        DownloadProgressEvent::PlanFailed { error, .. } => eprintln!("download failed: {error}"),
        DownloadProgressEvent::PlanCancelled { error, .. } => {
            eprintln!("download cancelled: {error}");
        }
        _ => {}
    }
}
```

Use the `*_with_cancellation` or `*_with_progress_and_cancellation` download variants when a UI,
task supervisor, or cache server needs to stop work explicitly. A token can be cancelled from any
task with `cancel()` or `cancel_with_reason(...)`. Cancellation is checked during planning, before
new entries and sidecars, while waiting for retry backoff, while sending HTTP requests, while
streaming response bodies, and while waiting for `ffmpeg` muxing. Newly created partial files are
removed on cancellation; resumed files are truncated back to the pre-attempt size; already completed
entries remain valid and are counted in the terminal event.

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadCancellationToken, DownloadOptions, DownloadProgressEvent,
    DownloadProgressSink,
};

struct UiProgress;

impl DownloadProgressSink for UiProgress {
    fn on_download_progress(&self, event: &DownloadProgressEvent) {
        // Forward the event into the application's UI or task-state channel.
        eprintln!("{event:?}");
    }
}

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let progress = UiProgress;
    let cancellation = DownloadCancellationToken::new();

    let cancel_from_ui = cancellation.clone();
    tokio::spawn(async move {
        // Replace this with the application's cancel button, task shutdown, or HTTP disconnect.
        wait_for_user_cancel().await;
        cancel_from_ui.cancel_with_reason("user cancelled download");
    });

    let result = client
        .download_input_with_progress_and_cancellation(
            "BV1qt4y1X7TW",
            None,
            DownloadOptions::new("downloads"),
            &progress,
            &cancellation,
        )
        .await;

    if let Err(error) = &result {
        if error.is_cancelled() {
            eprintln!("download stopped by caller: {error}");
        }
    }

    result.map(|_| ())
}

async fn wait_for_user_cancel() {}
```

The same configuration surface can be combined when a downstream task needs stable UI state,
explicit cancellation, preferred audio, and subtitle filtering:

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadCancellationToken, DownloadOptions, DownloadProgressEvent,
    DownloadProgressSink, StreamSelection, SubtitleAiPolicy,
};

struct TaskProgress;

impl DownloadProgressSink for TaskProgress {
    fn on_download_progress(&self, event: &DownloadProgressEvent) {
        match event {
            DownloadProgressEvent::PlanCompleted { .. }
            | DownloadProgressEvent::PlanFailed { .. }
            | DownloadProgressEvent::PlanCancelled { .. } => {
                eprintln!("terminal task state: {event:?}");
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let cancellation = DownloadCancellationToken::new();
    let options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::audio_language("Japanese"))
        .with_subtitles(true)
        .with_subtitle_ai_policy(SubtitleAiPolicy::PreferNonAi);

    let report = client
        .download_input_with_progress_and_cancellation(
            "BV1qt4y1X7TW",
            None,
            options,
            &TaskProgress,
            &cancellation,
        )
        .await?;

    let summary = report.summary();
    eprintln!("{} files, {} bytes", summary.file_count, summary.total_bytes);
    Ok(())
}
```

Use `bbdown plan` or `BiliClient::plan_download` first when a UI needs to present quality choices.
`StreamSelection::video`, `StreamSelection::audio`, and `StreamSelection::new` select exact DASH
stream ids from the plan. `StreamSelection::audio_language("ja-JP")` or
`StreamSelection::new(None, Some(30280)).with_audio_language("Japanese")` selects the first DASH
audio stream whose `MediaStream.language` or `language_doc` matches the requested value
case-insensitively. Explicit stream selection is included in archive content keys, so different
chosen qualities or audio languages do not satisfy one another's duplicate preflight.
Use `DownloadMode::VideoOnly`, `AudioOnly`, `SubtitleOnly`, `DanmakuOnly`, or `CoverOnly` when an
embedding caller wants one output kind. Sidecar-only modes do not require media streams and never
spawn muxing; video-only and audio-only modes select only the matching DASH stream. Use the
mode-aware planning APIs before `DownloadPreflight::inspect` when the later download options use a
non-default mode.
`DownloadPlan` preserves raw subtitle AI metadata through `SubtitleTrack::is_ai_generated`,
`ai_type`, and `ai_status`. Use `DownloadOptions::with_subtitle_ai_policy(...)` to include all
subtitle tracks, prefer non-AI tracks for the same language, exclude AI tracks, or download only AI
tracks. Non-default subtitle AI policies participate in archive keys because they change the
sidecar set.
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

Append-only danmaku refresh is a separate archive-driven operation for already downloaded entries.
Plan the same input and selection with `DownloadMode::DanmakuOnly`, load the archive, then call
`BiliClient::update_danmaku_for_archive`. Danmaku-only planning keeps the refresh independent from
media playurl availability. The method matches archive entries by aid/cid, downloads the current XML
danmaku payload, append-merges new comments into `danmaku.xml`, regenerates selected derived formats
such as ASS, updates the archive entry sidecar list, and returns a typed `DanmakuUpdateReport` with
per-entry existing, fetched, and appended comment counts. XML is always the canonical update target;
`DanmakuUpdateOptions::with_danmaku_formats([DanmakuFormat::Ass])` adds or refreshes `danmaku.ass`
from the merged XML.

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DanmakuFormat, DanmakuUpdateOptions, DownloadArchive, DownloadMode,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download_with_mode("BV1qt4y1X7TW", None, DownloadMode::DanmakuOnly)
        .await?;
    let archive_path = "downloads/archive.json";
    let mut archive = DownloadArchive::load(archive_path)?;
    let report = client
        .update_danmaku_for_archive(
            &plan,
            &mut archive,
            DanmakuUpdateOptions::default().with_danmaku_formats([DanmakuFormat::Ass]),
        )
        .await?;
    archive.save(archive_path)?;

    println!("updated {} entries", report.entries.len());
    Ok(())
}
```

Callers that manage sidecar storage themselves can use `merge_xml_append_only(existing, fetched)`
to apply the same XML-level append-only merge without touching `DownloadArchive`.

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

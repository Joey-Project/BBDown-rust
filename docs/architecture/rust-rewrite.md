[ English | [简体中文](rust-rewrite.zh-CN.md) ]

# Rust Rewrite Architecture

## Goals

- Build a Rust crate that other projects can embed without shelling out to a CLI.
- Keep the CLI as the user-facing tool and e2e test surface.
- Preserve BBDown's practical Bilibili knowledge while replacing CLI-log parsing with typed data.
- Support normal videos, `ep`, `ss`, `md`, intl episodes, PUGV/cheese inputs, batch collection and
  feed/list inputs, B23 short links, and configured restricted-area resolvers.

## Workspace

- `crates/bbdown`: library package published as `bbdown-core` and imported as `bbdown_core`, with
  typed input parsing, metadata models, credential store, client config, and resolver APIs.
- `crates/bbdown-cli`: CLI wrapper that uses the crate only through public APIs.
- `docs/`: architecture, user-facing notes, project state, and project journal entries.

## Public API Shape

The reusable crate keeps configuration ergonomic for embedders through `Default`, `new`, and
builder-style `with_*` methods. `EndpointConfig`, `ClientConfig`, `RestrictedAreaConfig`,
`Credentials`, `DownloadOptions`, `RetryPolicy`, `StreamSelection`, and `MuxOptions` all have
constructor paths so downstream projects do not need struct literals for ordinary integration code.
The CLI uses the same public builders, which makes it an in-repo integration test surface for the
crate API.

Output models remain typed data surfaces. The current crate version is `0.5.0`, a post-`0.4.0`
development line focused on downloader and embedding polish: progress callbacks,
cancellation-aware execution, chapter metadata muxing, audio language selection, and AI subtitle
filtering; callers should read fields or serialize output
values rather than treating output structs as stable construction targets.

## Resolver Model

Inputs normalize into `Input`:

- `Aid` and `Bvid` for normal videos.
- `Episode`, `Season`, and `Media` for Bilibili PGC URLs and ids.
- `CheeseEpisode` and `CheeseSeason` for PUGV/cheese courses.
- `IntlEpisode` for `bilibili.tv` episode URLs.
- `SpaceVideos`, `FavoriteList`, `CollectionList`, `SeriesList`, `SpaceCollectionList`, and
  `SpaceSeriesList` for batch content. Owner-scoped space collection/series variants keep the
  uploader mid from canonical URLs so newer space APIs can be called directly.
- `RecommendationFeed` for homepage recommendation batches from the `recommendations` shorthand or
  the Bilibili homepage URL.
- `History` for authenticated watch-history input from the `history` shorthand or the
  `/account/history` page.
- `WatchLater` for authenticated watch-later input from the watch-later shorthands, `/watchlater`,
  or `/list/watchlater` pages.
- `FollowingFeed` and `SpaceDynamic` for authenticated dynamic video feeds from followed uploaders
  or one uploader's dynamic page.
- `ShortLink` for B23 links, resolved through HTTP redirect before normal input dispatch.

The library resolves metadata into `ResolvedContent`:

- `VideoMetadata` includes title, description, owner, tags, cover, pub time, and pages.
- `SeasonResolution` includes season metadata plus the selected episode set.
- `VideoCollectionResolution` includes collection metadata plus the selected item set for
  favorites, space uploads, collections, series, homepage recommendations, watch history,
  watch-later lists, following feeds, and space dynamic feeds. Favorite parsing accepts shorthand ids, path-based medialist
  pages, and canonical `/list/ml...` pages. `resolve_input` keeps full parsed collection metadata
  even when a selector narrows `selected_items`.

Feed/list behavior that is shared across collection-like page families lives behind an internal
`feed_list` resolver layer. It owns selection validation, page/range fetch-mode calculation,
identity-based deduplication, and one-based item renumbering. The existing public collection output
shape stays unchanged; history, recommendation, watch-later, and dynamic feed page families now add
page-specific fetchers on top of this layer.

The library resolves media availability into `DownloadPlan`:

- `DownloadEntry` records the selected `aid`, `bvid`, `cid`, optional `epid`, title, and source.
  Batch collection items are mapped back to normal video entries before stream planning.
- `StreamSet` keeps DASH video/audio tracks, FLV segments, raw accepted quality ids, structured
  selectable DASH quality labels, and duration.
- `StreamDiagnostics` records non-default resolver attempts such as restricted-area proxy fallback.
- `SubtitleTrack` records language metadata, normalized URL, basic format classification, and
  upstream AI subtitle metadata when present.
- `ChapterTrack` records title plus start/end seconds when the upstream player metadata exposes
  usable chapter boundaries.
- `DanmakuTrack` records the XML comment endpoint derived from `cid` and the configured comment
  endpoint base.

`ss`, `md`, and `cheese/ss` require a `Selection` in non-interactive contexts. Batch collection and
feed/list inputs default to all parsed items; callers can pass `Selection::Page(...)` for one item,
`Selection::Indices(IndexSelection)` for list/range selection over item indexes, or
`Selection::Latest` for the first parsed item in the upstream list order. The same index-selection
surface applies to normal video pages and season episode indexes, while `Selection::Episode(...)`
continues to mean an exact PGC episode id. Empty batch collections resolve as empty selected item
lists for the default/all selection. `plan_download` may fetch only enough batch items to cover the
selected maximum index because `DownloadPlan` does not expose collection metadata. Recommendation
input uses the web homepage recommendation endpoint and currently emits normal-video `av` cards,
walking additional `fresh_idx` refresh batches within a safety cap when explicit index selection
needs more cards after non-video cards are skipped.
History input uses the web history cursor endpoint, requires an authenticated cookie, and currently
filters to normal-video `archive` records that can plan through the normal video pipeline.
Watch-later input uses the web toview endpoint, requires an authenticated cookie, and emits normal
videos from the authenticated account's watch-later list.
Following and space dynamic inputs use the web dynamic feed endpoints, also require an
authenticated cookie, and currently emit normal-video archive cards. The
CLI will later add interactive prompting, but the library keeps season-like contracts explicit so
integrations cannot accidentally download a full season.

Mode-aware planning uses the same resolver dispatch but skips media stream resolution for
sidecar-only modes. Callers that need a plan for archive preflight or UI decisions with a
non-default `DownloadMode` use `plan_download_with_mode` or `plan_with_download_mode` so the plan
shape matches the later download options.

## Stream Planning

`BiliClient::plan` is the public crate API for building a typed download plan from a parsed
`Input` without performing file I/O. `BiliClient::plan_download` remains a raw-string convenience
wrapper for CLI-style callers. Planning currently supports these official source modes:

- `NormalWeb` uses the normal web playurl endpoint for `aid`/`bvid` inputs.
- `NormalTv` uses the BBDown-compatible TV HTTP playurl endpoint for `aid`/`bvid` inputs when
  `ClientConfig.playurl_mode` is `PlayurlMode::Tv`.
- `NormalApp` uses the BBDown-compatible APP gRPC playurl endpoint for `aid`/`bvid` inputs when
  `ClientConfig.playurl_mode` is `PlayurlMode::App`.
- `PgcWeb` uses the PGC web playurl endpoint for `ep`, `ss`, and `md` inputs.
- `PgcTv` uses the BBDown-compatible TV HTTP playurl endpoint for PGC inputs when
  `ClientConfig.playurl_mode` is `PlayurlMode::Tv`.
- `PgcApp` uses the BBDown-compatible APP PGC gRPC playurl endpoint for PGC inputs when
  `ClientConfig.playurl_mode` is `PlayurlMode::App`; restricted or preview-only signals can still
  fall back to configured restricted-area HTTP playurl proxies.
- `PugvWeb` uses the PUGV/cheese playurl endpoint for `cheese/ep` and selected `cheese/ss` inputs.
  PUGV metadata follows `episode_page` pagination through the episode-list endpoint before applying
  season selection.
- `IntlWeb` uses the intl OGV playurl endpoint with BiliIntl mobile signing parameters for
  `bilibili.tv` episode inputs and includes the caller-provided access key when configured.

Subtitle discovery follows the selected source. Normal and PGC entries use the player subtitle
endpoint. Intl entries use the intl subtitle endpoint. Subtitle failures are treated as missing
optional tracks, matching BBDown's practical behavior, while stream resolution failures remain
hard errors.

Intl season metadata can return `code: 0` with a region-limit payload and no episode list. The
resolver preserves that as an access-restricted error instead of reporting a generic selection
failure.

The CLI exposes this layer through `bbdown plan`. The command is intentionally a planning surface:
it prints typed JSON or a short human summary, but it does not download, merge, or mutate output
files.

## Playback Integration Contract

`BiliClient::plan_playback` and `BiliClient::plan_playback_input` build a `PlaybackPlan` from the
same resolver path used by `DownloadPlan`. This surface is intended for downstream players, cache
servers, and API layers that need media requests without invoking the file downloader. A playback
entry preserves the selected aid/bvid/cid/epid, title, source, quality labels, duration, and a set
of playback variants. DASH variants carry selected video and audio `MediaRequestSpec` values; FLV
variants carry ordered segment specs.

`MediaRequestSpec` is deliberately serializable and transport-neutral. It contains the primary URL,
backup URLs, media headers, mime type, exact codec string when known, codec-family metadata,
optional audio language metadata, bandwidth, dimensions, duration, size, and a structured cache key.
The cache key is based on content identity, media kind, stream id, exact codec string when present,
and a hash of the source URL with fragments removed but query identity preserved. This avoids
exposing the URL in plaintext while preventing collisions for proxy URLs whose query string
identifies the resource. Playback planning
also exposes `PlaybackEntry.cache_key`, `PlaybackVariant.cache_key`,
`PlaybackEntry.abr.groups`, and `PlaybackVariant.abr` so a downstream cache server can store media
by request key, retain completed variants by variant key, and map ABR level changes back to the same
codec/mime-compatible switching group without refetching already cached levels.
`PlaybackVariant.selection_hints.avplayer` adds an AVPlayer-oriented profile with exact codec
strings when known, codec families, a `format_key`, score/preferred signals, and machine-readable
reasons. The public `PlaybackCodecPreference` helper lets downstream clients rank variants with
their own H.264, HEVC, AV1, or other codec order instead of accepting a hard-coded H.264-first
policy. APP/gRPC streams expose numeric codec ids as family metadata without fabricating exact MP4
codec strings.
The same planning path honors `PlayurlMode::Tv` and `PlayurlMode::App`, so `DownloadPlan` and
`PlaybackPlan` can expose `NormalTv`, `PgcTv`, `NormalApp`, or `PgcApp` sources without changing the
downstream request-spec shape. TV mode uses `Credentials::tv_access_key`. APP/gRPC mode uses
`Credentials::tv_access_key` first, falls back to `Credentials::access_key`, sends BBDown-compatible
protobuf gRPC frames, reads gRPC status from both initial headers and trailing metadata, and
normalizes APP DASH/FLV replies into `StreamSet`. APP DASH width, height, and frame-rate metadata
is preserved on the same `MediaStream` fields used by HTTP playurl modes. APP legacy FLV replies
can contain segment sets for multiple qualities; because `StreamSet::flv_segments` is a single
ordered list, the normalizer keeps one highest-quality FLV candidate instead of concatenating
incompatible qualities.

This repository does not implement player runtime responsibilities. A downstream cache/player
service owns task state such as `preparing`, `playback_ready`, `downloading`, `completed`, and
`failed`; HLS session directories, playlists, segment files, retention, cleanup, and finalization;
HTTP serving such as `/tasks/{id}/hls/master.m3u8`; AVPlayer-compatible event playlists during
download and VOD playlists after completion; and registering completed HLS or remuxed MP4 artifacts
as library items. Future crate work may add richer device policy profiles without moving those
runtime responsibilities into `bbdown-core`.

## Download Execution

`BiliClient::download_plan` executes a caller-provided `DownloadPlan`. `BiliClient::download` and
`BiliClient::download_input` are convenience wrappers that plan first, then execute. The executor
returns a typed `DownloadReport` instead of scraping CLI output.
Progress is an opt-in execution-side observer. The `*_with_progress` variants accept a
`DownloadProgressSink` and emit typed `DownloadProgressEvent` values for plan, entry, file, and mux
milestones. The non-progress methods are preserved and route through a no-op sink. The CLI exposes
the same event stream only when `--progress-json` is set, writing JSON Lines to stderr so stdout
remains either human output or the final `DownloadReport` JSON.
Cancellation is also opt-in for embedding callers. The `*_with_cancellation` and
`*_with_progress_and_cancellation` variants accept a `DownloadCancellationToken` that can be
cancelled from another task. The executor checks the token across planning, entry boundaries,
sidecar generation, retry sleeps, HTTP request/response streaming, and muxing. Cancellation returns
`Error::Cancelled` and emits the same plan-level `PlanCancelled` terminal progress event used by
archive duplicate cancellation. Completed entries remain intact; newly created partial files are
removed, and resumed files are truncated back to the pre-attempt size. The CLI installs the same
token for download-time `Ctrl-C`, so terminal users and embedders share the cancellation semantics.
The interactive archive duplicate prompt is the CLI exception: because terminal `stdin` input cannot
be rolled back like an executor task, `Ctrl-C` at that prompt exits immediately with status 130.

Execution behavior is controlled by `DownloadOptions`:

- output directory;
- path templates for the output root, entry directory, and mux file stem;
- bounded retry policy;
- optional DASH video/audio stream id selection;
- all-output or single-output download mode;
- HTTP range resume on or off;
- media read idle timeout;
- cover, subtitle, and danmaku sidecar inclusion;
- danmaku sidecar format set (`xml`, `ass`, or `xml,ass`);
- media host replacement and PCDN handling policy;
- disabled muxing or explicit `ffmpeg` binary path.

For each entry, execution prefers a complete DASH video/audio pair from the plan. By default this is
the first video and first audio stream; callers can set `StreamSelection::new(...)` to request exact
DASH video or audio stream ids, and can attach `StreamSelection::with_audio_language(...)` to select
the first audio stream whose `MediaStream.language` or `language_doc` matches. If a requested id or
language is unavailable, the executor reports the available ids or languages and fails before media
writes. If DASH media is incomplete and FLV `durl` segments are available, it downloads the FLV
segments instead; explicit stream selection requires DASH media and therefore rejects FLV fallback.
Otherwise the entry fails before media writes. Cover, subtitle, and danmaku files remain sidecars.
When muxing is enabled, the executor invokes `ffmpeg` with explicit argv and returns the command plus
output path in the report. If the plan entry carries chapters, ffmpeg muxing
adds a temporary ffmetadata input, maps chapters from that input, and reports the included count as
`MuxReport::chapter_count`.
Plan entries keep the canonical danmaku XML endpoint; the executor converts that XML to ASS only
when the selected `DanmakuFormats` set contains `DanmakuFormat::Ass`. ASS generation supports
common scrolling, top, bottom, and reverse-scrolling comments and skips advanced positioned comments
rather than writing misleading coordinates.

Append-only danmaku refresh is intentionally modeled as a separate archive-backed execution path
rather than as an implicit download side effect. `BiliClient::update_danmaku_for_archive` takes a
fresh `DownloadPlan`, a mutable `DownloadArchive`, and `DanmakuUpdateOptions`; it matches existing
archive entries by stable aid/cid identity, downloads the current XML payload, merges only new
comment blocks into canonical `danmaku.xml`, and regenerates selected derived formats such as ASS
from the merged XML. The lower-level `merge_xml_append_only` helper is public for callers that own
sidecar storage outside `DownloadArchive`.

Output naming is driven by `DownloadPathTemplates`. The output-root template is rendered from plan
context, while entry-directory and mux-file-stem templates are rendered from entry context. Rendered
values are sanitized as single filename components, so templates cannot inject nested paths. Media,
cover, subtitle, and danmaku sidecar filenames intentionally remain metadata-derived and stable for
resume behavior, duplicate track names, and archive path records. Duplicate preflight compares the
rendered planned output directory for the exact `DownloadOptions` that will be used during
execution. The executor rejects entry templates that render the same directory for multiple entries
in one plan, because shared entry directories would make resume and sidecar outputs ambiguous.

`DownloadMode` keeps the default all-output path separate from single-output workflows. `VideoOnly`
and `AudioOnly` download one matching DASH stream and skip sidecars and muxing. `SubtitleOnly`,
`DanmakuOnly`, and `CoverOnly` skip media requirements, write only the requested sidecar family, and
reject stream-quality selection because no media stream is selected.
Archive content keys are also mode-aware for single-output downloads while preserving the legacy key
for full downloads, so existing archives still match full downloads and single-output records do not
claim that a complete entry is already downloaded. Explicit stream selection adds stream tokens to
the archive key, preventing different selected qualities or audio languages from satisfying one
another's duplicate preflight. Non-default subtitle AI policies also add archive-key tokens because
they change which subtitle sidecars are downloaded.

Media and sidecar downloads use media headers without account cookies, because media URLs come from
API payloads and can target CDN or proxy hosts. DASH and FLV backup URLs are part of the candidate
list. `DownloadPlan` preserves upstream media URLs; `MediaHostOptions` is applied only when the
executor builds the concrete DASH/FLV candidate list. A configured `upos_host` rewrites all media
candidates, `force_replace_host` rewrites to the built-in BBDown fallback host, and the CLI default
rewrites only PCDN-like non-local candidates unless `--allow-pcdn` is set. Sidecar URLs are not
rewritten. Media body reads use a separate idle timeout instead of the metadata request timeout.
Resume appends only when `Content-Range` starts at the local file length and completes at the
advertised range total or an expected media size proves the final length; matching 416 responses are
treated as already complete. Wildcard `Content-Range` totals are rejected when no expected size is
available. When a stream or FLV segment declares a size, the executor rejects mismatched final file
lengths and rolls back failed writes to the pre-attempt length. Media responses that complete without
writing bytes are rejected. Entry directories include content identity so same-title videos do not share
resume targets, subtitle sidecar names include track identity, and filename components are bounded
by UTF-8 byte length. If a server ignores `Range` and returns `200 OK` for a partial file, the
executor writes the full retry to a temporary file and only replaces the old partial after available
validation succeeds. Without an advertised size, `Content-Length`, or `Content-Range`, a full retry
is rejected and the old file is preserved. Forced fresh writes also use
temporary files when replacing an existing target, so failed `--no-resume` retries do not clear
previous output. DASH media output names prefer stable stream metadata and only fall back to URL
path hashing when metadata is absent, so CDN host or query changes do not split resume targets.

Duplicate handling is modeled before execution instead of hidden inside the downloader.
`DownloadArchive` stores completed output records by content identity without media URLs or
credentials, and records output, sidecar, and mux paths as absolute paths at completion time.
`DownloadPreflight::inspect` reports content/archive hits, same-output archive records, and planned
output directory conflicts, so embedding applications can show what already exists and choose a
`DuplicateDecision`. `Replace` removes the existing planned output root before a fresh download,
then replaces stale archive records for that output path. `KeepBoth` writes to the next suffixed
output root while avoiding all archive record output paths, and comparisons use normalized output
path keys instead of raw `PathBuf` equality. These keys resolve existing symlink prefixes before
folding parent components, matching filesystem path resolution for archive records and CLI overlap
guards. `DownloadPreflight` serializes its reserved output paths so embedding applications can
round-trip preflight state before executing a `KeepBoth` decision without losing archive-only output
reservations, and execution validates that the preflight still matches the current archive before
applying a decision. Entry-level archive identities use stable aid/cid content ids instead of display
indexes, optional BVIDs, or optional episode ids, so reordered pages and episode-vs-BV URL forms can
still be detected as duplicates. `DownloadArchive::records_for_plan` returns matching content
records across all download modes, while `records_for_plan_with_mode` narrows the lookup to one
`DownloadMode`.
`Cancel` is a caller-level stop decision. The CLI exposes the same model with `--archive-file` and
`--on-duplicate`, rejects an archive file path that overlaps the chosen output root by checking both
lexical paths and canonical targets, and applies the same guard to archive save sidecar paths.
JSON/non-TTY mode requires an explicit decision instead of prompting. After showing preflight state,
the CLI executes against the same preflight so a no-conflict default cannot be upgraded into an
implicit replace if an output root appears between preflight and execution; it also rechecks the
archive-file guard against the actual output directory before saving. `DownloadArchive::save` rejects
directory targets before writing the archive file, and when the archive path is a symlink it writes
through to the symlink target so shared archive files keep one history.
Output-root occupancy checks use symlink metadata so stale or broken symlink roots are handled
consistently with replacement cleanup, while metadata errors such as inaccessible parents are
reported to callers instead of being retried as suffixed output roots forever.

The crate default keeps muxing disabled so embedding projects do not spawn external processes by
surprise. The CLI `download` command enables ffmpeg by default and exposes `--no-mux` for users and
mock e2e tests. Mux subprocess stdin, stdout, and stderr are isolated from CLI stdio. Muxing writes
to a temporary output first, validates that output, and then replaces the final file, so a failed
rerun preserves an existing muxed file and JSON reports remain parseable and accurate. Temporary
chapter ffmetadata is removed after successful, failed, or cancelled mux attempts.

## Restricted Area And Intl

The project must not hard-code public proxy services. Restricted-area support is designed as a
configured resolver chain:

- official web and PGC APIs;
- intl API using caller-provided access key when available;
- user-configured BBDown/BiliPlus-style playurl proxy hosts;
- user-configured proxies that mirror `api.bilibili.com` paths;
- user-configured area hints such as `cn`, `hk`, `tw`, or `th`.

`ClientConfig::restricted_area` holds a per-client `RestrictedAreaConfig`. Embedders can set an
optional area hint and a list of `RestrictedAreaProxy` candidates through
`RestrictedAreaConfig::new`, `RestrictedAreaConfig::default().with_area_hint(...)`,
`with_proxy(...)`, or `with_proxies(...)`. Candidate ordering follows the bilibili-helper approach
without browser-local caches: matching area hint first, generic candidates, then `cn`, `th`, `hk`,
and `tw`, with duplicate `(base_url, area, kind)` candidates removed. CLI-created configs also
preserve source priority before area grouping, so explicit command-line proxy candidates are tried
before environment-derived proxy candidates.

PGC stream planning first calls the selected official PGC playurl endpoint, either web HTTP or APP
gRPC depending on `PlayurlMode`. If that response clearly reports a region/area restriction and
restricted-area proxies are configured, the client tries ordered candidates until one returns a
valid DASH or FLV stream shape. For APP gRPC, fallback signals can come from region/area messages,
permission-denied gRPC status, or PGC response-body metadata such as `view_info.dialog`, stream
limits, or preview-only business state.
Non-area official failures keep their original error and do not contact proxy hosts. A
BBDown/BiliPlus-style HTTP(S) playurl proxy
receives the PGC playurl query at the configured URL. A Bilibili API HTTP(S) proxy receives the same
query at `/pgc/player/web/playurl` below the configured base URL, matching common BALH-style API
proxy hosts, and then tries `/pgc/player/web/v2/playurl` as a compatibility fallback for existing
API proxy deployments. Both paths preserve any query parameters already present on the configured
base URL. Proxy playurl responses may use the official `data` / `result` wrapper or older helper
shapes where `dash` / `durl`, `timelength`, and quality metadata are returned at the top level.
Legacy string status fields such as `result: "suee"` are tolerated for these top-level helper
payloads.
When a generic access key is present in
`Credentials::access_key`, proxy requests include it as `access_key`; the TV-specific access key is
not reused for this flow. Bilibili cookies are intentionally omitted from restricted-area proxy
requests.

When proxy fallback succeeds, `DownloadEntry.source` is `PgcProxy` and `DownloadEntry.diagnostics`
contains the official failed attempt plus the successful proxy attempt. When all candidates fail,
the returned access-restricted error summarizes the ordered attempts. Diagnostic endpoint fields
are reduced to URL origins so path/query/userinfo secrets are not printed; diagnostic error messages
also redact URL tokens and common sensitive key-value patterns before they are exposed through JSON or
final errors.

The current implementation supports endpoint override, B23 redirect resolution, PUGV/cheese
metadata and stream planning, batch favorite/space/collection/series/recommendation/history/dynamic-feed
metadata planning, intl metadata shape, official PGC stream planning, official intl OGV signed stream
planning, configured PGC proxy fallback, top-level helper playurl response parsing, typed source
reporting, resolver diagnostics, and download execution. Browser-only mobile response rewriting
remains intentionally out of scope.

## Credentials

The CLI stores credentials in a local JSON file under the platform config directory with `0600`
permissions on Unix. The crate exposes `Credentials` and `CredentialStore` so other projects can
inject their own storage or keep credentials in memory.
`CredentialStore::load()` and `CredentialStore::save()` continue to read and write the legacy flat
JSON credential shape for the default profile, so existing users and test fixtures do not need a
migration step. Profile-aware callers can use `CredentialProfiles`, `load_profiles`,
`save_profiles`, `load_profile`, `save_profile`, and `remove_profile` to store multiple named
credential sets in a versioned profile document. Loading a legacy flat file through the profile API
wraps it as the `default` profile, and saving a named profile migrates the file to the profile
document while preserving default credentials. Raw credential values stay redacted in debug output
for both `Credentials` and `CredentialProfiles`.
`CredentialProfileSelection` is the shared selection layer for CLI and embedding callers: the default
selection preserves legacy `load`/`save` behavior, while named selections route through the profile
document APIs and preserve other profiles during writes. The CLI exposes this through the global
`--credential-profile` flag and `BBDOWN_CREDENTIAL_PROFILE`; `auth logout` clears the whole store for
legacy default selection and removes only the named profile when one is explicitly selected.
Profile documents can include optional lifecycle metadata keyed by profile and credential kind. The
metadata records provenance, acquisition/check/expiry timestamps, and a boolean
`refresh_token_present` hint without duplicating raw token values in the metadata map. Empty metadata
is dropped during normalization, orphan metadata is removed when its profile has no credentials, and
unknown or malformed optional metadata is ignored so legacy flat credential files and valid profile
documents still load without lifecycle metadata.
`CredentialLifecyclePolicy` turns that persisted metadata into deterministic stale/expiring/expired
status output without network I/O. The policy requires an explicit `now_unix_millis` value and lets
embedders choose stale and expiring windows, so UI preflight, background jobs, and tests can make the
same lifecycle decision without reading wall-clock time inside the model.
The CLI applies that policy in `auth status --profiles`, where the no-flag `auth status` output
remains the legacy selected-profile redacted credential summary. Profile status output adds
default/selected markers, per-credential lifecycle status, and non-secret guidance; `--all-profiles`
expands it from the selected profile to every saved profile.
Credential health diagnostics are a read-only layer over the same credential model. The crate exposes
`CredentialHealthReport` and `BiliClient::check_credential_health()` so embedding callers can check
the WEB cookie, generic `access_key`, and TV `tv_access_key` independently before choosing a login or
fallback flow. Each probe records `kind` for the credential storage slot and `scope` for the checked
consumer. The WEB cookie probe uses `/x/web-interface/nav`; the token probes use
`/x/passport-login/oauth2/info` with the credential sent as a signed `access_key` app query value and
without sending cookies. Generic token probes currently check the intl/Bstar scope through the
configured `passport_base`; this does not claim APP gRPC or restricted-area proxy validity for the
same stored `access_key`. TV token probes use the configured `tv_passport_poll_base`. Probe failures
are contained per credential as `missing`, `valid`, `rejected`, or `request_failed` rather than
failing the whole report.
`CredentialHealthReport::summary()` gives downstream UI a compact aggregate status while preserving
the per-kind probes for exact policy decisions.
The CLI keeps single-profile `auth health --json` compatible with the raw report schema. Human
`auth health` output augments the probes with lifecycle or health guidance when a credential should
be re-checked, renewed, or re-acquired. `auth health --all-profiles --json` wraps each profile's
redacted lifecycle status, network health report, aggregate health summary, and guidance for profile
management UIs.

Generic access-key acquisition is modeled as a BiliPlus/BALH-compatible browser handoff rather than
an official Bilibili poller. `AccessKeyLoginConfig` builds the authorization URL with
`balh_auth=1` and a normalized callback origin; `AccessKeyLoginTicketOutput` exposes the URL, QR
payload, expected message origin, and callback origin for embedding UIs. The parser accepts the
historical `balh-login-credentials:` message prefix with either JSON credentials or a URL/query
callback using `access_key` or `access_token`. `AccessKeyLoginCredentials` preserves optional
`refresh_token`, absolute `oauth_expires_at`, and relative `expires_in` metadata, but conversion back
to `Credentials` stores only the generic `access_key`. Embedding callers that own storage should
copy expiration and refresh-token presence into `CredentialLifecycleMetadata` explicitly, and store
raw refresh secrets in `CredentialProfileSecrets` rather than runtime `Credentials`. Profile secrets
are provider-scoped as `profile_secrets.<profile>.access_key.<provider>` so BALH/BiliPlus,
Bilibili main OAuth2, and BiliIntl OAuth2 refresh behavior can evolve independently. The CLI login
path stores BALH/BiliPlus refresh secrets in plaintext in the same private credential file, marks the
refresh provider as `bilibili_main_oauth2`, and records the `bili_tv` keypair family observed for the
current BiliPlus handoff flow. Lifecycle metadata still records only source, provider, timestamps,
and refresh-token presence, not raw secret values. `AccessKeyRenewalDecision` turns a selected
profile lifecycle status into either `NoAction` or `Reauthorize`, and reports
`automatic_refresh_readiness` separately: `metadata_only_refresh_token` means only an old presence bit
exists, while `ready` means the selected provider has a stored refresh secret, refresh provider, and
any keypair required by that refresh provider. The provider-specific network refresh layer is exposed
as `AccessKeyRefreshRequest` plus `BiliClient::refresh_access_key(...)`. Bilibili main OAuth2 refresh
uses the configured `passport_base` and signed app keypairs, with the `bili_tv` keypair routed to the
TV OAuth refresh path; BiliIntl OAuth2 refresh uses the configured `intl_passport_base` and its intl
refresh form. Refresh returns
`AccessKeyLoginCredentials`, letting CLI and embedders reuse the same lifecycle metadata and
provider-secret persistence path used by initial access-key acquisition. Failed refresh attempts are
non-destructive; callers keep the old credential and can fall back to a reauthorization ticket.
Media credential preflight is modeled as an explicit policy layer rather than hidden behavior inside
`BiliClient::plan_download`. `CredentialPreflightReport` evaluates the selected profile lifecycle
status against request-path requirements and returns serializable requirement statuses, issues, and
the associated access-key renewal decision. The CLI applies that report before `plan`, `playback`,
and `download`: `warn` writes diagnostics to stderr, `fail` blocks before stream resolution, and
`renew` attempts provider-specific generic access-key refresh only when the report says the selected
profile is ready. This keeps embedders in control of storage mutation while still sharing the same
requirement model as the CLI.
Browser `postMessage` consumers should parse through the ticket/output `credentials_from_message`
helpers, which validate the sender origin against the trusted auth or callback origin before using
the raw BALH payload parser.
The CLI wraps this core API in `auth login-access-key`: it prints the same authorization URL and QR
payload, accepts pasted message or callback data through `--stdin` or `--file`, then merges the
resulting generic `access_key` and safe lifecycle metadata into the currently selected credential
profile. It deliberately avoids
interactive secret paste prompts because terminal echo can leak callback tokens into scrollback.
`--stdin` requires piped or redirected input for the same reason, `--file` rejects terminal-backed
paths, and the command rejects implicit stdin so callers must opt in before pipe or redirect input is
consumed.
`auth renew-access-key` uses the same parser and save path, but starts with the renewal decision. If
reauthorization is needed, it emits a fresh ticket; if callback input is supplied, it saves the new
access key and refreshed lifecycle metadata. Without callback input it stops after the decision and
ticket, letting an embedding shell or wrapper render the QR payload and collect the handoff.
Automation can use newline-delimited JSON ticket/saved events without receiving token values in stdout.

QR login is modeled as an explicit state machine in the crate. WEB QR login creates a
`QrLoginTicket`, which can be converted to `QrLoginTicketOutput` for a stable serialized scan URL and
QR payload surface, polls waiting-for-scan, waiting-for-confirmation, expired, and succeeded states,
then returns a cookie credential. TV QR login uses the BBDown-compatible app signed form flow and
returns a TV-specific access-key credential. This stays separate from the generic intl/Bstar
`access_key` because Bilibili app tokens are appkey-bound. WEB QR success prefers response
`Set-Cookie` headers and falls back to BBDown-compatible cookie extraction from the cross-domain
success URL. TV auth-code creation and TV polling are separately configurable so tests and controlled
proxies can mirror either the upstream split-host flow or a single local endpoint. TV tickets retain
the generated device session context so polling reuses the same device identity. QR login requests
use anonymous headers even when the client has stored credentials. The CLI `auth login-web` and
`auth login-tv` commands update the local credential store after a succeeded state by reloading the
current store before merging returned credentials, so a long QR wait does not overwrite another
command's credential update with a stale pre-wait snapshot.

Secrets are never included in status output; `auth status` and QR login `saved` JSON output report
only booleans. The QR login `ticket` event and human scan output intentionally expose the scan URL and
QR payload so the user can authenticate, and callers should treat those values as temporary login
secrets. The public QR state enum intentionally does not derive serde traits because the succeeded
state carries full credentials for embedding callers that handle storage themselves. QR ticket and QR
ticket-output debug output is redacted because ticket keys and scan URL query strings can act as
pre-authentication secrets. Credential health reports never include raw credential values, and API
messages are passed through the same diagnostic sanitizer used for restricted-area diagnostics before
they are serialized.
HTTP request errors are converted without retaining full URLs so query secrets such as intl
`access_key` do not appear in user-facing errors.

## Testing And CI

Default CI is deterministic:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- declared MSRV check with `cargo +1.95.0 check --workspace --locked`
- unit and workspace integration tests with `cargo test --workspace --locked`
- local CLI mock e2e tests with `cargo test -p bbdown-cli --test cli_e2e --locked`
- crates.io dry-run packaging for the publishable `bbdown-core` library package

Release packaging is a separate GitHub Actions workflow stack. `Release Artifacts` is reusable and
manual-only: it builds Linux x86_64, macOS x86_64, macOS aarch64, and Windows x86_64 CLI archives
without publishing tags, GitHub Releases, or crates. `Release Verification` is also reusable: both
RC creation and RC promotion call it to run formatter, clippy, declared MSRV, tests, and crates.io
dry-run validation for the selected commit. `Create Release Candidate` validates either the
repository default branch or a `release/*` source branch, builds those archives, and creates an
annotated `vX.Y.Z-rc.N` tag through the release GitHub App at the workflow ref commit, but first
rejects versions that already have a final tag or GitHub Release. It repeats that final tag and
GitHub Release check immediately before writing the RC tag.
`Promote Release Candidate` must be run from the latest RC tag for the requested version; it reruns
validation, rebuilds final archives, rechecks that the selected RC is still latest immediately before
publication, creates the final annotated `vX.Y.Z` tag, publishes the GitHub Release, then publishes
`bbdown-core` to crates.io through the protected `crates-io` environment. It also checks any existing
crates.io `bbdown-core` version is non-yanked and has the expected package checksum before creating
the final tag or GitHub Release, and repeats that check in the publish job before treating an
existing crate version as recovered success. RC
creation and promotion share a version-scoped concurrency group, so a later RC cannot be created
while the same version is being promoted. Archives contain the
`bbdown` binary, English and Simplified Chinese README files, English and Simplified Chinese user,
embedding, release, and architecture guides, and `LICENSE`. Each archive also has an adjacent
platform-specific checksum file. GitHub Release notes are generated from the previous non-RC final
release tag when one exists, so the RC tag is not used as the comparison base for the final release.
Promotion also supports retry after GitHub Release creation is interrupted: draft releases are
deleted and recreated, while published releases are reused only if the expected asset set is already
complete. Reuse requires the exact expected asset name set, `uploaded` states, non-empty sizes, and
downloaded archives that are named by and verify against their published `.sha256` sidecars. This
also requires the rebuilt `dist` archives to verify against their own sidecars and have the same
archive checksums as the published assets. Release archives normalize entry ordering, timestamps,
owners, groups, and archive container metadata so the same compiled inputs produce stable package
checksums. The workflow lists releases by `tag_name` instead
of relying only on the published-release tag endpoint, so draft releases are visible to the release
GitHub App token. The crates.io publish step checks the exact `bbdown-core` version first, then
repackages the selected RC source and requires the local `.crate` SHA256 to match the crates.io
checksum before treating an already-published version as recovered success. This covers runner
failures after the upload was accepted without allowing a different package for the same version to
pass recovery. Release workflows use the GitHub-hosted runner `rustup` and the floating stable
Rust channel from `rust-toolchain.toml`; third-party Rust toolchain and cache actions are
intentionally avoided. They also install Rust 1.95.0 for a
`cargo check` gate matching the crate `rust-version` metadata. Package names normalize release refs
to the packager-safe `[A-Za-z0-9._-]` character set, so tags such as SemVer build metadata do not
fail at packaging time. Shared release shell helpers live in `scripts/release/` so tag/release API
queries and Cargo version extraction can be linted outside YAML.

Crate publishing is intentionally scoped to the reusable `bbdown-core` library package, imported as
`bbdown_core` in Rust code. The crate has crates.io metadata, a package-local README and LICENSE,
dirty-tree-friendly local publish dry-run validation, and CI-backed
`cargo publish --dry-run -p bbdown-core --locked` validation. `bbdown-cli` remains `publish = false`
because CLI distribution is handled by GitHub release archives.

Plan output now exposes structured stream quality data. The library keeps raw
`StreamSet::accept_quality` for compatibility and adds `StreamSet::qualities` with actual selectable
DASH video ids plus optional labels derived from `accept_description` and `support_formats`. The CLI
human summary prints the same ids alongside video/audio stream summaries, while JSON callers can
select exact DASH streams through `DownloadOptions::stream_selection`.

The reusable crate is now on the `0.5.0` development line after the published `0.4.0` release, so
public configuration structs are intentionally hardened through constructor and builder APIs rather
than preserving local struct-literal experiments. Embedders should create configuration with those APIs, including
`ClientConfig::default().with_*`, `EndpointConfig::default().with_*`,
`RestrictedAreaConfig::default().with_*`, `DownloadOptions::new(...).with_*`,
`RetryPolicy::new`, `StreamSelection::new`, `StreamSelection::video`, and
`StreamSelection::audio`. Public output containers such as `StreamSet` and `StreamQuality` are
marked non-exhaustive because plan models are consumed data surfaces and may gain fields while the
crate matures.

Download archive and duplicate handling are covered at both crate and CLI levels. Unit tests cover
preflight archive/output conflict detection, entry-level archive overlap detection, replace
removing stale output-root artifacts before fresh writes, keep-both suffixed output roots, and
archive JSON round trips/replacement without media URLs. They also cover archive-only keep-both path
reservation, unrelated archive-only output path reservation, same-output archive record replacement,
display-index-insensitive entry archive identity, broken-symlink output roots, metadata error
reporting, preflight JSON round-trip reservation preservation, stale archive/preflight rejection,
episode-vs-video entry identity, symlink archive target saves, and directory-target archive save
rejection. CLI mock e2e tests cover
JSON duplicate failure without an
explicit decision, `cancel` preflight output, `keep-both` suffixed output roots, `replace`
overwriting an existing file, symlink archive target updates, and rejecting an archive file path that
overlaps the chosen output root lexically or through canonicalized targets, including archive save
sidecar paths.
Append-only danmaku update coverage includes XML merge unit tests, archive-backed core update tests
that regenerate ASS, and CLI mock e2e coverage that verifies JSON reports plus archive sidecar path
updates.


Live tests against Bilibili are opt-in only through `just live-e2e`. The recipe fails fast unless an
ignored `live-e2e.samples.json` manifest exists, so branch CI is not blocked by network, account, or
regional state. The tracked `live-e2e.samples.example.json` documents the manifest shape. Each live
case can run `info`, `plan`, or both against normal, PGC, intl, or restricted PGC inputs; can set a
case-specific selection and area hint; and can assert the expected JSON kind, allowed or required
plan sources, minimum entry count, and stream presence. Restricted PGC cases can explicitly allow an
access-restricted plan failure with required diagnostic fragments. The manifest parser rejects
unknown fields so expectation typos cannot silently disable source or diagnostic assertions. The
harness writes a temporary credential store per case from configured credential and access-key files,
removes CLI override environment variables, and expands all-area restricted proxy shortcuts into the
fixed `cn`, `th`, `hk`, and `tw` ordering. Network requests have a configurable timeout through
`ClientConfig` and CLI/settings so misbehaving official or proxy endpoints do not hang indefinitely.

## Planned PR Slices

1. Workspace, CI, docs, metadata resolver, credential store, and CLI `info/auth`. Completed in
   PR #1.
2. Stream resolver chain, download planning, subtitle and danmaku discovery. Completed in PR #2.
3. File download, retry/resume policy, ffmpeg mux integration, and mock e2e downloads. Completed
   in PR #3.
4. QR login state machine and live-test opt-in harness. Completed in PR #4.
5. Restricted-area proxy resolver ordering and diagnostics. Completed in PR #5.
6. Manifest-driven local live e2e sample matrix. Completed in PR #7.
7. GitHub binary release packaging. Completed in PR #8.
8. Crate publish readiness and dry-run validation. Completed in PR #9.
9. Clearer stream quality selection and listing support. Completed in PR #10.
10. Restricted-area proxy response compatibility expansion. Completed in PR #11.
11. Integration API and documentation hardening. Completed in PR #12.
12. Download archive and duplicate decision handling. Completed in PR #13.
13. More input parsing and batch collection parsing for short links, PUGV/cheese, favorites, space
    uploads, collections, and series. Completed in this slice.
14. Shared feed/list resolver abstraction for collection-like page families. Completed in this
    slice.

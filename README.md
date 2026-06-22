[ English | [简体中文](README.zh-CN.md) ]

# BBDown Rust

`BBDown Rust` is a Rust-native rewrite of BBDown with two goals:

- expose a reusable `bbdown-core` package / `bbdown_core` crate for other Rust projects;
- provide a CLI that can serve as the e2e surface for metadata resolution and downloads.

This project uses the original [BBDown](https://github.com/nilaoda/BBDown) project as a practical
Bilibili behavior reference. Thanks to BBDown and its contributors for that reference.

The current implementation establishes the crate/CLI/CI foundation, metadata resolver, stream
planning, media downloads, cover/subtitle/danmaku sidecar downloads, retry/resume behavior,
optional ffmpeg muxing, QR login, opt-in live test harnesses, configured restricted-area proxy
ordering with diagnostics, playback request specs for downstream streaming/cache integrations,
UPOS/PCDN media host controls, and builder-style crate integration APIs. It also supports an
explicit download archive for duplicate preflight, CLI replace / keep-both / cancel decisions, and
append-only danmaku sidecar updates for already-downloaded archive entries.
Input parsing covers normal videos, PGC and intl episodes, PUGV/cheese courses, B23 short links,
favorite lists, space videos, collections, series, homepage recommendations, watch history,
watch-later lists, following video feeds, and space dynamic video feeds. URL parsing includes canonical
`bilibili.com/list/...` pages, path-based medialist favorite URLs, space collection/series URLs that
carry the uploader mid needed by newer space APIs, the Bilibili homepage, space dynamic pages, plus
the authenticated `bilibili.com/account/history` watch-history page plus `bilibili.com/watchlater`
and `bilibili.com/list/watchlater` watch-later pages.

## Current CLI

Resolve metadata as JSON:

```bash
bbdown info av170001 --json
bbdown info BV1qt4y1X7TW --json
bbdown info ep267851 --json
bbdown info ss26801 --select latest --json
bbdown info md22718131 --select latest --json
bbdown info https://b23.tv/example --json
bbdown info cheese/ep101 --json
bbdown info fav456 --json
bbdown info recommendations --select latest --json
bbdown info history --select latest --json
bbdown info watch-later --select latest --json
bbdown info following --select latest --json
bbdown info https://www.bilibili.com/list/ml1103407912 --json
bbdown info https://space.bilibili.com/123/dynamic --select latest --json
bbdown info 'https://space.bilibili.com/123/lists/456?type=series' --json
```

Build a download plan as JSON:

```bash
bbdown plan av170001 --json
bbdown plan ep267851 --json
bbdown plan ss26801 --select latest --json
bbdown plan https://www.bilibili.tv/en/play/34613/341736 --json
bbdown plan fav456 --select page:1 --json
bbdown plan recommendations --select latest --json
bbdown plan history --select latest --json
bbdown plan watch-later --select latest --json
bbdown plan following --select latest --json
bbdown plan cheese/ss202 --select latest --json
```

`plan` resolves the selected entries, available DASH or FLV stream URLs, subtitle URLs, and the
danmaku XML URL for each `cid`. PGC and intl planning may still require eligible account or region
access. PGC playurl resolution can fall back to user-configured restricted-area proxies. It does
not download files. Collection-like inputs default to all items; use `--select page:<index>` to plan
one collection item or `--select latest` for the first parsed item in the upstream list order. `info --json` keeps full
parsed collection metadata under `collection.collection.items`; `plan` emits only selected entries.

Build a playback request spec as JSON:

```bash
bbdown playback av170001 --json
bbdown --playurl-mode tv playback av170001 --json
bbdown --playurl-mode app playback av170001 --json
bbdown playback ss26801 --select latest --json
bbdown playback fav456 --select 1,3-5 --json
```

`playback` resolves the same selected entries as `plan`, then emits selected DASH video/audio
request specs or FLV segment specs with primary URLs, backup URLs, headers, mime/codec metadata,
duration/size, entry/variant/media cache keys, ABR switching groups, and
`selection_hints.avplayer` metadata with exact codec strings when known, codec families, a
`format_key`, and AVPlayer-oriented ranking signals. Downstream clients can use
`PlaybackCodecPreference` to prefer
H.264, HEVC, AV1, or another codec order, then use `PlaybackVariant.abr` and `PlaybackEntry.abr`
to keep already cached variants available while switching codec/mime-compatible levels. It does not
download files, create HLS playlists, or run a player.
Set `--playurl-mode tv` or `BBDOWN_PLAYURL_MODE=tv` to resolve normal videos and PGC episodes
through BBDown-compatible TV HTTP playurl endpoints. TV mode uses the TV-specific access key saved
by `auth login-tv` and `--tv-api-base` / `BBDOWN_TV_API_BASE` for endpoint overrides.
Set `--playurl-mode app` or `BBDOWN_PLAYURL_MODE=app` to use BBDown-compatible APP gRPC playurl
endpoints for normal videos and PGC episodes. APP mode uses `Credentials::tv_access_key` first and
falls back to the generic `Credentials::access_key`; use `--app-grpc-base` /
`BBDOWN_APP_GRPC_BASE` and `--app-pgc-grpc-base` / `BBDOWN_APP_PGC_GRPC_BASE` for mock or proxy
endpoint overrides; the normal-video APP default uses `https://grpc.biliapi.net` and the PGC APP
default follows the BBDown reference host `https://app.bilibili.com`. PGC APP gRPC restricted or
preview-only signals still fall back to configured restricted-area HTTP playurl proxies when reported
by region-limit messages, APP permission-denied gRPC status, or PGC response-body metadata. Proxy fallback
URLs use only the generic imported `Credentials::access_key`, never the TV-specific token. Non-zero
gRPC status is read from both initial headers and trailing metadata. APP DASH response metadata such
as resolution and frame rate is preserved in playback/API output. If an APP response returns
multiple legacy FLV segment qualities, the normalized `StreamSet`
exposes one highest-quality segment set instead of mixing segments from different qualities.

Download selected media files:

```bash
bbdown download av170001 --output-dir downloads
bbdown download ss26801 --select latest --output-dir downloads
bbdown download fav456 --select page:1 --output-dir downloads
bbdown download av170001 --output-dir downloads --no-mux --json
bbdown download av170001 --audio-language ja-JP --output-dir downloads
bbdown download av170001 --only cover --output-dir downloads --json
bbdown download av170001 --output-dir downloads --archive-file downloads/archive.json --on-duplicate keep-both
bbdown download av170001 --output-template "{title}-{entry_count:02}" --entry-template "{index:02}-{entry_title}" --no-mux
bbdown danmaku update av170001 --archive-file downloads/archive.json --danmaku-format xml,ass
bbdown download av170001 --upos-host upos-sz-mirrorcoso1.bilivideo.com --no-mux
bbdown download av170001 --output-dir downloads --no-mux --json --progress-json
```

`download` resolves a plan, downloads the first complete DASH video/audio pair or FLV segments,
writes cover, subtitle, and danmaku sidecars by default, resumes partial files with HTTP range
requests, retries bounded transient failures, validates advertised media sizes when present, fails
incomplete media shapes, and runs `ffmpeg` unless `--no-mux` is supplied. Use `--no-cover`,
`--no-subtitles`, or `--no-danmaku` to skip individual sidecar families. Danmaku output defaults to
XML; pass `--danmaku-format ass` for ASS-only output or `--danmaku-format xml,ass` to keep both
XML and ASS sidecars.
Use `--subtitle-ai include|prefer-non-ai|exclude-ai|only-ai` to control AI-generated subtitle
sidecars. The default `include` preserves all discovered subtitle tracks.
Use `--only video`, `--only audio`, `--only subtitle`, `--only danmaku`, or `--only cover` for a
single output kind; single-output modes skip muxing.
Use `bbdown plan` to inspect `streams.audios[*].language` and `language_doc`, then pass
`--audio-language <LANG>` to select the first matching DASH audio stream. Explicit video/audio
quality or language selections are included in archive keys so different selected media variants do
not satisfy one another's duplicate preflight.
Use `--output-template`, `--entry-template`, and `--mux-template` to customize the output root,
entry directory, and muxed file stem. Template output is sanitized as a filename component; media,
cover, subtitle, and danmaku sidecar filenames remain stable for resume and duplicate-track safety.
Entry templates must render unique directory names across selected entries.
The CLI avoids suspected PCDN media URLs by default while leaving local and private hosts alone.
Pass `--allow-pcdn` to keep original PCDN candidates, `--upos-host <HOST>` to rewrite DASH/FLV
media candidates to a specific UPOS host, or `--force-replace-host` to rewrite media candidates to
the built-in BBDown fallback host. Cover, subtitle, and danmaku sidecar URLs are not rewritten.
Pass `--progress-json` to stream typed progress events as JSON Lines on stderr while stdout remains
human output or the final `--json` report.
Pass `--archive-file <path>` to record completed downloads by content identity. Archive output,
sidecar, and mux paths are stored as absolute paths at record time so the same archive can be reused
from another working directory. Entry identity uses stable aid/cid media ids, so the same PGC
episode can still match when later planned through its BV/av URL even if one form lacks a BVID.
When the same content, entry, or
archive output directory is seen again, non-interactive JSON mode requires `--on-duplicate replace`,
`--on-duplicate keep-both`, or `--on-duplicate cancel`; interactive human mode prompts when no
decision is provided, and `Ctrl-C` exits that prompt immediately with status 130. `replace` removes
the existing planned output root before a fresh download and
replaces stale archive records for that output path, `keep-both` writes the next suffixed output root
while avoiding all archive record output paths, and `cancel` reports the preflight state without
downloading. The archive file itself must not be the chosen output root or inside that root; the CLI
applies the same guard to archive save sidecar paths. If the archive file is a symlink, saves update
the symlink target so shared archive history is not forked.

Use `bbdown danmaku update <input> --archive-file <path>` to refresh danmaku sidecars for existing
archive records without redownloading media. The command replans the input, finds matching archive
entries by stable aid/cid identity, downloads the current XML danmaku payload, append-merges only
new comments into `danmaku.xml`, regenerates selected derived formats such as `danmaku.ass`, and
saves the updated sidecar paths back to the archive. XML is always the canonical update target;
`--danmaku-format ass` adds or refreshes ASS from the merged XML.

`ss` and `md` inputs require an explicit selection in non-interactive mode:

```bash
bbdown info ss26801 --select latest
bbdown info ss26801 --select all
bbdown info ss26801 --select episode:267851
bbdown info ss26801 --select page:1
```

`cheese/ss...` inputs follow the same explicit-selection rule. Favorite lists, space videos,
collections, series, homepage recommendations, watch history, watch-later lists, following feeds, and space dynamic
feeds are batch inputs. Recommendation input uses `recommendations`, `recommendation`, `recommend`,
or the Bilibili homepage URL; it resolves homepage recommendation batches and currently includes
normal-video `av` cards, walking additional `fresh_idx` refresh batches within a safety cap when
needed to cover explicit index selection. History input uses `history` or
`https://www.bilibili.com/account/history`, requires an authenticated web cookie, and currently
includes normal-video `archive` history records. Watch-later input uses `watchlater`,
`watch-later`, `watch_later`, `later`, `toview`, `https://www.bilibili.com/watchlater`, or
`https://www.bilibili.com/list/watchlater`, requires
an authenticated web cookie, and includes normal videos from the authenticated account's
watch-later list. Following
input uses `following`, `https://t.bilibili.com/`, or
`https://www.bilibili.com/account/dynamic`; space dynamic input uses
`https://space.bilibili.com/<mid>/dynamic`. Dynamic feed inputs require an authenticated web cookie
and currently include normal-video archive cards.

Manage local credentials:

```bash
bbdown auth import-cookie --stdin
bbdown auth import-cookie --file cookie.txt
bbdown auth import-access-key --stdin
bbdown auth login-access-key --stdin < balh-callback.txt
bbdown auth login-access-key --file balh-callback.txt
bbdown auth renew-access-key --json
bbdown auth renew-access-key --stdin < balh-callback.txt
bbdown auth login-web
bbdown auth login-tv
bbdown auth status
bbdown auth health --json
bbdown auth logout
```

Credentials are stored in the platform config directory by default. Use
`--credential-file <path>` to override this path for integration tests or local experiments.
Secret import commands also read `BBDOWN_COOKIE` or `BBDOWN_ACCESS_KEY` when no input flag is
provided, so callers can avoid passing credentials through process arguments. `auth login-access-key`
prints a BiliPlus/BALH-compatible authorization URL plus QR payload, then reads a pasted
`balh-login-credentials:` message or callback URL/query from `--stdin` or `--file`, then saves the
resulting generic intl/Bstar access key. It does not offer an interactive paste prompt because
terminal echo can expose token values in scrollback; `--stdin` must be piped or redirected and will
reject terminal stdin, and `--file` rejects terminal-backed paths. The command never consumes
implicit stdin; pass `--stdin` for pipes or redirects. Use `--message-origin` when ingesting browser
`postMessage` data and `--auth-base` / `--callback-origin` for compatible mocks or deployments.
`auth renew-access-key` evaluates the selected profile's access-key lifecycle metadata and emits a
structured renewal decision. Fresh credentials return `no_action`; missing, unknown, stale,
expiring, expired, or forced credentials return a BiliPlus/BALH reauthorization ticket. Passing
`--stdin` or `--file` to the same command completes that reauthorization and saves the new generic
access key. The command reports `automatic_refresh_readiness` so embedders can distinguish metadata
that says a refresh token was present from a stored refresh secret; this release does not silently
refresh access keys because raw refresh tokens are not persisted.
QR login commands poll the Bilibili QR state machine and save only the resulting credential. WEB QR
login saves a cookie; TV QR login saves a TV-specific access
key without overwriting the generic intl/Bstar access key. With `--json`, login commands emit
newline-delimited JSON events: a `ticket` event with the login URL and `qr_payload` before the
credential handoff or poll, then a `saved` event after credentials are stored. The current WEB and
TV login flows use the scan URL itself as the QR payload. Treat login URLs and QR payloads as
temporary login secrets; status output and the `saved` event expose redacted booleans only.
`auth health` checks configured credentials without printing secret values: the WEB cookie is
checked through the web nav endpoint, while the generic `access_key` and TV `tv_access_key` are
checked through the OAuth info endpoint as signed `access_key` app query values. Generic token probes
currently cover the intl/Bstar scope and use `--passport-base`; they do not prove the same token is
usable for every APP gRPC or proxy consumer. TV token probes use `--tv-passport-poll-base`, which
follows `--tv-passport-base` when only that TV override is supplied. JSON output reports `kind` for
the credential slot, `scope` for the checked consumer, and `missing`, `valid`, `rejected`, or
`request_failed` states for embedding callers and automation.

Use `--request-timeout-seconds` or `BBDOWN_REQUEST_TIMEOUT_SECONDS` to tune API request bounds.
Media body reads use `--download-idle-timeout-seconds`; pass `0` to disable the idle timeout.
Use `--comment-base` or `BBDOWN_COMMENT_BASE` to point danmaku XML downloads at a mock or proxy
endpoint. Use `--passport-base` for WEB QR login and generic token-health mocks or proxies, and use
`--tv-passport-base` / `--tv-passport-poll-base` for TV QR login and TV token-health mocks or
proxies. TV QR polling and TV token-health probes follow
`--tv-passport-base` only when that TV-specific override is supplied; otherwise it uses the upstream
TV poll default unless `--tv-passport-poll-base` is set explicitly.
Use `--playurl-mode tv` with `--tv-api-base` when a plan, playback request, or download should use
the TV playurl host instead of the default web playurl host.
Use `--playurl-mode app` with `--app-grpc-base` and `--app-pgc-grpc-base` when a plan, playback
request, or download should use APP gRPC playurl hosts.

Configure restricted-area PGC playurl fallback with explicit proxy hosts. Fallback runs only when the
official PGC playurl response reports a region/area restriction, or when APP gRPC mode reports a
permission-denied status or preview-only PGC response-body signal:

```bash
bbdown --restricted-area hk --restricted-area-proxy hk=https://proxy.example/playurl plan ep267851 --json
bbdown --restricted-api-proxy tw=https://proxy.example/bili/api plan ss26801 --select latest --json
```

`--restricted-area-proxy` targets BBDown/BiliPlus-style HTTP(S) playurl proxy endpoints.
`--restricted-api-proxy` targets HTTP(S) proxies that mirror `api.bilibili.com` paths and preserves query
parameters already present on the configured proxy base URL. Use repeated flags or comma-separated
`BBDOWN_RESTRICTED_AREA_PROXY` / `BBDOWN_RESTRICTED_API_PROXY` values to configure multiple
candidates. Repeated command-line flags preserve declaration order within the same area priority.
When command-line and environment proxy values are both present, command-line candidates are tried
first, followed by environment playurl proxies and then environment API-path proxies. Each source
group is ordered by area hint, generic candidates, then fixed area order. Hosts are user supplied;
the tool does not ship public proxy defaults. Proxy requests do not forward Bilibili cookies.
Resolver diagnostics reduce endpoints to URL origins and redact sensitive error-message values.

## Release Builds

GitHub tag releases build prepackaged `bbdown` CLI archives for Linux x86_64, macOS x86_64,
macOS aarch64, and Windows x86_64 through the two-phase release candidate and promotion workflow.
Manual release artifact workflow runs can also build the same archives without publishing a tag,
GitHub Release, or crate. Each archive includes the CLI binary, English and Simplified Chinese
README files, the English and Simplified Chinese user, embedding, release, and architecture guides,
and `LICENSE`. Each archive also has an adjacent `.sha256` checksum file. Maintainer release steps
are documented in [docs/release.md](docs/release.md).

## Developer Commands

```bash
just fmt-check
just lint
just test
just e2e
just publish-dry-run
just publish-dry-run-strict
just live-e2e
just ci
```

Default local `just ci` runs formatter, clippy, unit tests, mock e2e tests, and a dirty-tree-friendly
crates.io dry run for the publishable `bbdown-core` library package. GitHub CI runs the same test
gates and a strict clean-checkout crates.io dry run. The CLI crate is not a crates.io publish target; use the
GitHub release archives for binary distribution. `just live-e2e` is intentionally excluded from
default CI and fails fast unless the ignored local `live-e2e.samples.json` exists. Start from
`live-e2e.samples.example.json`, then point `credential_file` and `access_key_file` at local secret
files and list the public, PGC, intl, or restricted PGC samples to probe. The live harness writes an
isolated temporary credential store per case and removes CLI override environment variables before
running, so sample behavior is driven by the manifest rather than shell state.

## Documentation

- Crate API note: the publishable package is `bbdown-core`, imported as `bbdown_core`. This rewrite
  is now on the `0.5.0` development line after the published `0.4.0` release. The line is focused on
  downloader and embedding polish: progress callbacks, cancellation-aware execution, chapter metadata
  muxing, audio language selection, and AI subtitle filtering. The current download APIs include
  `DownloadProgressEvent` callbacks, `DownloadCancellationToken`-based graceful cancellation, and
  ffmpeg chapter metadata muxing when plan entries expose chapters.
  Embedding projects should prefer
  `Default`, `new`, and `with_*` constructors such as `ClientConfig::default().with_*`,
  `EndpointConfig::default().with_*`, `RestrictedAreaConfig::default().with_*`,
  `DownloadOptions::new(...).with_*`, and `RetryPolicy::new(...)` over struct literals so added
  configuration fields are less disruptive. Output model structs such as `DownloadEntry` are
  intended to be consumed as returned values or through serde; their struct-literal construction is
  not treated as a stable compatibility surface. For duplicate handling, embedding projects can inspect
  `DownloadPreflight`, present existing `DownloadArchiveRecord` values to users, then pass an
  explicit `DuplicateDecision`. Batch metadata resolves through `ResolvedContent::Collection`, and
  download planning maps selected collection items back to normal video stream planning entries.
- User guide: [docs/user-guide.md](docs/user-guide.md)
- Embedding guide: [docs/embedding.md](docs/embedding.md)
- Architecture: [docs/architecture/rust-rewrite.md](docs/architecture/rust-rewrite.md)
- Release notes: [docs/release-notes/README.md](docs/release-notes/README.md)
- Simplified Chinese README: [README.zh-CN.md](README.zh-CN.md)
- Simplified Chinese user guide: [docs/user-guide.zh-CN.md](docs/user-guide.zh-CN.md)
- Simplified Chinese embedding guide: [docs/embedding.zh-CN.md](docs/embedding.zh-CN.md)
- Simplified Chinese architecture guide:
  [docs/architecture/rust-rewrite.zh-CN.md](docs/architecture/rust-rewrite.zh-CN.md)
- Simplified Chinese release notes:
  [docs/release-notes/README.zh-CN.md](docs/release-notes/README.zh-CN.md)
- Agent-facing project tracking, not localized: [Project state](docs/PROJECT_STATE.md),
  [Project TODO](docs/PROJECT_TODO.md), and [workstream journals](docs/project_journal/).

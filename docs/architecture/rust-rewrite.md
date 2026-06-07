# Rust Rewrite Architecture

## Goals

- Build a Rust crate that other projects can embed without shelling out to a CLI.
- Keep the CLI as the user-facing tool and e2e test surface.
- Preserve BBDown's practical Bilibili knowledge while replacing CLI-log parsing with typed data.
- Support normal videos, `ep`, `ss`, `md`, intl episodes, and configured restricted-area resolvers.

## Workspace

- `crates/bbdown`: library crate with typed input parsing, metadata models, credential store,
  client config, and resolver APIs.
- `crates/bbdown-cli`: CLI wrapper that uses the crate only through public APIs.
- `docs/`: architecture, user-facing notes, project state, and project journal entries.

## Resolver Model

Inputs normalize into `Input`:

- `Aid` and `Bvid` for normal videos.
- `Episode`, `Season`, and `Media` for Bilibili PGC URLs and ids.
- `IntlEpisode` for `bilibili.tv` episode URLs.

The library resolves metadata into `ResolvedContent`:

- `VideoMetadata` includes title, description, owner, tags, cover, pub time, and pages.
- `SeasonResolution` includes season metadata plus the selected episode set.

The library resolves media availability into `DownloadPlan`:

- `DownloadEntry` records the selected `aid`, `bvid`, `cid`, optional `epid`, title, and source.
- `StreamSet` keeps DASH video/audio tracks, FLV segments, accepted quality ids, and duration.
- `SubtitleTrack` records language metadata, normalized URL, and basic format classification.
- `DanmakuTrack` records the XML comment endpoint derived from `cid`.

`ss` and `md` require a `Selection` in non-interactive contexts. The CLI will later add
interactive prompting, but the library keeps the contract explicit so integrations cannot
accidentally download a full season.

## Stream Planning

`BiliClient::plan` is the public crate API for building a typed download plan from a parsed
`Input` without performing file I/O. `BiliClient::plan_download` remains a raw-string convenience
wrapper for CLI-style callers. Planning currently supports three official source modes:

- `NormalWeb` uses the normal web playurl endpoint for `aid`/`bvid` inputs.
- `PgcWeb` uses the PGC web playurl endpoint for `ep`, `ss`, and `md` inputs.
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

## Download Execution

`BiliClient::download_plan` executes a caller-provided `DownloadPlan`. `BiliClient::download` and
`BiliClient::download_input` are convenience wrappers that plan first, then execute. The executor
returns a typed `DownloadReport` instead of scraping CLI output.

Execution behavior is controlled by `DownloadOptions`:

- output directory;
- bounded retry policy;
- HTTP range resume on or off;
- media read idle timeout;
- subtitle and danmaku sidecar inclusion;
- disabled muxing or explicit `ffmpeg` binary path.

For each entry, execution prefers the first DASH video and first DASH audio track from the plan. If
the entry has no DASH media, it downloads the FLV `durl` segments. Subtitle and danmaku files remain
sidecars. When muxing is enabled, the executor invokes `ffmpeg` with explicit argv and returns the
command plus output path in the report.

Media and sidecar downloads use media headers without account cookies, because media URLs come from
API payloads and can target CDN or proxy hosts. DASH and FLV backup URLs are part of the candidate
list. Media body reads use a separate idle timeout instead of the metadata request timeout. Resume
appends only when `Content-Range` starts at the local file length and completes at the advertised
range total; matching 416 responses are treated as already complete. When a stream or FLV segment
declares a size, the executor rejects mismatched final file lengths and rolls back failed writes to
the pre-attempt length.

The crate default keeps muxing disabled so embedding projects do not spawn external processes by
surprise. The CLI `download` command enables ffmpeg by default and exposes `--no-mux` for users and
mock e2e tests. Mux subprocess stdout and stderr are isolated from CLI stdout so JSON reports remain
parseable.

## Restricted Area And Intl

The project must not hard-code public proxy services. Restricted-area support is designed as a
configured resolver chain:

- official web and PGC APIs;
- intl API using caller-provided access key when available;
- user-configured proxy web or mobile resolver hosts;
- user-configured area hints such as `cn`, `hk`, `tw`, or `th`.

The current implementation supports endpoint override, intl metadata shape, official PGC stream
planning, official intl OGV signed stream planning, typed source reporting, and download execution.
Later slices will add configured proxy candidate ordering based on the same principles.

## Credentials

The CLI stores credentials in a local JSON file under the platform config directory with `0600`
permissions on Unix. The crate exposes `Credentials` and `CredentialStore` so other projects can
inject their own storage or keep credentials in memory.

Secrets are never included in status output; `auth status` reports only booleans.
HTTP request errors are converted without retaining full URLs so query secrets such as intl
`access_key` do not appear in user-facing errors.

## Testing And CI

Default CI is deterministic:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- unit tests
- CLI mock e2e tests

Live tests against Bilibili will be opt-in only. They must require explicit environment variables
for credentials and sample URLs so branch CI is not blocked by network, account, or regional state.
Network requests have a configurable timeout through `ClientConfig` and CLI/env settings so
misbehaving official or proxy endpoints do not hang indefinitely.

## Planned PR Slices

1. Workspace, CI, docs, metadata resolver, credential store, and CLI `info/auth`. Completed in
   PR #1.
2. Stream resolver chain, download planning, subtitle and danmaku discovery. Completed in PR #2.
3. File download, retry/resume policy, ffmpeg mux integration, and mock e2e downloads. Completed
   in PR #3.
4. QR login state machine and live-test opt-in harness.
5. Restricted-area proxy resolver ordering and diagnostics.

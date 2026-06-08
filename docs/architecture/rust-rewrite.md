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

## Public API Shape

The reusable crate keeps configuration ergonomic for embedders through `Default`, `new`, and
builder-style `with_*` methods. `EndpointConfig`, `ClientConfig`, `RestrictedAreaConfig`,
`Credentials`, `DownloadOptions`, `RetryPolicy`, `StreamSelection`, and `MuxOptions` all have
constructor paths so downstream projects do not need struct literals for ordinary integration code.
The CLI uses the same public builders, which makes it an in-repo integration test surface for the
crate API.

Output models remain typed data surfaces. Callers should read fields or serialize them rather than
treating output structs as stable construction targets while the crate remains pre-release.

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
- `StreamSet` keeps DASH video/audio tracks, FLV segments, raw accepted quality ids, structured
  selectable DASH quality labels, and duration.
- `StreamDiagnostics` records non-default resolver attempts such as restricted-area proxy fallback.
- `SubtitleTrack` records language metadata, normalized URL, and basic format classification.
- `DanmakuTrack` records the XML comment endpoint derived from `cid` and the configured comment
  endpoint base.

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
- optional DASH video/audio stream id selection;
- HTTP range resume on or off;
- media read idle timeout;
- subtitle and danmaku sidecar inclusion;
- disabled muxing or explicit `ffmpeg` binary path.

For each entry, execution prefers a complete DASH video/audio pair from the plan. By default this is
the first video and first audio stream; callers can set `StreamSelection::new(...)` to request exact
DASH video or audio stream ids. If a requested id is unavailable, the executor reports the available
ids and fails before media writes. If DASH media is incomplete and FLV `durl` segments are available, it
downloads the FLV segments instead; explicit stream selection requires DASH media and therefore
rejects FLV fallback. Otherwise the entry fails before media writes. Subtitle and danmaku files
remain sidecars. When muxing is enabled, the executor invokes `ffmpeg` with explicit argv and returns
the command plus output path in the report.

Media and sidecar downloads use media headers without account cookies, because media URLs come from
API payloads and can target CDN or proxy hosts. DASH and FLV backup URLs are part of the candidate
list. Media body reads use a separate idle timeout instead of the metadata request timeout. Resume
appends only when `Content-Range` starts at the local file length and completes at the advertised
range total or an expected media size proves the final length; matching 416 responses are treated as
already complete. Wildcard `Content-Range` totals are rejected when no expected size is available.
When a stream or FLV segment declares a size, the executor rejects mismatched final file lengths and
rolls back failed writes to the pre-attempt length. Media responses that complete without writing
bytes are rejected. Entry directories include content identity so same-title videos do not share
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
reservations. Entry-level archive identities use stable aid/bvid/cid content ids instead of display
indexes or optional episode ids, so reordered pages and episode-vs-BV URL forms can still be
detected as duplicates.
`Cancel` is a caller-level stop decision. The CLI exposes the same model with `--archive-file` and
`--on-duplicate`, rejects an archive file path that overlaps the chosen output root by checking both
lexical paths and canonical targets, and applies the same guard to archive save sidecar paths.
JSON/non-TTY mode requires an explicit decision instead of prompting. After showing preflight state,
the CLI executes against the same preflight so a no-conflict default cannot be upgraded into an
implicit replace if an output root appears between preflight and execution. `DownloadArchive::save`
also rejects directory targets before writing the archive file, and when the archive path is a
symlink it writes through to the symlink target so shared archive files keep one history.
Output-root occupancy checks use symlink metadata so stale or broken symlink roots are handled
consistently with replacement cleanup, while metadata errors such as inaccessible parents are
reported to callers instead of being retried as suffixed output roots forever.

The crate default keeps muxing disabled so embedding projects do not spawn external processes by
surprise. The CLI `download` command enables ffmpeg by default and exposes `--no-mux` for users and
mock e2e tests. Mux subprocess stdin, stdout, and stderr are isolated from CLI stdio. Muxing writes
to a temporary output first, validates that output, and then replaces the final file, so a failed
rerun preserves an existing muxed file and JSON reports remain parseable and accurate.

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

PGC stream planning first calls the official PGC web playurl endpoint. If that response clearly
reports a region/area restriction and restricted-area proxies are configured, the client tries
ordered candidates until one returns a valid DASH or FLV stream shape. Non-area official failures keep
their original error and do not contact proxy hosts. A BBDown/BiliPlus-style HTTP(S) playurl proxy
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

The current implementation supports endpoint override, intl metadata shape, official PGC stream
planning, official intl OGV signed stream planning, configured PGC proxy fallback, top-level helper
playurl response parsing, typed source reporting, resolver diagnostics, and download execution.
Browser-only mobile response rewriting remains intentionally out of scope.

## Credentials

The CLI stores credentials in a local JSON file under the platform config directory with `0600`
permissions on Unix. The crate exposes `Credentials` and `CredentialStore` so other projects can
inject their own storage or keep credentials in memory.

QR login is modeled as an explicit state machine in the crate. WEB QR login creates a
`QrLoginTicket`, polls waiting-for-scan, waiting-for-confirmation, expired, and succeeded states,
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
only booleans. The QR login `ticket` event and human scan output intentionally expose the scan URL so
the user can authenticate, and callers should treat that URL as a temporary login secret. The public QR
state enum intentionally does not derive serde traits because the succeeded state carries full
credentials for embedding callers that handle storage themselves. QR ticket debug output is redacted
because ticket keys and scan URL query strings can act as pre-authentication secrets.
HTTP request errors are converted without retaining full URLs so query secrets such as intl
`access_key` do not appear in user-facing errors.

## Testing And CI

Default CI is deterministic:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- unit tests
- CLI mock e2e tests
- crates.io dry-run packaging for the publishable `bbdown` library crate

Release packaging is a separate GitHub Actions workflow. Tag pushes matching `v*` build Linux
x86_64, macOS x86_64, macOS aarch64, and Windows x86_64 CLI archives and publish them to the
GitHub Release with generated release notes. Manual workflow dispatch builds the same archives as
downloadable workflow artifacts without publishing a release. Archives contain the `bbdown` binary,
`README.md`, `docs/user-guide.md`, `docs/embedding.md`, and `LICENSE`. Each archive also has an
adjacent platform-specific checksum file. Action references in the release workflow are pinned to
commit SHAs. Package names normalize release refs to the packager-safe `[A-Za-z0-9._-]` character
set, so tags such as SemVer build metadata do not fail at packaging time.

Crate publishing is intentionally scoped to the reusable `bbdown` library crate. The crate has
crates.io metadata, a package-local README and LICENSE, dirty-tree-friendly local publish dry-run
validation, and CI-backed `cargo publish --dry-run -p bbdown --locked` validation. `bbdown-cli`
remains `publish = false` because CLI distribution is handled by GitHub release archives.

Plan output now exposes structured stream quality data. The library keeps raw
`StreamSet::accept_quality` for compatibility and adds `StreamSet::qualities` with actual selectable
DASH video ids plus optional labels derived from `accept_description` and `support_formats`. The CLI
human summary prints the same ids alongside video/audio stream summaries, while JSON callers can
select exact DASH streams through `DownloadOptions::stream_selection`.

The reusable crate is still preparing for its first crates.io release, so this branch intentionally
hardens public structs before publishing rather than preserving local pre-release struct-literal
experiments. Embedders should create configuration with constructor and builder APIs, including
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
reporting, preflight JSON round-trip reservation preservation, episode-vs-video entry identity,
symlink archive target saves, and directory-target archive save rejection. CLI mock e2e tests cover
JSON duplicate failure without an
explicit decision, `cancel` preflight output, `keep-both` suffixed output roots, `replace`
overwriting an existing file, symlink archive target updates, and rejecting an archive file path that
overlaps the chosen output root lexically or through canonicalized targets, including archive save
sidecar paths.

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

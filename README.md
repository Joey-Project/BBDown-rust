[ English | [简体中文](README.zh-CN.md) ]

# BBDown Rust

`BBDown Rust` is a Rust-native rewrite of BBDown with two goals:

- expose a reusable `bbdown-core` package / `bbdown_core` crate for other Rust projects;
- provide a CLI that can serve as the e2e surface for metadata resolution and downloads.

This project uses the original [BBDown](https://github.com/nilaoda/BBDown) project as a practical
Bilibili behavior reference. Thanks to BBDown and its contributors for that reference.

The current implementation establishes the crate/CLI/CI foundation, metadata resolver, stream
planning, media downloads, sidecar downloads, retry/resume behavior, optional ffmpeg muxing, QR
login, opt-in live test harnesses, configured restricted-area proxy ordering with diagnostics, and
builder-style crate integration APIs. It also supports an explicit download archive for duplicate
preflight and CLI replace / keep-both / cancel decisions. Input parsing covers normal videos, PGC
and intl episodes, PUGV/cheese courses, B23 short links, favorite lists, space videos, collections,
and series.

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
bbdown info https://space.bilibili.com/123/lists/456?type=series --json
```

Build a download plan as JSON:

```bash
bbdown plan av170001 --json
bbdown plan ep267851 --json
bbdown plan ss26801 --select latest --json
bbdown plan https://www.bilibili.tv/en/play/34613/341736 --json
bbdown plan fav456 --select page:1 --json
bbdown plan cheese/ss202 --select latest --json
```

`plan` resolves the selected entries, available DASH or FLV stream URLs, subtitle URLs, and the
danmaku XML URL for each `cid`. PGC and intl planning may still require eligible account or region
access. PGC playurl resolution can fall back to user-configured restricted-area proxies. It does
not download files. Collection-like inputs default to all items; use `--select page:<index>` to plan
one collection item or `--select latest` for the newest parsed item. `info --json` keeps full
parsed collection metadata under `collection.collection.items`; `plan` emits only selected entries.

Download selected media files:

```bash
bbdown download av170001 --output-dir downloads
bbdown download ss26801 --select latest --output-dir downloads
bbdown download fav456 --select page:1 --output-dir downloads
bbdown download av170001 --output-dir downloads --no-mux --json
bbdown download av170001 --output-dir downloads --archive-file downloads/archive.json --on-duplicate keep-both
```

`download` resolves a plan, downloads the first complete DASH video/audio pair or FLV segments,
writes subtitle and danmaku sidecars by default, resumes partial files with HTTP range requests,
retries bounded transient failures, validates advertised media sizes when present, fails incomplete
media shapes, and runs `ffmpeg` unless `--no-mux` is supplied.
Pass `--archive-file <path>` to record completed downloads by content identity. Archive output,
sidecar, and mux paths are stored as absolute paths at record time so the same archive can be reused
from another working directory. Entry identity uses stable aid/cid media ids, so the same PGC
episode can still match when later planned through its BV/av URL even if one form lacks a BVID.
When the same content, entry, or
archive output directory is seen again, non-interactive JSON mode requires `--on-duplicate replace`,
`--on-duplicate keep-both`, or `--on-duplicate cancel`; interactive human mode prompts when no
decision is provided. `replace` removes the existing planned output root before a fresh download and
replaces stale archive records for that output path, `keep-both` writes the next suffixed output root
while avoiding all archive record output paths, and `cancel` reports the preflight state without
downloading. The archive file itself must not be the chosen output root or inside that root; the CLI
applies the same guard to archive save sidecar paths. If the archive file is a symlink, saves update
the symlink target so shared archive history is not forked.

`ss` and `md` inputs require an explicit selection in non-interactive mode:

```bash
bbdown info ss26801 --select latest
bbdown info ss26801 --select all
bbdown info ss26801 --select episode:267851
bbdown info ss26801 --select page:1
```

`cheese/ss...` inputs follow the same explicit-selection rule. Favorite lists, space videos,
collections, and series are batch inputs; without `--select`, they resolve all parsed items.

Manage local credentials:

```bash
bbdown auth import-cookie --stdin
bbdown auth import-cookie --file cookie.txt
bbdown auth import-access-key --stdin
bbdown auth login-web
bbdown auth login-tv
bbdown auth status
bbdown auth logout
```

Credentials are stored in the platform config directory by default. Use
`--credential-file <path>` to override this path for integration tests or local experiments.
Secret import commands also read `BBDOWN_COOKIE` or `BBDOWN_ACCESS_KEY` when no input flag is
provided, so callers can avoid passing credentials through process arguments. QR login commands poll
the Bilibili QR state machine and save only the resulting credential. WEB QR login saves a cookie;
TV QR login saves a TV-specific access key without overwriting the generic intl/Bstar access key.
With `--json`, QR login emits newline-delimited JSON events: a `ticket` event with the scan URL
before polling, then a `saved` event after credentials are stored. Treat the scan URL as a temporary
login secret; status output and the `saved` event expose redacted booleans only.

Use `--request-timeout-seconds` or `BBDOWN_REQUEST_TIMEOUT_SECONDS` to tune API request bounds.
Media body reads use `--download-idle-timeout-seconds`; pass `0` to disable the idle timeout.
Use `--comment-base` or `BBDOWN_COMMENT_BASE` to point danmaku XML downloads at a mock or proxy
endpoint. Use `--passport-base` for WEB QR login mocks or proxies, and use `--tv-passport-base` /
`--tv-passport-poll-base` for TV QR login mocks or proxies. TV QR polling follows
`--tv-passport-base` only when that TV-specific override is supplied; otherwise it uses the upstream
TV poll default unless `--tv-passport-poll-base` is set explicitly.

Configure restricted-area PGC playurl fallback with explicit proxy hosts. Fallback runs only when the
official PGC playurl response reports a region/area restriction:

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
  is still `0.1`; embedding projects should prefer `Default`, `new`, and `with_*` constructors such
  as `ClientConfig::default().with_*`, `EndpointConfig::default().with_*`,
  `RestrictedAreaConfig::default().with_*`, `DownloadOptions::new(...).with_*`, and
  `RetryPolicy::new(...)` over struct literals so added configuration fields are less disruptive.
  Output model structs such as `DownloadEntry` are intended to be consumed as returned values or
  through serde; their struct-literal construction is not treated as a stable compatibility surface
  before the crate leaves `0.1`. For duplicate handling, embedding projects can inspect
  `DownloadPreflight`, present existing `DownloadArchiveRecord` values to users, then pass an
  explicit `DuplicateDecision`. Batch metadata resolves through `ResolvedContent::Collection`, and
  download planning maps selected collection items back to normal video stream planning entries.
- User guide: [docs/user-guide.md](docs/user-guide.md)
- Embedding guide: [docs/embedding.md](docs/embedding.md)
- Architecture: [docs/architecture/rust-rewrite.md](docs/architecture/rust-rewrite.md)
- Simplified Chinese README: [README.zh-CN.md](README.zh-CN.md)
- Simplified Chinese user guide: [docs/user-guide.zh-CN.md](docs/user-guide.zh-CN.md)
- Simplified Chinese embedding guide: [docs/embedding.zh-CN.md](docs/embedding.zh-CN.md)
- Simplified Chinese architecture guide:
  [docs/architecture/rust-rewrite.zh-CN.md](docs/architecture/rust-rewrite.zh-CN.md)
- Agent-facing project tracking, not localized: [Project state](docs/PROJECT_STATE.md),
  [Project TODO](docs/PROJECT_TODO.md), and [workstream journals](docs/project_journal/).

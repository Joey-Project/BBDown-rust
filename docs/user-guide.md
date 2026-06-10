[ English | [简体中文](user-guide.zh-CN.md) ]

# User Guide

## Scope

BBDown Rust currently exposes a reusable `bbdown-core` package / `bbdown_core` crate and a CLI for
deterministic metadata, download-plan resolution, media download execution, sidecar downloads, and
optional ffmpeg muxing. Supported input families include normal videos, PGC and intl episodes,
PUGV/cheese courses, B23 short links, favorite lists, space videos, collections, and series.

## Release Archives

GitHub releases provide prepackaged `bbdown` CLI archives for Linux x86_64, macOS x86_64,
macOS aarch64, and Windows x86_64. Download the archive for your platform, verify it against the
adjacent `.sha256` file if needed, extract it, and place the `bbdown` or `bbdown.exe` binary on your
`PATH`. Archives also include English and Simplified Chinese README files, user guides, embedding
guides, architecture guides, and `LICENSE`. After installing the binary, run `bbdown --version` to
confirm the archive version.

## Crate Publishing

The crates.io publish target is the reusable `bbdown-core` library package. Use
`just publish-dry-run` for a local locked dry run that tolerates an uncommitted worktree, and use
`just publish-dry-run-strict` or `cargo publish --dry-run -p bbdown-core --locked` to reproduce the
clean CI gate. The `bbdown-cli` package is marked `publish = false`; install or distribute the CLI
through GitHub release archives instead. The current development line is `0.2.0` after the published
`0.1.0` release because batch collection metadata adds `ResolvedContent::Collection`. Embedding
callers should still prefer constructors such as `DownloadOptions::new`, `StreamSelection::new`, and
`Default` over public struct literals, and treat public plan output containers as consumed data
surfaces that may gain fields while the crate matures.

## Library Embedding

Use the `bbdown_core` crate from the `bbdown-core` package when another Rust project needs typed
metadata, download plans, download execution reports, or restricted-area resolver diagnostics
without spawning the CLI. The repository embedding guide has copyable examples for `ClientConfig`,
`EndpointConfig`, credentials, restricted-area proxies, and `DownloadOptions`: `docs/embedding.md`.

Embedding callers should keep configuration construction on `Default`, `new`, and `with_*` methods.
The CLI in this workspace follows the same path, so these builders are covered by normal CI and mock
e2e tests.

## Metadata

Use `info` to resolve metadata:

```bash
bbdown info av170001 --json
bbdown info BV1qt4y1X7TW --json
bbdown info ep267851 --json
bbdown info ss26801 --select latest --json
bbdown info md22718131 --select episode:267851 --json
bbdown info https://b23.tv/example --json
bbdown info cheese/ep101 --json
bbdown info cheese/ss202 --select latest --json
bbdown info fav456 --json
bbdown info mid123 --select page:1 --json
bbdown info collection456 --json
bbdown info series456 --select latest --json
bbdown info https://www.bilibili.com/medialist/detail/ml1103407912 --json
bbdown info https://www.bilibili.com/list/ml1103407912 --json
bbdown info 'https://www.bilibili.com/list/1958703906?sid=547718' --json
bbdown info 'https://space.bilibili.com/123/favlist?fid=456' --json
bbdown info 'https://space.bilibili.com/123/lists/456?type=series' --json
```

Season, media, and `cheese/ss...` inputs require `--select` in non-interactive mode. Supported
selectors are `latest`, `all`, `episode:<epid>`, and `page:<index>`. `current` is only meaningful
for `ep`, `cheese/ep`, and `bilibili.tv` episode URLs, where the input already identifies the
current episode. Favorite lists, space videos, collections, and series are batch inputs; without
`--select`, they resolve all parsed items. Use `--select page:<index>` for one collection item or
`--select latest` for the first parsed item in the upstream list order. JSON metadata keeps the full parsed collection item
list under `collection.collection.items` and reports the active subset under
`collection.selected_items`; empty
collections are valid empty lists.

Favorite list URLs are accepted from shorthand ids, space favlist pages, canonical
`/list/ml...` pages, and `/medialist/.../ml...` pages. Space collection and series URLs retain the
owner mid from `/space.bilibili.com/<mid>/...` or `/list/<mid>?sid=...` so the resolver can use the
newer owner-scoped space APIs.

## Download Plans

Use `plan` to resolve stream, subtitle, and danmaku availability:

```bash
bbdown plan av170001 --json
bbdown plan ep267851 --json
bbdown plan ss26801 --select latest --json
bbdown plan https://www.bilibili.tv/en/play/34613/341736 --json
bbdown plan cheese/ep101 --json
bbdown plan fav456 --select page:1 --json
bbdown plan 'https://space.bilibili.com/123/channel/collectiondetail?sid=456' --select all --json
```

The JSON output contains:

- `entries`: selected pages, episodes, or batch collection items.
- `streams.qualities`: currently selectable DASH video quality ids with optional descriptions from
  the playurl response.
- `streams.accept_quality`: raw accepted video quality ids retained for compatibility.
- `streams.videos`: DASH video tracks.
- `streams.audios`: DASH audio tracks, including available Dolby or FLAC audio.
- `streams.flv_segments`: legacy FLV segments when the playurl response uses `durl`.
- `cover_url`: optional cover image URL used by download cover sidecars.
- `subtitles`: discovered subtitle tracks.
- `danmaku.xml_url`: the XML comment endpoint for the entry `cid`.

Planning is side-effect free. It does not create files, download media, or call ffmpeg.
For batch inputs, planning fetches and emits only the selected entries because collection metadata
belongs to `info`.
Human-readable plan output lists the same selectable quality ids and stream summaries, so users can
choose download quality without parsing JSON by hand.
PGC and intl planning may still require eligible account or region access. When intl metadata
returns a region-limit payload, the CLI reports it as an access restriction.
When configured, PGC playurl resolution falls back to restricted-area proxy candidates and includes
resolver diagnostics in the entry JSON.

## Downloads

Use `download` to resolve a plan and write files:

```bash
bbdown download av170001 --output-dir downloads
bbdown download ss26801 --select latest --output-dir downloads
bbdown download fav456 --select page:1 --output-dir downloads
bbdown download av170001 --output-dir downloads --no-mux --json
bbdown download av170001 --video-quality 64 --audio-quality 30216 --output-dir downloads
bbdown download av170001 --only subtitle --output-dir downloads --json
bbdown download av170001 --output-dir downloads --archive-file downloads/archive.json --on-duplicate keep-both
```

The command downloads the first complete DASH video/audio pair for each entry by default. Use
`bbdown plan` first to inspect available ids, then pass `--video-quality <ID>` or
`--audio-quality <ID>` to select a specific DASH video or audio stream. A requested id must exist in
the plan for that entry; otherwise the command reports the available ids and fails before writing
media. When DASH media is incomplete and legacy FLV `durl` segments are available, it downloads
those segments instead. Explicit quality selection requires DASH media and therefore disables FLV
fallback for that entry. If neither shape is complete, the download fails before writing media.
Cover, subtitle, and danmaku sidecars are enabled by default when the plan has those URLs. Disable
them individually with `--no-cover`, `--no-subtitles`, and `--no-danmaku`.

Use `--only video`, `--only audio`, `--only subtitle`, `--only danmaku`, or `--only cover` to write
one output kind for each planned entry. `video` and `audio` modes select DASH streams and accept the
matching quality flag; `subtitle`, `danmaku`, and `cover` modes do not require media streams and
reject media quality flags. Single-output modes skip muxing even when `--no-mux` is omitted.
When `--archive-file` is used, single-output records are tracked separately from full downloads, so
a cover-only or audio-only run does not mark the complete media download as already done.

Downloads resume partial files by default with HTTP range requests and validate `Content-Range`
plus advertised media sizes when the plan provides them. Use `--no-resume` to force a fresh write;
failed fresh writes preserve any existing target. If a server ignores a resume range, the old
partial is replaced from a temporary full retry after available length checks pass; without a
length signal, the full retry is rejected so the existing file is preserved. Retry behavior is
bounded by `--retry-attempts` and `--retry-backoff-ms`. Entry directories include content
identity, and DASH media filenames include stream metadata identity, so same-title videos and
different codec variants do not share the same resume target. Media downloads that complete without
writing any bytes are rejected.

Use `--archive-file <path>` when a caller wants duplicate preflight and a durable record of
completed downloads. The archive is a local JSON file keyed by content identity; it records output
paths, entry ids, sidecar paths, mux output paths, and completion timestamps, but it does not store
media URLs or credentials. Output, sidecar, and mux paths are stored as absolute paths at record
time so the archive can be reused from another working directory. Entry identity uses stable
aid/cid media ids, so the same PGC episode can still match when later planned through its BV/av URL
even if one form lacks a BVID. When a planned content key, entry identity, or archive output
directory already exists, the CLI needs a duplicate decision:

- `--on-duplicate replace` removes the existing planned output directory or file before a fresh
  download and replaces stale archive records that pointed at that output path.
- `--on-duplicate keep-both` writes to the next available suffixed output directory such as
  `Mock video (2)` and preserves prior archive records, including archive-only records whose old
  output directory is no longer present on disk.
- `--on-duplicate cancel` stops before downloading and, with `--json`, prints
  `{"status":"canceled","preflight":...}` so automation can inspect existing records and output
  conflicts.

Without an explicit decision, human TTY mode prompts on stderr. `--json` mode and non-TTY mode never
prompt; they fail with instructions to pass `--on-duplicate` instead. Without `--archive-file`,
download behavior is unchanged and no duplicate preflight runs. `--archive-file` must point to a JSON
file path that does not overlap the chosen output directory for the selected content and duplicate
decision; for `keep-both`, that check is applied to the actual suffixed output directory. If
`--archive-file` is a symlink, saves update the symlink target so multiple callers can share one
archive path without forking history. The CLI also rechecks the archive-file guard against the
actual output directory reported by the executor before saving the archive.

`--request-timeout-seconds` applies to API requests. Media body reads use
`--download-idle-timeout-seconds`; pass `0` to disable that idle timeout.

Muxing is enabled by default through `ffmpeg`. Use `--ffmpeg <path>` to choose a binary or
`--no-mux` to keep downloaded media files as sidecars only. The reusable crate keeps external
process execution explicit through `DownloadOptions::mux`. A mux rerun writes and validates a
temporary output before replacing the final file, so a failed mux keeps any existing muxed file.

## Credentials

Import credentials when an endpoint requires account access:

```bash
bbdown auth import-cookie --stdin
bbdown auth import-access-key --stdin
bbdown auth login-web
bbdown auth login-tv
bbdown auth status
bbdown auth logout
```

Secret import commands also read `BBDOWN_COOKIE` and `BBDOWN_ACCESS_KEY` when no input flag is
provided. Use `--credential-file <path>` to isolate test credentials from the default platform
config path. `auth login-web` prints a QR login URL, polls until scan confirmation, and saves the
resulting cookie. `auth login-tv` uses the TV QR flow and saves a TV-specific access key for future
TV/app flows without overwriting the generic intl/Bstar access key imported by `auth import-access-key`.
With `--json`, QR login prints newline-delimited JSON events: `ticket` includes the scan URL before
polling, and `saved` includes only redacted credential booleans. Treat the scan URL as a temporary
login secret because it contains the QR login key. Token values are not printed by status or the
`saved` JSON event.

## Endpoint Overrides

The CLI accepts endpoint overrides for mock tests and future resolver chains:

```bash
bbdown --api-base http://127.0.0.1:8080 plan av170001 --json
bbdown --pgc-base http://127.0.0.1:8080 --api-base http://127.0.0.1:8080 plan ep267851 --json
bbdown --intl-base http://127.0.0.1:8080 plan https://www.bilibili.tv/en/play/34613/341736 --json
bbdown --comment-base http://127.0.0.1:8080 download av170001 --output-dir downloads
bbdown --passport-base http://127.0.0.1:8080 auth login-web
bbdown --tv-passport-base http://127.0.0.1:8080 auth login-tv
bbdown --tv-passport-base http://127.0.0.1:8080 --tv-passport-poll-base http://127.0.0.1:8081 auth login-tv
```

Current intl support uses official intl metadata/subtitle endpoints and the official signed intl OGV
playurl endpoint with the configured access key when present. Danmaku XML downloads use the
configurable comment endpoint. WEB QR login uses `--passport-base`; TV QR login uses TV-specific
passport overrides. TV QR polling follows `--tv-passport-base` when that override is supplied; set
`--tv-passport-poll-base` for split-host mocks or proxies.

## Live E2E Samples

`just live-e2e` is a local-only validation gate for real Bilibili samples. It is not part of default
CI because the results depend on network, account, token, and regional eligibility. The recipe
requires an ignored `live-e2e.samples.json` manifest in the repository root; use
`live-e2e.samples.example.json` as the tracked shape.

The manifest can point to an existing credential file with `credential_file`, read a plain text
access key from `access_key_file`, set `request_timeout_seconds`, and configure restricted proxy
candidates with the same area names used by the CLI. `restricted_api_proxy_all_areas` and
`restricted_area_proxy_all_areas` expand each listed URL to `cn`, `th`, `hk`, and `tw` candidates.
Each case declares a `kind`, `url`, optional `selection`, optional restricted-area hint, actions
such as `info` or `plan`, and expected JSON shape. The harness copies only cookie/access-key fields
into a temporary credential file for the case and strips CLI override environment variables before
running the real `bbdown` binary. Unknown manifest fields are rejected so typoed expectation keys
fail fast instead of silently weakening the live gate.
Use `allowed_plan_sources` to reject unexpected sources, and `required_plan_sources` when a source
must appear at least once. For restricted samples that depend on mutable account or regional
eligibility, a case may set `allow_plan_error: true` with `plan_error_contains`: a successful `plan`
must still match the stream assertions, while a restricted failure must be an access-restricted
failure and must contain the listed diagnostics.

## Restricted-Area Proxies

The tool does not include public proxy defaults. Configure only proxy hosts you operate or trust.
PGC playurl fallback is attempted only after the official PGC playurl response reports a region/area
restriction. Other official failures, such as VIP/paywall errors, parse failures, or network errors,
keep their original error instead of trying proxy hosts.

```bash
bbdown --restricted-area hk --restricted-area-proxy hk=https://proxy.example/playurl plan ep267851 --json
bbdown --restricted-api-proxy tw=https://proxy.example/bili/api plan ss26801 --select latest --json
```

Proxy specs use `area=url` or a bare URL. Supported areas are `cn`, `th`, `hk`, and `tw`. Bare URLs
are generic candidates. `--restricted-area <area>` is a hint that moves matching candidates to the
front. Without a hint, ordering is generic, `cn`, `th`, `hk`, then `tw`, with duplicates removed.
Repeated command-line proxy flags preserve declaration order within the same area priority.
When command-line and environment proxy values are both present, command-line candidates are tried
first, followed by environment playurl proxies and then environment API-path proxies. Each source
group is ordered by area hint, generic candidates, then fixed area order.

`--restricted-area-proxy` targets BBDown/BiliPlus-style HTTP(S) playurl proxy endpoints where the
original PGC playurl query is sent to the configured URL. `--restricted-api-proxy` targets HTTP(S)
proxies that mirror `api.bilibili.com` path layout, so the CLI calls `/pgc/player/web/playurl`
below that base URL first, matching common BALH-style API proxy hosts, then falls back to
`/pgc/player/web/v2/playurl` for API proxies that implemented the older path.
If the configured API proxy base URL already contains a query string, that query is preserved before
the PGC playurl parameters are appended. Proxy responses may be wrapped in `data` / `result`, or may
return helper-style top-level `dash` / `durl`, `timelength`, and quality metadata; legacy string
status fields such as `result: "suee"` are tolerated. Both flags may be repeated.
`BBDOWN_RESTRICTED_AREA_PROXY` and `BBDOWN_RESTRICTED_API_PROXY` also accept comma-separated lists.

If a generic access key was imported with `auth import-access-key`, proxy playurl requests include
it as `access_key`. Bilibili cookies are not forwarded to restricted-area proxy hosts. Resolver
diagnostics record the official failure and proxy attempts, but endpoint fields are reduced to URL
origins and sensitive error-message values are redacted so token values are not printed.

# User Guide

## Scope

BBDown Rust currently exposes a reusable `bbdown` crate and a CLI for deterministic metadata,
download-plan resolution, media download execution, sidecar downloads, and optional ffmpeg muxing.

## Release Archives

GitHub releases provide prepackaged `bbdown` CLI archives for Linux x86_64, macOS x86_64,
macOS aarch64, and Windows x86_64. Download the archive for your platform, verify it against the
adjacent `.sha256` file if needed, extract it, and place the `bbdown` or `bbdown.exe` binary on your
`PATH`. Archives also include `README.md`, this user guide, and `LICENSE`.

## Crate Publishing

The crates.io publish target is the reusable `bbdown` library crate. Use `just publish-dry-run` for a
local locked dry run that tolerates an uncommitted worktree, and use
`just publish-dry-run-strict` or `cargo publish --dry-run -p bbdown --locked` to reproduce the clean
CI gate. The `bbdown-cli` package is marked `publish = false`; install or distribute the CLI through
GitHub release archives instead. The library is still preparing for its first crates.io release; this
pre-release branch intentionally hardens public structs before publishing. Embedding callers should
prefer constructors such as `DownloadOptions::new` and `Default` over public struct literals, and
treat public plan output containers as consumed data surfaces.

## Metadata

Use `info` to resolve metadata:

```bash
bbdown info av170001 --json
bbdown info BV1qt4y1X7TW --json
bbdown info ep267851 --json
bbdown info ss26801 --select latest --json
bbdown info md22718131 --select episode:267851 --json
```

Season and media inputs require `--select` in non-interactive mode. Supported selectors are
`latest`, `all`, `episode:<epid>`, and `page:<index>`. `current` is only meaningful for `ep` and
`bilibili.tv` episode URLs, where the input already identifies the current episode.

## Download Plans

Use `plan` to resolve stream, subtitle, and danmaku availability:

```bash
bbdown plan av170001 --json
bbdown plan ep267851 --json
bbdown plan ss26801 --select latest --json
bbdown plan https://www.bilibili.tv/en/play/34613/341736 --json
```

The JSON output contains:

- `entries`: selected pages or episodes.
- `streams.qualities`: currently selectable DASH video quality ids with optional descriptions from
  the playurl response.
- `streams.accept_quality`: raw accepted video quality ids retained for compatibility.
- `streams.videos`: DASH video tracks.
- `streams.audios`: DASH audio tracks, including available Dolby or FLAC audio.
- `streams.flv_segments`: legacy FLV segments when the playurl response uses `durl`.
- `subtitles`: discovered subtitle tracks.
- `danmaku.xml_url`: the XML comment endpoint for the entry `cid`.

Planning is side-effect free. It does not create files, download media, or call ffmpeg.
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
bbdown download av170001 --output-dir downloads --no-mux --json
bbdown download av170001 --video-quality 64 --audio-quality 30216 --output-dir downloads
```

The command downloads the first complete DASH video/audio pair for each entry by default. Use
`bbdown plan` first to inspect available ids, then pass `--video-quality <ID>` or
`--audio-quality <ID>` to select a specific DASH video or audio stream. A requested id must exist in
the plan for that entry; otherwise the command reports the available ids and fails before writing
media. When DASH media is incomplete and legacy FLV `durl` segments are available, it downloads
those segments instead. Explicit quality selection requires DASH media and therefore disables FLV
fallback for that entry. If neither shape is complete, the download fails before writing media.
Subtitle and danmaku sidecars are enabled by default and can be disabled with `--no-subtitles` and
`--no-danmaku`.

Downloads resume partial files by default with HTTP range requests and validate `Content-Range`
plus advertised media sizes when the plan provides them. Use `--no-resume` to force a fresh write;
failed fresh writes preserve any existing target. If a server ignores a resume range, the old
partial is replaced from a temporary full retry after available length checks pass; without a
length signal, the full retry is rejected so the existing file is preserved. Retry behavior is
bounded by `--retry-attempts` and `--retry-backoff-ms`. Entry directories include content
identity, and DASH media filenames include stream metadata identity, so same-title videos and
different codec variants do not share the same resume target. Media downloads that complete without
writing any bytes are rejected.

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
the PGC playurl parameters are appended. Both flags may be repeated.
`BBDOWN_RESTRICTED_AREA_PROXY` and `BBDOWN_RESTRICTED_API_PROXY` also accept comma-separated lists.

If a generic access key was imported with `auth import-access-key`, proxy playurl requests include
it as `access_key`. Bilibili cookies are not forwarded to restricted-area proxy hosts. Resolver
diagnostics record the official failure and proxy attempts, but endpoint fields are reduced to URL
origins and sensitive error-message values are redacted so token values are not printed.

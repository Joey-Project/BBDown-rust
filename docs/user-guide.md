# User Guide

## Scope

BBDown Rust currently exposes a reusable `bbdown` crate and a CLI for deterministic metadata,
download-plan resolution, media download execution, sidecar downloads, and optional ffmpeg muxing.

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
- `streams.videos`: DASH video tracks.
- `streams.audios`: DASH audio tracks, including available Dolby or FLAC audio.
- `streams.flv_segments`: legacy FLV segments when the playurl response uses `durl`.
- `subtitles`: discovered subtitle tracks.
- `danmaku.xml_url`: the XML comment endpoint for the entry `cid`.

Planning is side-effect free. It does not create files, download media, or call ffmpeg.
PGC and intl planning may still require eligible account or region access. When intl metadata
returns a region-limit payload, the CLI reports it as an access restriction.

## Downloads

Use `download` to resolve a plan and write files:

```bash
bbdown download av170001 --output-dir downloads
bbdown download ss26801 --select latest --output-dir downloads
bbdown download av170001 --output-dir downloads --no-mux --json
```

The command downloads the first complete DASH video/audio pair for each entry. When DASH media is
incomplete and legacy FLV `durl` segments are available, it downloads those segments instead; if
neither shape is complete, the download fails before writing media. Subtitle and danmaku sidecars
are enabled by default and can be disabled with `--no-subtitles` and `--no-danmaku`.

Downloads resume partial files by default with HTTP range requests and validate `Content-Range`
plus advertised media sizes when the plan provides them. Use `--no-resume` to force a fresh write;
failed fresh writes preserve any existing target. If a server ignores a resume range, the old
partial is replaced from a temporary full retry after available length checks pass; without a
length signal, a shorter retry response is rejected so the existing file is preserved. Retry behavior
is bounded by `--retry-attempts` and `--retry-backoff-ms`. Entry directories include content
identity, and DASH media filenames include stream metadata identity, so same-title videos and
different codec variants do not share the same resume target.

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
bbdown auth status
bbdown auth logout
```

Secret import commands also read `BBDOWN_COOKIE` and `BBDOWN_ACCESS_KEY` when no input flag is
provided. Use `--credential-file <path>` to isolate test credentials from the default platform
config path.

## Endpoint Overrides

The CLI accepts endpoint overrides for mock tests and future resolver chains:

```bash
bbdown --api-base http://127.0.0.1:8080 plan av170001 --json
bbdown --pgc-base http://127.0.0.1:8080 --api-base http://127.0.0.1:8080 plan ep267851 --json
bbdown --intl-base http://127.0.0.1:8080 plan https://www.bilibili.tv/en/play/34613/341736 --json
bbdown --comment-base http://127.0.0.1:8080 download av170001 --output-dir downloads
```

Restricted-area proxy ordering is not implemented yet. Current intl support uses official intl
metadata/subtitle endpoints and the official signed intl OGV playurl endpoint with the configured
access key when present. Danmaku XML downloads use the configurable comment endpoint.

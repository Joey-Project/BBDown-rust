---
id: 20260607-019e9eab-stream-planning
title: Stream Planning
status: completed
created: 2026-06-07
updated: 2026-06-07
branch: wip/stream-planning
pr: https://github.com/Joey-Project/BBDown-rust/pull/2
supersedes: []
superseded_by:
---

# Stream Planning

## Summary

- Second PR slice for the BBDown Rust rewrite.
- Scope is typed download planning, official normal/PGC/intl playurl resolution, subtitle
  discovery, danmaku URL discovery, user-facing `plan` CLI output, and mock e2e coverage.
- File download execution, retry/resume policy, ffmpeg muxing, QR login, and restricted-area proxy
  candidate ordering remain follow-up slices.

## Current State

- `BiliClient::plan` returns `DownloadPlan` values for parsed normal video, PGC, and intl episode
  inputs; `BiliClient::plan_download` remains a raw-string wrapper for CLI-style callers.
- `DownloadEntry` records stream source, selected ids, stream sets, subtitles, and danmaku XML URL.
- Intl planning uses the official intl metadata/subtitle endpoints and the signed intl OGV mobile
  playurl endpoint. It keeps mobile `video_info` `stream_list`/`dash_audio` parsing support for
  official and future proxy resolver responses.
- Review follow-up switched intl playurl away from the UGC-style web path and made intl module
  grouping reuse the same `ep_id`/`episode_id`/`id` episode alias handling as metadata conversion.
- Review follow-up also classifies intl `limit`/`status` metadata responses as access-restricted
  errors when no episode list is available, avoiding misleading `selected episode` failures.
- Review follow-up clarified that `current` selection applies to episode inputs, while `ss` and
  `md` inputs need `latest`, `all`, `episode:<epid>`, or `page:<index>`.
- Normal-video planning skips tag metadata so unrelated tag API failures do not block stream
  planning; `info` still treats tag API failures as metadata errors.
- Normal web playurl planning includes `try_look=1` so anonymous plans can report available trial
  quality tracks instead of stopping at the lowest default quality.
- Normal and PGC subtitle discovery uses the non-WBI player metadata endpoint so this slice does
  not require WBI signing support.
- Stream parsing handles DASH video/audio, Dolby/FLAC audio, legacy FLV `durl` segments, accepted
  qualities, duration, normalized protocol-relative URLs, nullable DASH lists, nullable Dolby audio,
  nullable FLV backup URLs, and real DASH tracks that contain both camelCase and snake_case URL
  fields.
- Subtitle discovery supports normal/PGC player subtitles and intl subtitle responses. Subtitle
  endpoint failures degrade to empty subtitle lists so optional subtitles do not block planning.
  Intl subtitle requests include the configured access key when present.
- `bbdown plan` prints typed JSON or a short human summary for integrations and e2e tests.
- User-facing examples use currently resolvable PGC identifiers and document that PGC/intl stream
  planning can still require eligible account or region access.

## Next Steps

- Add real file downloads, bounded retry/resume behavior, ffmpeg mux integration, and mock e2e
  download coverage.
- Keep restricted-area proxy resolver ordering as a separate slice so configured proxy policy and
  diagnostics can be reviewed independently.

## Evidence

- Planning thread: `019e9eab-43cf-7fc0-9218-cad1f8cd7819`.
- Reference demand threads: `019e9e9d-8782-7c71-a72d-1b3fbf0d6942`, `019e7fd0-b463-76f2-a5ab-37d7d77620f4`.
- PR: `https://github.com/Joey-Project/BBDown-rust/pull/2`.
- Local gate: `just ci`.

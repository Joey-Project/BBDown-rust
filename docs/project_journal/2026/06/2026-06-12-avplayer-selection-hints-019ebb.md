---
id: 20260612-019ebb
title: AVPlayer Selection Hints
status: completed
created: 2026-06-12
updated: 2026-06-12
branch: wip/playback-avplayer-hints
pr:
supersedes: []
superseded_by:
---

# AVPlayer Selection Hints

## Summary
- Added structured playback selection hints for AVPlayer-oriented clients.
- Kept the scope limited to metadata and ranking signals; downstream player/cache services still
  own playback, HLS, serving, cache retention, and ABR runtime decisions.

## Current State
- Each `PlaybackVariant` carries `selection_hints.avplayer`.
- The hint reports `format_key`, exact video/audio codec strings, codec families, `playable`,
  `preferred`, `score`, and reason codes.
- DASH H.264/AAC, HEVC/AAC, and AV1/AAC variants are all playable AVPlayer candidates; the score
  is a default ranking signal rather than a hard-coded downstream policy.
- `PlaybackCodecPreference` lets embedding clients rank variants with a custom codec order, such as
  HEVC-first or AV1-only, while validating exact codec strings before playback.
- FLV variants are not marked as AVPlayer-preferred and remain available as raw request specs.
- The CLI human summary shows `avplayer=preferred`, `avplayer=playable`, or `avplayer=avoid`.

## Next Steps
- Add ABR policy metadata and cache identity helpers so downstream services can retain fetched
  variants and segments while switching bitrate levels.
- Add app/TV playurl modes once request-spec and selection-hint surfaces are stable.

## Evidence
- Targeted validation: `cargo test -p bbdown-core playback --lib`.
- Targeted validation:
  `cargo test -p bbdown-cli --test cli_e2e playback_json_resolves_media_request_specs`.
- Local full validation: `just ci`.
- Internal review finding fixed: legacy `PlaybackVariant` JSON without `selection_hints` now
  deserializes and backfills AVPlayer hints from existing media request specs.
- Internal review finding fixed: `mp4a` codec family classification now uses a conservative AAC
  object-type allowlist (`mp4a.40.2`, `mp4a.40.5`, `mp4a.40.29`, `mp4a.40.42`), so MP3 object
  types such as `mp4a.40.34`, `mp4a.69`, and `mp4a.6b` do not become AVPlayer AAC preferred
  variants.
- API review correction: AVPlayer hints no longer treat HEVC or AV1 as unsupported; downstream
  clients can provide their own codec preference order.
- Internal review finding fixed: legacy `selection_hints.avplayer_h264_aac` payloads are ignored
  for `PlaybackVariant` backfill so old H.264-only policy metadata cannot override current exact
  codec preference hints.
- Independent review findings fixed: deprecated hint payloads are parsed as raw JSON so malformed
  old keys cannot fail before backfill, and codec preference matching now rejects
  `playable=false` hints such as FLV/AVPlayer-avoid variants.
- Internal review final: helper-backed `codex-readonly` returned `LGTM`.

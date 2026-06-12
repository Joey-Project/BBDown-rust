---
id: 20260612-019ebc
title: Playback ABR Cache Metadata
status: completed
created: 2026-06-12
updated: 2026-06-12
branch: wip/playback-abr-cache-metadata
pr:
supersedes: []
superseded_by:
---

# Playback ABR Cache Metadata

## Summary
- Added playback metadata that downstream cache/player services can use for ABR switching and disk
  cache retention without moving playback runtime responsibilities into `bbdown-core`.
- Kept the scope limited to serializable planning data; HLS sessions, HTTP serving, retention,
  cleanup, and runtime ABR policy remain downstream responsibilities.

## Current State
- `PlaybackEntry.cache_key` identifies the selected content entry.
- `PlaybackVariant.cache_key` groups the media cache keys that make up a playable variant.
- `PlaybackEntry.abr.groups` lists codec/mime-compatible DASH switching groups with level ordering
  and min/max bandwidth.
- `PlaybackVariant.abr` points to the variant's group, low-to-high level index, total level count,
  and switchability.
- Legacy playback JSON without ABR/cache metadata is deserialized by rebuilding current metadata
  from the existing media request specs.

## Next Steps
- Add additional app/TV playurl modes after the playback request-spec surface is stable.
- Add richer feed/list resolver inputs for history, following/UP pages, recommendation pages, and
  watch-later.

## Evidence
- Targeted validation: `cargo test -p bbdown-core playback --lib`.
- Targeted validation:
  `cargo test -p bbdown-cli --test cli_e2e playback_json_resolves_media_request_specs`.

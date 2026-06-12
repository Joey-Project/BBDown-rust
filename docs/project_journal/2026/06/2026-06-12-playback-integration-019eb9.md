---
id: 20260612-019eb9
title: Playback Integration Contract
status: completed
created: 2026-06-12
updated: 2026-06-12
branch: wip/playback-ladder
pr:
supersedes: []
superseded_by:
---

# Playback Integration Contract

## Summary
- Added a `PlaybackPlan` surface for downstream streaming and cache integrations.
- Kept playback planning on the same resolver path as download planning so input parsing,
  selection, restricted-area fallback, intl access, and diagnostics remain consistent.
- Documented the boundary between `bbdown-core` and downstream player/cache services.

## Current State
- `BiliClient::plan_playback` accepts raw CLI-style inputs and returns selected playback variants.
- `BiliClient::plan_playback_input` accepts a parsed `Input` and returns the same playback surface.
- `PlaybackVariant` carries DASH video/audio request specs or ordered FLV segment specs.
- `MediaRequestSpec` carries primary URL, backup URLs, media headers, mime/codec metadata,
  bandwidth, dimensions, duration, size, and a structured cache key.
- The CLI exposes the same surface through `bbdown playback`.
- The crate intentionally does not implement player task state, HLS session management, HTTP
  segment serving, AVPlayer playlist transitions, retention/cleanup, or library registration.

## Next Steps
- Add AVPlayer-oriented codec/device compatibility profiles, starting with H.264/AAC hints.
- Add ABR policy metadata and cache identity helpers so downstream services can retain fetched
  variants and segments while switching bitrate levels.
- Add app/TV playurl modes once the request-spec surface is stable.
- Add a feed/list resolver abstraction, then layer history, following/UP pages, recommendations,
  and watch-later parsing on top of it.

## Evidence
- Local targeted validation: `cargo test -p bbdown-core playback --lib`.
- Local targeted validation:
  `cargo test -p bbdown-cli --test cli_e2e playback_json_resolves_media_request_specs`.
- Local full validation: `just ci`.

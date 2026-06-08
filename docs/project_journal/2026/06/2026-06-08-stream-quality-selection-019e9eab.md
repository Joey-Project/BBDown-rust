---
id: 20260608-019e9eab-stream-quality-selection
title: Stream Quality Selection
status: completed
created: 2026-06-08
updated: 2026-06-08
branch: wip/stream-quality-selection
pr: https://github.com/Joey-Project/BBDown-rust/pull/10
supersedes: []
superseded_by:
---

# Stream Quality Selection

## Summary

- Tenth PR slice for the BBDown Rust rewrite continuation track.
- Scope is clearer stream quality discovery for users and embedders.
- The slice keeps FLV fallback behavior intact unless the caller explicitly requests a DASH stream
  id.

## Current State

- `StreamSet` exposes `qualities`, a structured list of actual selectable DASH video quality ids and
  optional labels derived from playurl `support_formats` and `accept_description`.
- `StreamSet::accept_quality` remains in the model for callers that already consume the raw Bilibili
  ids.
- Human `bbdown plan` output lists available quality ids plus video/audio stream summaries, while
  JSON output includes the same structured quality list.
- `DownloadOptions::stream_selection` lets embedding callers request exact DASH video or audio
  stream ids.
- `DownloadOptions::new` gives embedders a constructor path while the pre-1.0 settings surface stays
  non-exhaustive.
- CLI `download` exposes `--video-quality <ID>` and `--audio-quality <ID>` for the same selection
  path.
- Invalid requested ids fail before media writes and report available ids. Explicit quality
  selection rejects FLV fallback because FLV segments do not carry the DASH stream ids being
  requested.

## Evidence

- Unit coverage for unavailable requested ids and available-id diagnostics.
- Client planning coverage for `support_formats` and `accept_description` quality labels.
- CLI mock e2e coverage for human plan quality output.
- CLI mock e2e coverage that downloads the requested non-default video/audio ids.
- Regression coverage ensures raw `accept_quality` ids without returned DASH tracks are not listed
  as selectable plan qualities.
- Regression coverage verifies multi-entry requested-id failures are preflighted before creating the
  download output directory.

---
id: 20260610-019eb1-cover-download-support
title: Cover Download Support
status: completed
created: 2026-06-10
updated: 2026-06-10
branch: wip/cover-download-support
pr: https://github.com/Joey-Project/BBDown-rust/pull/21
supersedes: []
superseded_by:
---

# Cover Download Support

## Summary

- Adds cover sidecar download support to the post-`0.2.0` development line.
- Keeps download planning side-effect free while carrying cover URLs into `DownloadEntry`.
- Leaves ASS danmaku, UPOS/PCDN handling, and single-download mode for separate follow-up PRs.

## Current State

- `DownloadEntry` carries an optional `cover_url` from normal video, PGC/PUGV season, and batch
  collection planning paths, normalizing protocol-relative URLs before download execution.
- `DownloadOptions` includes `sidecars.cover`, keeps the crate default conservative, and exposes
  `with_cover(...)` for embedding callers that want cover sidecars.
- Download execution writes cover files as `DownloadFileKind::Cover` using the same retry, resume,
  timeout, media-header, and report paths as other sidecars.
- CLI `download` enables cover sidecars by default and supports `--no-cover` to skip them.
- English and Simplified Chinese user-facing docs were updated for README, crate README, user guide,
  embedding guide, and architecture guide.

## Evidence

- `cargo test -p bbdown-core --locked` passes: 145 tests.
- `cargo test -p bbdown-cli --test cli_e2e --locked` passes: 21 tests.
- `just ci` passes, including formatter, clippy, workspace tests, CLI e2e, and crate publish dry-run.
- `just live-e2e` passes: 1 ignored live manifest test executed.
- Independent Codex review and offline frozen diff review findings were addressed: re-export
  `SidecarOptions`, normalize protocol-relative cover URLs, and default missing `cover_url` during
  plan deserialization.

## Next Steps

- Open the PR, then run the normal CI and triple-review merge gate before moving to ASS danmaku,
  UPOS/PCDN, and single-download mode slices.

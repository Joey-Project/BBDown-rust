---
id: 20260618-019f0b-v0-5-downloader-embedding-roadmap
title: v0.5.0 Downloader And Embedding Roadmap
status: active
created: 2026-06-18
updated: 2026-06-18
branch: post-release/v0.5-devline
pr:
supersedes: []
superseded_by:
---

# v0.5.0 Downloader And Embedding Roadmap

## Summary

- `v0.4.0` shipped through the protected release candidate and promotion workflow as a GitHub
  Release and crates.io `bbdown-core` package.
- The next release line focuses on downloader and embedding polish instead of broad page parsing or
  credential lifecycle automation.
- The goal is to make long-running download execution easier to embed, inspect, cancel, and package
  with richer media metadata.

## Planned Slices

- PR 1: post-release housekeeping, moving workspace package versions and docs to the `0.5.0`
  development line.
- PR 2: embedder progress callbacks and a stable progress event model for download execution.
  Completed by adding `DownloadProgressEvent`, `DownloadProgressSink`, `*_with_progress` download
  APIs, and CLI `--progress-json` JSON Lines on stderr.
- PR 3: progress terminal events, clearer CLI `--progress-json` schema/sample docs, and
  `DownloadReport` summary helpers for downstream UI state. Scope is limited to failure events plus
  the currently reachable plan-level archive duplicate cancellation event; cancellable execution
  remains PR 4.
- PR 4: cancellation-aware download execution so embedders and CLI flows can stop work without
  corrupting completed artifacts, using the progress terminal event model from PR 3.
- PR 5: chapter metadata mux support where the selected media source provides usable chapter data
  and the mux backend can carry it.
- PR 6: audio language selection in API and CLI surfaces, including listing enough source metadata
  for callers to make their own choice.
- PR 7: AI subtitle filtering in API and CLI surfaces, keeping raw subtitle metadata visible while
  allowing callers to prefer or exclude AI-generated subtitles.
- PR 8: `v0.5.0` release prep, release notes, full CI/live-e2e validation, and protected RC
  creation.

## Out Of Scope For This Line

- Automatic credential refresh, credential health policies, profile-level lifecycle status, and
  multi-account lifecycle UX remain planned for `v0.6.0`.
- Per-video related recommendations and additional Bilibili page-family parsing remain planned for
  `v0.7.0` or a later feed/page release.
- Remaining BBDown parity items such as aria2 or multi-thread download integration, MP4Box muxing,
  and subtitle-to-SRT conversion stay available for reprioritization.

## Evidence

- GitHub Release `v0.4.0`: `https://github.com/Joey-Project/BBDown-rust/releases/tag/v0.4.0`.
- Promotion workflow: `https://github.com/Joey-Project/BBDown-rust/actions/runs/27769537947`.
- Published crate version: `bbdown-core` `0.4.0`.
- Progress callback slice validation covers core callback events and CLI `--progress-json` mock e2e
  output.

---
id: 20260618-019f0b-v0-5-downloader-embedding-roadmap
title: v0.5.0 Downloader And Embedding Roadmap
status: active
created: 2026-06-18
updated: 2026-06-19
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
  `DownloadReport` summary helpers for downstream UI state. Completed with failure/cancelled
  terminal events, clearer schema docs, embedding progress guidance, and report summary helpers.
- PR 4: cancellation-aware download execution so embedders and CLI flows can stop work without
  corrupting completed artifacts, using the progress terminal event model from PR 3. Completed by
  adding `DownloadCancellationToken`, cancellable download API variants, CLI `Ctrl-C` cancellation,
  `Error::Cancelled`, partial-file rollback, and cancellation-focused unit/e2e coverage.
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
- Progress terminal/report summary validation covers terminal failure events, archive duplicate
  cancellation progress, CLI schema samples, and `DownloadReport::summary()`.
- Cancellation validation covers pre-start plan cancellation, mid-file rollback/removal, completed
  entry counts after cancellation, and CLI SIGINT `--progress-json` `plan_cancelled` output.
- Cancellation hardening validation covers cancellation reason publication before terminal
  notification, archive duplicate prompt Ctrl-C force-exit decisions, post-ffmpeg mux temporary
  cleanup on late cancellation, and matching CLI/user-facing documentation.
- Local cancellation hardening gate passed with project journal validation and `just ci`, covering
  fmt, clippy, pinned toolchain check, workspace tests, CLI e2e, and `bbdown-core` publish dry-run.
- Independent cancellation review follow-up covers prompt-mode SIGINT before duplicate preflight
  output, CLI token-cancellation exit code `130`, and embedding docs that distinguish token-driven
  cancellation from explicit archive duplicate cancel decisions. The follow-up gate passed targeted
  CLI SIGINT/duplicate tests, clippy, project journal validation, and `just ci`.
- Offline frozen diff review found that default retry cancellation could emit duplicate
  `FileFailed` progress events. The fix short-circuits cancelled file attempts before retry/backoff
  handling and adds default-retry coverage that asserts one `FileFailed` event.
- Independent review found the same duplicate `FileFailed` risk when cancellation happens during a
  non-zero retry backoff after a retryable file failure. The fix suppresses terminal cancellation
  `FileFailed` for file attempts that never started and adds explicit backoff-cancellation coverage.

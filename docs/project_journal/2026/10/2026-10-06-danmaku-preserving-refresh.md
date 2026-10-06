---
id: 20261006-danmaku-preserving-refresh
title: Strictly Preserving Danmaku Refresh
status: completed
created: 2026-10-06
updated: 2026-10-06
branch:
pr:
supersedes: []
superseded_by:
---

# Strictly Preserving Danmaku Refresh

## Summary

- Add opt-in preserving refresh for existing archive-backed danmaku files while keeping the
  default legacy update behavior and JSON report unchanged.
- The implementation is complete for the `0.8.0` source line. Actual publication remains a
  separate protected post-merge release workflow.

## Current State

- The existing workflow remains documented in
  `docs/project_journal/2026/06/2026-06-18-danmaku-append-update-019f0a.md` and remains the CLI
  default. The new `Preserve` policy stages XML, optional ASS, and archive JSON before grouped
  publication. The core exposes staged outputs and a group publisher with content/destination
  revalidation, detected-error rollback, and recovery-location reporting.
- The request was transferred from the Telegram-Video-Downloader task. Its bot-side consumer pinned
  BBDown-rust revision `0a94b071bbc1897ec1d1fec9dfcf7883c5754a15`; its dependency update remains a
  downstream follow-up.
- Responsibility boundary: BBDown core owns content parsing/merging and grouped publication of
  staged sidecars plus archive. The bot owns historical-task buttons, whole-directory selection,
  queueing and progress, and coordination with File Provider materialization and concurrent external
  writers; core content checks do not lock those external writers.
- The complete local gate passed: formatting, workspace all-target Clippy, Rust 1.95.0 workspace
  check, workspace tests (754 passed, 3 ignored), CLI e2e repeat (153 passed), and
  `cargo publish --dry-run -p bbdown-core --locked --allow-dirty` (29 files packaged; dry-run did
  not upload).
  Test suites included CLI unit (67), CLI e2e (153), live e2e (9 passed, 2 ignored), core library
  (521), CDN benchmark (3 passed, 1 ignored), public API (1), and doc tests (0).
- An initial full gate passed on Rust 1.95.0. After the separate `question_mark` lint correction, a
  CI-matching rerun also passed: `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0 in 105.82 seconds
  with rustc 1.99.0 (`b940084d7`), cargo 1.99.0, and Clippy 0.1.99. It included formatting,
  all-target Clippy, the explicit Rust 1.95.0 MSRV check, 754 workspace tests passed / 3 ignored,
  a separate CLI e2e repeat (153 passed), and the publish dry-run above.

## Next Steps

- Coordinate the downstream bot dependency update and bot-side workflow as a separate workstream.
- Publish the `0.8.0` source line through the protected RC and promotion workflow after merge.

## Evidence

- Existing implementation record: `docs/project_journal/2026/06/2026-06-18-danmaku-append-update-019f0a.md`.
- Release preparation: `docs/project_journal/2026/10/2026-10-06-v0-8-release-prep.md`.
- Downstream bot dependency pin: `0a94b071bbc1897ec1d1fec9dfcf7883c5754a15`.
- Local delivery evidence: full `just ci` passed; workspace suite counts are recorded above.

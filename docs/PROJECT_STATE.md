# Project State

## Current State

- The Rust BBDown rewrite has a crate/CLI/CI foundation plus typed metadata and download-planning
  APIs.
- Latest completed workstreams:
  - `docs/project_journal/2026/06/2026-06-06-rust-rewrite-foundation-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-07-stream-planning-019e9eab.md`.
- Next planned workstream: file download execution, retry/resume policy, ffmpeg mux integration,
  and mock e2e download coverage.

## Recovery Pointers

- Run `just ci` for the local default gate after dependencies are restored.
- Workstream detail and PR-local state should live under `docs/project_journal/`.
- User-facing CLI behavior is documented in `docs/user-guide.md`.

## Global Blockers

- None currently recorded.

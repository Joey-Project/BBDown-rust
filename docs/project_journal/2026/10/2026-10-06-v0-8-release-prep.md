---
id: 20261006-v0-8-release-prep
title: v0.8.0 Release Preparation
status: completed
created: 2026-10-06
updated: 2026-10-06
branch:
pr:
supersedes: []
superseded_by:
---

# v0.8.0 Release Preparation

## Summary

- Prepare the `0.8.0` workspace package versions and bilingual source-line notes for preserving
  danmaku refresh. Preparation is complete; the protected RC and promotion workflows remain the
  publication path after the feature PR lands.

## Current State

- `bbdown-core`, `bbdown-cli`, the CLI's local core dependency, and `Cargo.lock` are set to `0.8.0`.
- Release notes and indexes describe opt-in preserving refresh, staged publication, ASS-only
  baseline ambiguity, caller coordination, detected-error recovery, and the lack of a multi-file
  crash/power-loss atomicity promise.
- The full `just ci` gate passed: formatting, workspace all-target Clippy, Rust 1.95.0 workspace
  check, workspace tests (754 passed, 3 ignored), CLI e2e repeat (153 passed), and
  `cargo publish --dry-run -p bbdown-core --locked --allow-dirty` (29 files packaged; no upload occurred).
- No `v0.8.0` GitHub Release or crates.io publication is claimed. Those remain pending until the
  feature PR merges and the protected RC/promotion workflows run.

## Publication Follow-up

- After merge, create and promote the protected release candidate, publish the release, and record
  the actual tag and package evidence here.

## Evidence

- Feature workstream: `docs/project_journal/2026/10/2026-10-06-danmaku-preserving-refresh.md`.
- Bilingual notes: `docs/release-notes/v0.8.0.md` and `docs/release-notes/v0.8.0.zh-CN.md`.
- Gate evidence: full `just ci` passed; workspace suite counts are recorded above.

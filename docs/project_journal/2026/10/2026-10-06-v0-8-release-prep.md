---
id: 20261006-v0-8-release-prep
title: v0.8.0 Release Preparation
status: completed
created: 2026-10-06
updated: 2026-10-06
branch:
pr: 87
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
  `cargo publish --dry-run -p bbdown-core --locked --allow-dirty` (29 files packaged; no upload
  occurred).
- The initial full gate passed on Rust 1.95.0. After the separate `question_mark` lint correction,
  the CI-matching rerun `env RUSTUP_TOOLCHAIN=1.99.0 just ci` also exited 0 in 105.82 seconds with
  rustc 1.99.0 (`b940084d7`), cargo 1.99.0, and Clippy 0.1.99. The rerun included formatting,
  all-target Clippy, the explicit Rust 1.95.0 MSRV check, 754 workspace tests passed / 3 ignored, a
  separate CLI e2e repeat (153 passed), and the publish dry-run above.
- PR [#87](https://github.com/Joey-Project/BBDown-rust/pull/87) follow-up fixes addressed quoted
  XML root delimiters, ASS style insertion at the true styles header with custom comments, and
  revalidation through canonical parent-directory symlink targets; targeted pure tests passed (19)
  and targeted publisher tests passed (10).
- The final CI-matching gate `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0 in 42.5 seconds with
  rustc 1.99.0 (`b940084d7`), cargo 1.99.0, and Clippy 0.1.99. It passed formatting, all-target
  Clippy, the explicit Rust 1.95.0 MSRV check, and workspace tests (759 passed, 3 ignored), with
  separate CLI e2e repeat (153 passed). The publish dry-run verified 29 files and performed no
  upload.
- No `v0.8.0` GitHub Release or crates.io publication is claimed. Those remain pending until the
  feature PR merges and the protected RC/promotion workflows run.

## Publication Follow-up

- After merge, create and promote the protected release candidate, publish the release, and record
  the actual tag and package evidence here.

## Evidence

- Feature workstream: `docs/project_journal/2026/10/2026-10-06-danmaku-preserving-refresh.md`.
- Feature PR: [#87](https://github.com/Joey-Project/BBDown-rust/pull/87).
- Bilingual notes: `docs/release-notes/v0.8.0.md` and `docs/release-notes/v0.8.0.zh-CN.md`.
- Gate evidence: full `just ci` passed; workspace suite counts are recorded above.

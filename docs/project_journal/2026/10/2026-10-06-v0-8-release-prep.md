---
id: 20261006-v0-8-release-prep
title: v0.8.0 Release Preparation
status: completed
created: 2026-10-06
updated: 2026-10-08
branch:
pr: 88
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
- Release notes describe opt-in preserving refresh, positive comment-ID-first XML deduplication
  with complete-parameter-and-decoded-text fallback for invalid IDs, and full ASS regeneration
  from merged XML. When no XML baseline exists, fetched XML initializes generated files; ASS-only
  historical events are not backfilled. They also describe staged publication, caller coordination,
  detected-error recovery, and the lack of a multi-file crash/power-loss atomicity promise.
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
- A subsequent PR #87 documentation review caught an `.await` on synchronous
  `StagedDanmakuUpdate::publish`; both embedding examples now use `staged.publish()?`, matching the
  API signature. The existing publisher tests already compile and exercise this call.
- The final CI-matching gate `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0 in 42.5 seconds with
  rustc 1.99.0 (`b940084d7`), cargo 1.99.0, and Clippy 0.1.99. It passed formatting, all-target
  Clippy, the explicit Rust 1.95.0 MSRV check, and workspace tests (759 passed, 3 ignored), with
  separate CLI e2e repeat (153 passed). The publish dry-run verified 29 files and performed no
  upload.
- Final PR #87 follow-ups cover qualified ordinary and self-closing XML roots with and without
  default namespaces; every appended node is checked against the namespace at its actual insertion
  offset, with regressions for comment/CDATA shadows and later incompatible appends. Both embedding
  guides use the synchronous `staged.publish()?` API call.
- `cargo +1.99.0 test -p bbdown-core --lib danmaku::preserving::tests --locked` passed (12/12),
  and strict core Clippy passed.
- The final `env RUSTUP_TOOLCHAIN=1.99.0 just ci` gate exited 0 in 46.72 seconds. Formatting,
  all-target strict Clippy, and the explicit Rust 1.95.0 MSRV check passed; workspace suites passed
  761 tests with 3 ignored and 0 failed: CLI unit (67), CLI e2e (153), live e2e (9 passed, 2
  ignored), core library (528), CDN benchmark (3 passed, 1 ignored), and public API (1). A separate
  CLI e2e repeat passed all 153 tests. The publish dry-run verified 29 files and uploaded nothing.
- After separating the release-only version and documentation changes, the local release-prep tree
  passed `env RUSTUP_TOOLCHAIN=1.99.0 just ci` on 2026-10-08 (exit 0, 29.69 seconds). Formatting,
  all-target Clippy, and the Rust 1.95.0 MSRV check passed. Workspace suites passed 761 tests with
  3 ignored and 0 failed; the separate CLI e2e repeat passed 153 tests. The `bbdown-core 0.8.0`
  publish dry-run verified 29 packaged files and uploaded nothing.
- The release tree now includes the feature-side single-decode fix and its actual-renderer ASS-only
  regression. Its comparison against the feature branch remains limited to 14 release-only files.
  At release gate HEAD `9929ba2`, `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0 in 44.44 seconds
  (`/private/tmp/bbdown-danmaku-preserve-check.20261008/release-entity-ci.log`, 75,412 bytes).
  Formatting, strict all-target Clippy, and the explicit Rust 1.95.0 MSRV check passed. Workspace
  suites passed 763 tests with 3 ignored and 0 failed: CLI unit (67), CLI e2e (154), live e2e (9
  passed, 2 ignored), core (529), CDN benchmark (3 passed, 1 ignored), and public API (1). The
  separate CLI e2e repeat passed 154 tests. The `bbdown-core 0.8.0` publish dry-run verified 29
  packaged files and uploaded nothing.
- At release-prep validation HEAD `504b31c2eccff8c3d87c660be087d4952b2f1004`, based on feature
  source `d1bdc4ef`, `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0 in 56.00 seconds
  (`/private/tmp/bbdown-danmaku-id-rebuild.20261008/release-ci.log`, 75,494 bytes). Formatting,
  strict workspace Clippy, and the Rust 1.95.0 MSRV check passed. Workspace tests passed 763 with 3
  ignored and 0 failed; an independent CLI e2e repeat passed 154 tests and is not included in that
  workspace total. The `bbdown-core 0.8.0` publish dry-run verified 29 files and uploaded nothing.
  The release-only comparison remains 14 paths. This gate records source-tree validation only; it
  does not claim the feature PR was merged or the `0.8.0` release was published.
- The release-prep working tree was synchronized with feature head
  `0a2802cd32d9d815835cbe475017ca7a4cf2e93c`; its comparison against that feature tree remained
  exactly 14 release-only files. On this staged tree, `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0
  in 49.15 seconds (`/private/tmp/bbdown-pr87-findings.20261008/release-ci.log`, 75,974 bytes).
  Formatting, strict workspace Clippy, and the Rust 1.95 MSRV check passed. Workspace suites passed
  768 tests, with 3 ignored and 0 failed; the independent CLI e2e repeat passed 154 tests and is
  not added to the workspace total. The `bbdown-core 0.8.0` publish dry-run verified 29 files
  (1.6 MiB uncompressed, 246.6 KiB compressed) and uploaded nothing; the run reported the existing
  yanked `spin 0.9.8` dependency. This records staged source-tree validation, not the later
  journal-only commit, a merged PR, or a published release.
- No `v0.8.0` GitHub Release or crates.io publication is claimed. Those remain pending until the
  feature PR merges and the protected RC/promotion workflows run.

## Publication Follow-up

- After merge, create and promote the protected release candidate, publish the release, and record
  the actual tag and package evidence here.

## Evidence

- Feature workstream: `docs/project_journal/2026/10/2026-10-06-danmaku-preserving-refresh.md`.
- Feature dependency: PR [#87](https://github.com/Joey-Project/BBDown-rust/pull/87).
- Release-prep PR: [#88](https://github.com/Joey-Project/BBDown-rust/pull/88).
- Bilingual notes: `docs/release-notes/v0.8.0.md` and `docs/release-notes/v0.8.0.zh-CN.md`.
- Gate evidence: full `just ci` passed; workspace suite counts are recorded above.

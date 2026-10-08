---
id: 20261006-danmaku-preserving-refresh
title: Strictly Preserving Danmaku Refresh
status: completed
created: 2026-10-06
updated: 2026-10-08
branch:
pr: 87
supersedes: []
superseded_by:
---

# Strictly Preserving Danmaku Refresh

## Summary

- Add opt-in preserving refresh for existing archive-backed danmaku files while keeping the
  default legacy update behavior and JSON report unchanged.
- The implementation is complete in feature PR #87.

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
  check, workspace tests (754 passed, 3 ignored), and CLI e2e repeat (153 passed).
  Test suites included CLI unit (67), CLI e2e (153), live e2e (9 passed, 2 ignored), core library
  (521), CDN benchmark (3 passed, 1 ignored), public API (1), and doc tests (0).
- An initial full gate passed on Rust 1.95.0. After the separate `question_mark` lint correction, a
  CI-matching rerun also passed: `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0 in 105.82 seconds
  with rustc 1.99.0 (`b940084d7`), cargo 1.99.0, and Clippy 0.1.99. It included formatting,
  all-target Clippy, the explicit Rust 1.95.0 MSRV check, 754 workspace tests passed / 3 ignored,
  and a separate CLI e2e repeat (153 passed).
- PR [#87](https://github.com/Joey-Project/BBDown-rust/pull/87) follow-up fixes cover XML root
  insertion around quoted `>` and `/>`, ASS style insertion at the actual styles header while
  retaining custom comments, and canonical parent-directory symlink target revalidation. The
  symlink regressions cover equal content bytes, both snapshots missing, and stable logical aliases.
- A later PR #87 documentation review found that the embedding examples awaited synchronous
  `StagedDanmakuUpdate::publish`; the English and Chinese examples now call `staged.publish()?`.
  Existing core publisher tests compile and exercise the synchronous call.
- After those fixes, targeted pure tests passed (19) and targeted publisher tests passed (10). The
  final CI-matching gate `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0 in 42.5 seconds with rustc
  1.99.0 (`b940084d7`), cargo 1.99.0, and Clippy 0.1.99. It passed formatting, all-target Clippy,
  the explicit Rust 1.95.0 MSRV check, and workspace tests (759 passed, 3 ignored): CLI unit (67),
  CLI e2e (153), live e2e (9 passed, 2 ignored), core library (526), CDN benchmark (3 passed, 1
  ignored), and public API (1). The separate CLI e2e repeat passed all 153 tests.
- Final PR #87 follow-ups fix qualified XML root handling for ordinary and self-closing roots with
  and without default namespaces, and validate each appended node's namespace at its actual output
  offset. Regression cases cover comment/CDATA namespace shadows and a later append with an
  incompatible namespace. The English and Chinese embedding examples also use the synchronous
  `staged.publish()?` call.
- A 2026-10-08 follow-up found a double-decoding bug in preservation matching: `roxmltree` had
  already decoded XML text, but the old-ASS parsing path passed that text through `xml_unescape`
  again. This could miss an existing old-ASS-only event containing a literal entity. The fix routes
  already-decoded text directly to the renderer through a small private helper; the raw-XML path
  still decodes once.
- Focused regressions reproduced the issue before the fix and passed afterward. The core RED case
  expected one append but produced three (`preserve-red.log`, exit 101); after the fix, the
  preserving core tests passed (13 passed). The CLI RED case used actual `xml_to_ass` output as the
  old ASS, removed the XML baseline from disk and archive, then expected one append but produced
  two (`entity-red3.log`, exit 101). After the fix, that regression passed (1 passed), the preserving
  CLI group passed (7 passed, 147 filtered), and a second run appended zero events while leaving the
  ASS bytes identical. At that checkpoint, the complete feature gate for this follow-up had not yet
  been reported.
- The follow-up's complete gate subsequently passed: `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0
  in 45.02 seconds (`/private/tmp/bbdown-danmaku-preserve-check.20261008/feature-entity-ci-green.log`,
  75,383 bytes). Formatting, strict all-target Clippy, and the Rust 1.95 MSRV check passed. The
  workspace had 763 passed, 3 ignored, and 0 failed: CLI unit (67), CLI e2e (154), live e2e (9
  passed, 2 ignored), core (529), CDN benchmark (3 passed, 1 ignored), and public API (1). A separate
  CLI e2e repeat passed 154 tests; this is independent of, and not added to, the workspace total.
  The `bbdown-core` 0.7.0 publish dry-run verified 29 packaged files and uploaded nothing. The
  broader danmaku suite passed 36 tests, and formatting checks passed. The new test's strict Clippy
  `expect_used` finding was corrected before this successful full gate.
- These regression inputs are synthetic/mock; no new live Bilibili two-time comparison was run.
  XML identity remains the full `p` attribute plus decoded text, so a source rewrite of `p` may be
  treated as a new item. ASS-only input has no original comment IDs; its fallback identity is an
  approximation based on time and text plus recognized font, color, and mode.
- `cargo +1.99.0 test -p bbdown-core --lib danmaku::preserving::tests --locked` passed (12 passed,
  0 failed); strict core Clippy passed. The final
  `env RUSTUP_TOOLCHAIN=1.99.0 just ci` gate exited 0 in 46.72 seconds with formatting,
  all-target strict Clippy, and the explicit Rust 1.95.0 MSRV check. Workspace suites passed 761
  tests with 3 ignored and 0 failed: CLI unit (67), CLI e2e (153), live e2e (9 passed, 2 ignored),
  core library (528), CDN benchmark (3 passed, 1 ignored), and public API (1). A separate CLI e2e
  repeat passed 153 tests.

## Next Steps

- Coordinate the downstream bot dependency update and bot-side workflow as a separate workstream.

## Evidence

- Existing implementation record: `docs/project_journal/2026/06/2026-06-18-danmaku-append-update-019f0a.md`.
- Feature PR: [#87](https://github.com/Joey-Project/BBDown-rust/pull/87).
- Downstream bot dependency pin: `0a94b071bbc1897ec1d1fec9dfcf7883c5754a15`.
- Local delivery evidence: full `just ci` passed; workspace suite counts are recorded above.
- After the release-only split, the feature tree retained `bbdown-core` and `bbdown-cli` at
  `0.7.0`; release-version and release-documentation changes were separated into the release patch.
  On 2026-10-08, `env RUSTUP_TOOLCHAIN=1.99.0 just ci` passed in 96.93 seconds with formatting,
  all-target strict Clippy, and the explicit Rust 1.95.0 MSRV check. Workspace suites passed 761
  tests with 3 ignored and 0 failed: CLI unit (67), CLI e2e (153), live e2e (9 passed, 2 ignored),
  core library (528), CDN benchmark (3 passed, 1 ignored), and public API (1). The separate CLI e2e
  repeat passed 153 tests. The core `0.7.0` publish dry-run verified 29 packaged files and uploaded
  nothing; the existing `0.7.0` registry warning was expected.

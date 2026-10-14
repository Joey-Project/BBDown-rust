---
id: 20261011-v0-10-release-prep
title: v0.10.0 Release Preparation
status: active
created: 2026-10-11
updated: 2026-10-11
branch: wip/release-v0.10.0
pr:
supersedes: []
superseded_by:
---

# v0.10.0 Release Preparation

## Summary

- Prepare `bbdown-core` and `bbdown-cli` version `0.10.0`, the CLI's path-constrained core
  dependency, and the two local package lock records without changing third-party resolution.
- Add bilingual release notes and index entries for single-video related recommendations and
  the reproducible CDN measurement/test baseline. Synchronize root/crate README, user, and
  embedding guidance while retaining `0.9.0` as the latest published version.
- Refresh the stable project state and TODO entrypoints. Correct both download flow diagrams
  to allow parallel Range transfer with one compatible host, and cross-host transfer with
  multiple compatible hosts.
- Synchronize the release runbooks with the current `master` required-check configuration:
  `Rust` and `codex/github-review-gate`, with Code Owner review. Do not require v2 on unsupported
  fork-head or `release/*` PRs.
- Record the separate pending [Space dynamic request and bounded metadata API follow-ups](2026-10-11-space-dynamic-request-and-bounded-metadata-followup.md).
  The completed WBI signing workstream remains completed.

## Scope And Limits

- PR [#96](https://github.com/Joey-Project/BBDown-rust/pull/96) merged at
  `76d555bca479a9876035e080bc02d10ea5e68a33`. Related recommendations preserve upstream order,
  filter unsupported rows, deduplicate aids, and reuse selection/planning/download behavior;
  they return one finite batch without pagination or recursive traversal.
- PR [#97](https://github.com/Joey-Project/BBDown-rust/pull/97) merged at
  `e971ee5f15826ce455fd4642e1b886060876af49`. The measurement harness and deterministic tests
  change no production CDN API or routing policy. Consumed response-body counters are not total
  wire traffic, sample compatibility is not whole-representation identity, and output hashes
  verify the produced samples rather than every CDN edge.
- Automatic routing, persistent route health, adaptive sizing/concurrency, additional page
  families, and the new metadata-only API remain follow-ups. Parallel new CDN measurements are
  not part of this release-preparation record.
- Consumer-supplied initial dynamic HTTP 412 evidence has not been reproduced by this thread;
  the cause is undetermined and the consumer-focused dynamic live gate has not passed. This
  does not invalidate deterministic release verification or establish failure for every fixture.
- This preparation does not create a tag, start RC/promotion workflows, or publish a GitHub
  Release/crate. Publication requires a separate approved step after the preparation PR lands.

## Source Validation

- The source-feature journals record their original gates. The combined `master` source CI
  [38168314709](https://github.com/Joey-Project/BBDown-rust/actions/runs/38168314709) completed
  successfully at `e971ee5f15826ce455fd4642e1b886060876af49`. These are source validation
  records, not validation of the version change or publication.

## Release Preparation Validation

- This workstream is on hold at Joey's request until the Space dynamic initial HTTP 412 and
  bounded metadata-only API fixes land. The version and documentation edits remain uncommitted
  on `wip/release-v0.10.0`; this thread has not created a PR or started publication.
- Lightweight checks passed: `git diff --check`, project journal validation, local manifest/version metadata using
  `cargo metadata --locked --offline --no-deps`, and the validation runner's Python syntax check.
  Full offline dependency metadata could not complete because `anstyle-wincon 3.0.11` was absent
  from the local cache. This does not establish a lockfile inconsistency.
- The bounded `just ci` runner is prepared but has not been started. No release-preparation build,
  test suite, package dry run, remote review, tag, RC, or promotion ran in this thread.

## Resume Checkpoint

- Preserve the checkout at `/Users/joey/.codex/worktrees/4268/BBDown-rust`, branch
  `wip/release-v0.10.0`, and starting base `e971ee5f15826ce455fd4642e1b886060876af49`.
- Preserve the task-scoped validation runner and recovery materials under
  `/private/tmp/bbdown-v010-prep-01a12c87`. No Cargo build was started by the prepared runner.
- After the fix PRs merge and the coordinating task authorizes resumption, update the base,
  incorporate the implemented APIs and acceptance evidence into both language versions, and run
  the necessary release-preparation gate before creating the independent PR.
- The latest published version remains `0.9.0`. Release notes are a draft source-preparation
  record and must be refreshed after the fixes; they do not announce publication.

## Publication Handoff

- After the preparation PR is approved and merged, create an RC from the merged `master` with
  version input `0.10.0`, approve `release-candidate`, and verify the RC source/artifacts.
- Promote the selected latest RC using version input `0.10.0`, then obtain the separate
  `production-release` and `crates-io` approvals. Verify final source, asset checksums, and crate
  publication before changing documentation from prepared source to published release.

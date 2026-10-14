---
id: 20261011-v0-10-release-prep
title: v0.10.0 Release Preparation
status: completed
created: 2026-10-11
updated: 2026-10-14
branch: wip/release-v0.10.0
pr: https://github.com/Joey-Project/BBDown-rust/pull/101
supersedes: []
superseded_by:
---

# v0.10.0 Release Preparation

## Summary

- Prepare `bbdown-core` and `bbdown-cli` version `0.10.0`, the CLI's path-constrained core
  dependency, and the two local package lock records without changing third-party resolution.
- Add bilingual release notes and index entries for single-video related recommendations,
  reproducible and expanded CDN measurements, bounded dynamic metadata, and device-cookie
  initialization. Synchronize root/crate README, user, and
  embedding guidance while retaining `0.9.0` as the latest published version.
- Refresh the stable project state and TODO entrypoints. Correct both download flow diagrams
  to allow parallel Range transfer with one compatible host, and cross-host transfer with
  multiple compatible hosts.
- Synchronize the release runbooks with the current `master` required-check configuration:
  `Rust` and `codex/github-review-gate`, with Code Owner review. Document the maintainer integration
  handoff for fork contributions; `release/*` is not a PR target and has no v2 requirement.
- Synchronize the [dynamic follow-up record](2026-10-11-space-dynamic-request-and-bounded-metadata-followup.md)
  with the implemented #99/#100 scope while keeping consumer V2 acceptance separate. The completed
  WBI signing workstream remains completed.
- Incorporate the explicitly assigned page-family planning documents from the coordinator's
  checkout. Their priorities and resolver capability boundaries remain pending roadmap work.

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
- PR [#98](https://github.com/Joey-Project/BBDown-rust/pull/98) added two bounded benchmark
  invocations and six valid downloads. Sequential transfer was faster in these two samples.
  Harness budget/representation controls and bilingual routing proposals do not implement
  automatic routing, persistent host health, or adaptive chunk/concurrency policy.
- PR [#99](https://github.com/Joey-Project/BBDown-rust/pull/99) added explicit bounded metadata
  pages only for Following and SpaceDynamic, preserving full `resolve`/`info` defaults. PR
  [#100](https://github.com/Joey-Project/BBDown-rust/pull/100) added in-memory missing/empty device
  cookie initialization, context-bound cache reuse, and accurate request accounting.
- The repair thread verified actual-core metadata plus continuation: five requests returned two
  video metadata items. Consumer V2 planning/focused smoke, download, HLS, and native playback
  still have no successful acceptance evidence. This release-preparation thread ran no new live
  requests and did not reuse the repair thread's credentials.
- Additional page families and broader bounded metadata support remain future work.
- This preparation does not create a tag, start RC/promotion workflows, or publish a GitHub
  Release/crate. Publication requires a separate approved step after the preparation PR lands.

## Source Validation

- The source-feature journals record their original gates. The preparation resumed from merged
  `master` `58038b3032067b82e074b7cf813f87d569be8fa5`, including #96–#100. Their historical tests
  and live observations do not validate the release-preparation version change or publication.

## Review Corrections

- Make packaged README and guide content describe versioned API capabilities without freezing an
  unpublished/latest-version claim into immutable artifacts. Date preparation-status records and
  link current release status; synchronize both languages.
- Synchronize both packaged architecture guides with the versioned API boundary and the 0.10
  additions, including explicit bounded metadata and in-memory device-cookie initialization.
- Name the actual cookie configuration API in both release notes, and link consumer follow-up
  guidance to its dedicated record.
- Use repository-hosted links for documents absent from binary archives; keep links between
  bundled release notes local and synchronize language-specific destinations.
- Explain that target-branch required checks also apply to fork-head PRs. Preserve `master`
  protections and document reviewed integration through a same-repository PR; no ruleset changed.

## Release Preparation Validation

- The initial pre-review bounded `just ci` gate passed with Rust/Cargo 1.95.0 on 2026-10-14: formatting, strict
  workspace Clippy, MSRV installation/check, workspace tests, CLI e2e, and core package dry run.
  Workspace tests: 858 passed, 3 ignored, 0 failed. The separate CLI e2e step: 159 passed,
  0 ignored, 0 failed. No live/Bilibili tests ran.
- The recorded producer exit was `0`; elapsed time was 31.450 seconds using the isolated warm
  build cache. Whole-input and code/package digests were unchanged during the successful gate.
- The first gate's producer log reached package dry-run completion, but a temporary runner's
  summary error prevented persistence of its exit code. It is not counted as a passed gate.
  The runner was repaired and checked against that saved log before the successful rerun;
  no repository source changed between those attempts.
- The review-correction `just ci` rerun passed all seven stages with producer exit `0` in 38.024
  seconds. Workspace tests: 858 passed, 3 ignored, 0 failed. The separate CLI e2e step: 159 passed,
  0 ignored, 0 failed. Whole-input and code/package digests remained stable during that gate;
  no live/Bilibili tests ran.
- After synchronizing the packaged architecture guides, another `just ci` rerun passed all seven
  stages with producer exit `0` in 36.002 seconds and the same test counts. Its whole-input digest,
  including `docs/`, remained stable; code/core-package inputs and both architecture-guide content
  digests were rechecked before the signed commit. No live/Bilibili tests ran.
- Documentation checks and code/package digest revalidation accompany the validation-journal
  update before the signed correction commit. No tag, RC, promotion, or publication is authorized
  by this workstream.

## Publication Handoff

- After the preparation PR is approved and merged, create an RC from the merged `master` with
  version input `0.10.0`, approve `release-candidate`, and verify the RC source/artifacts.
- Promote the selected latest RC using version input `0.10.0`, then obtain the separate
  `production-release` and `crates-io` approvals. Verify final source, asset checksums, and crate
  publication before changing documentation from prepared source to published release.

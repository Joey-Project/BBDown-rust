---
id: 20261010-v0-9-release-prep
title: v0.9.0 Release Preparation
status: completed
created: 2026-10-10
updated: 2026-10-10
branch: wip/release-v0.9.0
pr:
supersedes: []
superseded_by:
---

# v0.9.0 Release Preparation

## Summary

- Prepared the bilingual source-line notes, release-note index entries, and current-version
  summaries for the four English and Simplified Chinese root/crate READMEs.
- Prepared the `0.9.0` versions for both workspace crates, kept the CLI core dependency
  path-constrained, and updated the two workspace lock records; third-party dependency resolution
  remained unchanged.
- The prepared `0.9.0` scope covers structured CDN transfer diagnostics and WBI-signed
  personal-space dynamic feed requests. The previously published `0.8.0` notes remain intact as
  historical documentation.
- This journal records version and documentation preparation only. It does not record publication
  of `v0.9.0` or `bbdown-core 0.9.0`.

## Scope And Limits

- Transfer-byte events describe response-body bytes consumed by the application, not bytes written
  or published and not total wire traffic. Candidate selection remains based on existing
  sample/size/path-query compatibility preflight; it does not prove full-representation identity.
- Production shard chunks remain fixed at 1 MiB. No adaptive sizing, automatic routing, persistent
  route health, suffix resume, hedging, or performance claim is included.
- Space dynamic feed requests sign the initial and continuation query with WBI, including `offset`;
  the existing Following feed behavior is preserved. Dynamic metadata rows do not establish
  playback or download acceptance.

## Source Validation Evidence

- PR #92's completed journal records full CI success with 808 workspace tests passed and 3 ignored,
  a separate CLI repeat of 154 passed, and a successful package dry run with no upload. PR #92
  merged at `18e614f724b848c734bc7f7b1ec07255b69827aa`.
- PR [#93](https://github.com/Joey-Project/BBDown-rust/pull/93) merged on 2026-10-10 at
  `9e9abe5589f9fcb6afcf45f94d7d7205afe84970`. Its reported CI3 passed with 811 workspace tests
  passed and 3 ignored,
  a separate CLI repeat of 154 passed, formatting, strict all-target Clippy, the Rust 1.95 locked
  workspace check, and package dry run. These are source-feature validation records, not a release
  publication result.

## Release Preparation Validation

- On `wip/release-v0.9.0`, `env RUSTUP_TOOLCHAIN=1.99.0 just ci` passed on attempt 1 (exit 0,
  83.23 seconds, 80,125 bytes). Formatting, strict all-target Clippy, and the locked workspace
  check with Rust 1.95 passed. Workspace tests reported 811 passed, 0 failed, and 3 ignored; the
  separate CLI repeat reported 154 passed, 0 failed, and 0 ignored.
- The package dry run passed with 29 files (1.8 MiB; 274.6 KiB compressed). Upload was aborted;
  no package was uploaded. The existing yanked `spin 0.9.8` dependency produced the only warning.
- The post-run receipt confirmed all 31 tracked source/config inputs were stable. No authenticated
  live calls were made. These checks validate release preparation, not publication of `v0.9.0`.

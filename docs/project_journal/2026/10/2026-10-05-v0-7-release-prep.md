---
id: 20261005-v0-7-release-prep
title: v0.7.0 Release Preparation
status: completed
created: 2026-10-05
updated: 2026-10-05
branch: wip/release-070-prep
pr:
supersedes: []
superseded_by:
---

# v0.7.0 Release Preparation

## Summary

- Prepare the release documentation for the landed opt-in CDN/downloader features and independent
  restricted-area PGC Web playurl route API. Both workspace crates, the local path dependency, and
  `Cargo.lock` are prepared at `0.7.0`; the release remains unpublished.

## Current State

- Bilingual `v0.7.0` release notes and release-note index links describe explicit CDN host selection,
  bounded probing, single- and multi-host parallel Range transfer, bundled CDN/public-resolver
  snapshots, and PGC Web route selection.
- User-facing and embedding/architecture docs identify `0.7.0` as the current development line after
  the published `0.6.0` line. API notes retain the pre-1.0 compatibility caveat and recommend
  constructors/builders and wildcard handling for non-exhaustive enums.
- `bbdown-core`, `bbdown-cli`, their local path dependency, and the lockfile are set to `0.7.0`.
- CDN controls remain opt-in. The benchmark covered two media sizes in two short morning windows
  and 18 successful downloads; results do not establish stable speedup, a default route policy, or
  all-day/multi-location performance. Public CDN benchmark requests need no credentials; restricted
  PGC live checks use user-local credentials and explicitly selected endpoints.
- Automatic routing, persistent route health, adaptive policy, scheduler diagnostics, and the
  feed/page backlog remain deferred.

## Next Steps

- Create the protected `v0.7.0` release candidate, then promote it to GitHub Release and crates.io
  after the required approval. No PR number or publication is recorded here.

## Evidence

- Feature and benchmark behavior: `docs/project_journal/2026/06/2026-06-21-overseas-cdn-routing-roadmap-019f17.md`.
- PGC route implementation and live-validation history: `docs/project_journal/2026/09/2026-09-30-pgc-web-playurl-routes.md`.
- API definitions: `crates/bbdown/src/client.rs` and `crates/bbdown/src/download.rs`.
- Release notes: `docs/release-notes/v0.7.0.md` and `docs/release-notes/v0.7.0.zh-CN.md`.
- Tool versions: `just 1.51.0`, `cargo 1.95.0`, and `rustc 1.95.0`.
- `just ci` exited 1: formatting and Clippy passed, then rustup's temporary-file operation was
  blocked by the sandbox. A narrow `rustup toolchain install 1.95.0 --profile minimal` succeeded
  without changing the configured toolchain; the recovered `cargo +1.95.0 check --workspace
  --locked` passed.
- Workspace tests passed: 728 passed, 0 failed, 3 ignored; `public_api` passed (1 test). CLI e2e
  passed (146 tests). `cargo publish --dry-run -p bbdown-core --locked` and
  `cargo doc --no-deps -p bbdown-core --locked` passed.
- `target/debug/bbdown --version` reported `bbdown 0.7.0`. Unix archive smoke packaging included the
  bilingual release notes and both release-note indexes; SHA-256 verification passed. No package
  was uploaded or published. The publish dry run reported the existing yanked `spin 0.9.8` lockfile
  warning.
- This documentation pass ran `project_journal.py validate --repo <repo>` and `git diff --check`;
  both passed. Team-generated logs, runner files, and archive temporary files were cleaned up.

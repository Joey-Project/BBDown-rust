---
id: 20261005-v0-7-release-prep
title: v0.7.0 Release Preparation
status: completed
created: 2026-10-05
updated: 2026-10-05
branch: wip/release-070-prep
pr: 85
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
- PR #85 Codex review found that `crates/bbdown/README.zh-CN.md` still described the `0.6.0`
  credential lifecycle line; the crate README is now aligned with `0.7.0` networking/downloader
  features and the independent PGC Web route API (review `5417691490`, inline `4186337026`).
- CDN controls remain opt-in. The benchmark covered two media sizes in two short morning windows
  and 18 successful downloads; results do not establish stable speedup, a default route policy, or
  all-day/multi-location performance. Public CDN benchmark requests need no credentials; restricted
  PGC live checks use user-local credentials and explicitly selected endpoints.
- Automatic routing, persistent route health, adaptive policy, scheduler diagnostics, and the
  feed/page backlog remain deferred.

## Next Steps

- Create the protected `v0.7.0` release candidate, then promote it to GitHub Release and crates.io
  after the required approval.

## Evidence

- Feature and benchmark behavior: `docs/project_journal/2026/06/2026-06-21-overseas-cdn-routing-roadmap-019f17.md`.
- PGC route implementation and live-validation history: `docs/project_journal/2026/09/2026-09-30-pgc-web-playurl-routes.md`.
- API definitions: `crates/bbdown/src/client.rs` and `crates/bbdown/src/download.rs`.
- Release notes: `docs/release-notes/v0.7.0.md` and `docs/release-notes/v0.7.0.zh-CN.md`.
- Tool versions: `just 1.51.0`, `cargo 1.95.0`, and `rustc 1.95.0`.
- An initial `just ci` attempt exited 1 when the sandbox blocked rustup's temporary-file operation;
  formatting and Clippy had passed. The narrow `rustup toolchain install 1.95.0 --profile minimal`
  then succeeded without changing the configured toolchain. The recovered `cargo +1.95.0 check
  --workspace --locked` and a complete rerun of `just ci` both passed (exit 0).
- The passing gate included formatting, Clippy, Rust 1.95 MSRV check, workspace tests (728 passed,
  0 failed, 3 ignored; `public_api` passed with 1 test), CLI e2e (146 passed), and
  `cargo publish --dry-run -p bbdown-core --locked`. Fresh `cargo doc --no-deps -p bbdown-core
  --locked` also passed.
- `target/debug/bbdown --version` reported `bbdown 0.7.0`. Unix archive smoke packaging included all
  four bilingual release-note/index members and SHA-256 verification passed. No package was uploaded
  or published. The only gate warning was the existing yanked `spin 0.9.8` lockfile entry.
- This documentation pass ran `project_journal.py validate --repo <repo>` and `git diff --check`;
  both passed. Team-generated logs, runner files, and archive temporary files were cleaned up.

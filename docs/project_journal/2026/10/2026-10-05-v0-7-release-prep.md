---
id: 20261005-v0-7-release-prep
title: v0.7.0 Release Preparation
status: completed
created: 2026-10-05
updated: 2026-10-06
branch: wip/release-070-prep
pr: 85
supersedes: []
superseded_by:
---

# v0.7.0 Release Preparation

## Summary

- Prepare the release documentation for the landed opt-in CDN/downloader features and independent
  restricted-area PGC Web playurl route API. Both workspace crates, the local path dependency, and
  `Cargo.lock` are prepared at `0.7.0`; the release was published on 2026-10-06.

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

- No remaining publication steps for `v0.7.0`.

## Publication Checkpoint

- PR #85 was squash-merged at `2026-10-05T21:05:30Z` as commit
  `70e8e1562e11783524ff28b71e06133ee653173a`.
- The successful [RC workflow run](https://github.com/Joey-Project/BBDown-rust/actions/runs/37374109441)
  created annotated tag `v0.7.0-rc.1` (object
  `b91c5bf0da4c2d433882b23587c5ede944195e2e`) targeting that source commit.
- The single [promotion workflow run](https://github.com/Joey-Project/BBDown-rust/actions/runs/37492934836)
  completed successfully on attempt 1 for the RC ref and source commit. Validation, verification,
  crate preflight, all four platform builds, GitHub Release publication, and crates.io publication
  succeeded.
- Final annotated tag `v0.7.0` (object `96d487d02d9e23eda86a4cf4511a55134f7aeb3e`) targets the
  source commit. The [GitHub Release](https://github.com/Joey-Project/BBDown-rust/releases/tag/v0.7.0)
  (id `405087898`) was published at `2026-10-06T19:51:22Z`, with `draft=false` and
  `prerelease=false`. It contains Linux x86_64, macOS x86_64/aarch64, and Windows x86_64 archives
  with SHA-256 sidecars; all eight assets are nonempty and have GitHub digests.
- The [crates.io `bbdown-core` 0.7.0 package](https://crates.io/crates/bbdown-core/0.7.0) is not
  yanked, has checksum
  `89b93b82d1aafd7da5724b767745d36e2758d8a2784a3a176099fe875115e433`, and was published at
  `2026-10-06T19:52:27.105837Z`.

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
  four bilingual release-note/index members and SHA-256 verification passed. At that preparation-gate
  checkpoint, no package had been uploaded or published; the later publication is recorded above.
  The only gate warning was the existing yanked `spin 0.9.8` lockfile entry.
- This documentation pass ran `project_journal.py validate --repo <repo>` and `git diff --check`;
  both passed. Team-generated logs, runner files, and archive temporary files were cleaned up.

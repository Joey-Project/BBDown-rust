---
id: 20260607-019e9eab-crate-publish-readiness
title: Crate Publish Readiness
status: completed
created: 2026-06-07
updated: 2026-06-07
branch: wip/crate-publish-readiness
pr: https://github.com/Joey-Project/BBDown-rust/pull/9
supersedes: []
superseded_by:
---

# Crate Publish Readiness

## Summary

- Ninth PR slice for the BBDown Rust rewrite continuation track.
- Scope is crates.io readiness for the reusable `bbdown` library crate.
- The CLI crate remains distributed through GitHub release archives rather than crates.io.

## Current State

- `crates/bbdown/Cargo.toml` has crates.io-facing metadata: description, documentation URL, readme,
  keywords, categories, repository, homepage, license, and rust-version.
- `crates/bbdown/README.md` gives embedding callers the crate scope, pre-1.0 API guidance, and a
  minimal `BiliClient` example.
- `crates/bbdown-cli/Cargo.toml` is marked `publish = false` and keeps a versioned path dependency
  on `bbdown`.
- `just publish-dry-run` runs `cargo publish --dry-run -p bbdown --locked`.
- Default CI now includes the same `bbdown` publish dry-run gate.

## Evidence

- Current-name probe: `cargo search bbdown --limit 5`.
- Baseline publish probe: `cargo publish --dry-run -p bbdown`.
- Baseline CLI publish probe: `cargo publish --dry-run -p bbdown-cli`.
- Package content check: `cargo package --list -p bbdown --allow-dirty`.
- Pre-commit publish dry run: `cargo publish --dry-run -p bbdown --locked --allow-dirty`.
- Strict local default gate after commit: `just ci`.

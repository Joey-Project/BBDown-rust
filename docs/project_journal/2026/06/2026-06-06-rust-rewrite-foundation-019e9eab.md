---
id: 20260606-019e9eab
title: Rust Rewrite Foundation
status: active
created: 2026-06-06
updated: 2026-06-06
branch: wip/rust-rewrite-foundation
pr: https://github.com/Joey-Project/BBDown-rust/pull/1
supersedes: []
superseded_by:
---

# Rust Rewrite Foundation

## Summary

- First PR slice for the BBDown Rust rewrite.
- Scope is workspace setup, CI, project documentation, typed metadata resolver, local credential store, and CLI `info/auth`.
- Download execution, subtitle/danmaku file output, muxing, QR login, and restricted-area stream fallback remain follow-up slices.

## Current State

- The Rust workspace has `bbdown` and `bbdown-cli` crates.
- Default CI is designed around formatter, clippy, unit tests, and mock e2e tests.
- Architecture and user-facing README now document the crate-first direction and selection semantics.
- PR review fixes cover redacted credential debug output, non-argv secret imports, explicit tag API
  failures, access-key-safe HTTP errors, bounded request timeouts, intl module episode parsing,
  BVID validation, correct repository metadata, and real PGC response compatibility for duplicate
  episode id fields plus string-style tags.

## Next Steps

- Complete PR readiness gates for PR #1.
- After PR #1 lands, start the stream/download planning slice.

## Evidence

- Planning thread: `019e9eab-43cf-7fc0-9218-cad1f8cd7819`.
- Reference demand threads: `019e9e9d-8782-7c71-a72d-1b3fbf0d6942`, `019e7fd0-b463-76f2-a5ab-37d7d77620f4`.
- Local gate: `just ci`.
- Live PGC smoke: `bbdown info ep508404 --json` parsed the restricted `ss41410` metadata response
  after the post-review serde fix.
- Independent review findings addressed in the PR branch.

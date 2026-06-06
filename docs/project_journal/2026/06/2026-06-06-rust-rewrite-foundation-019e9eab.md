---
id: 20260606-019e9eab
title: Rust Rewrite Foundation
status: active
created: 2026-06-06
updated: 2026-06-06
branch: wip/rust-rewrite-foundation
pr:
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

## Next Steps

- Run formatter, clippy, unit tests, mock e2e, and journal validation.
- Commit and open the foundation PR after local/internal review is clean.
- After merge, start the stream/download planning slice.

## Evidence

- Planning thread: `019e9eab-43cf-7fc0-9218-cad1f8cd7819`.
- Reference demand threads: `019e9e9d-8782-7c71-a72d-1b3fbf0d6942`, `019e7fd0-b463-76f2-a5ab-37d7d77620f4`.

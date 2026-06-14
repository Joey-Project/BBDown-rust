---
id: 20260614-019ec6-feed-list-resolver-abstraction
title: Feed/List Resolver Abstraction
status: completed
created: 2026-06-14
updated: 2026-06-14
branch: wip/feed-list-resolver-abstraction
pr:
supersedes: []
superseded_by:
---

# Feed/List Resolver Abstraction

## Summary
- Added a shared internal feed/list resolver layer for list-like Bilibili page families.
- Kept existing favorite, space upload, collection, and series public behavior compatible with the
  existing `ResolvedContent::Collection` JSON and Rust API shape.
- Prepared the `0.3.0` line for history, following/UP page, recommendation, and watch-later inputs
  without reimplementing selection and pagination behavior in each page fetcher.

## Current State
- `crates/bbdown/src/feed_list.rs` owns reusable list selection, maximum-index fetch-mode
  calculation, identity-based deduplication, and one-based renumbering helpers.
- Existing collection paths in `BiliClient` now call the shared helpers while preserving
  `VideoCollectionResolution`, selected item behavior, empty-list behavior, and collection planning
  semantics.
- Architecture and embedding docs describe the compatibility boundary and future feed/list
  extension point in English and Simplified Chinese.

## Next Steps
- Add history record parsing on top of the shared feed/list helpers as the next sequential
  `0.3.0` PR.
- Keep following/UP pages, recommendations, and watch-later as separate follow-up PRs after history
  is merged.

## Evidence
- Targeted validation: `cargo test -p bbdown-core feed_list --locked`.
- Targeted validation: `cargo test -p bbdown-core collection_ --locked`.
- Project journal validation:
  `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/29f61f3e579e2a4166436b963eab301ac5d80d94/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
- Full local gate: `just ci`, including formatter, clippy, MSRV check, workspace tests, CLI e2e,
  live manifest unit tests, and crate publish dry-run.

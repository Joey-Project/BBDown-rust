---
id: 20260614-019ec5-v0-3-feed-list-roadmap
title: v0.3.0 Feed/List Roadmap
status: active
created: 2026-06-14
updated: 2026-06-14
branch: wip/v0.3-feed-list-roadmap
pr:
supersedes: []
superseded_by:
---

# v0.3.0 Feed/List Roadmap

## Summary
- The `0.2.0` line is published from `release/0.2.0`; `master` is the `0.3.0` development line.
- The next feature set is richer Bilibili feed/list parsing, implemented as six sequential PRs.
- Each PR should start from an updated `master`, land one reviewable slice, update relevant docs and
  journals, pass the full test gate, receive the required three-review coverage, clear CI, and have
  all PR conversations resolved before merge.

## Current State
- PR 1 records this roadmap and the completed `0.2.0` release state.
- PR 2 adds a shared feed/list resolver abstraction that can support multiple page families without
  baking each page type into unrelated collection code paths.
- PR 3 adds history record parsing on top of the shared abstraction.
- PR 4 will add following/UP page parsing on top of the shared abstraction.
- PR 5 will add recommendation page parsing on top of the shared abstraction.
- PR 6 will add watch-later parsing after the other feed/list inputs are in place.
- `v0.3.0-rc.1` already exists as an abandoned tag and was never promoted. Future `0.3.0` release
  candidate creation should use the next automatically selected RC number.

## Next Steps
- Cut the following/UP page parsing branch from the updated `master` as the next `0.3.0` feature
  slice.
- Keep the remaining BBDown parity backlog, including aria2 or multi-thread download integration,
  MP4Box muxing, and subtitle-to-SRT conversion, outside this six-PR feed/list sequence unless Joey
  explicitly reprioritizes it.

## Evidence
- Published GitHub Release `v0.2.0`:
  `https://github.com/Joey-Project/BBDown-rust/releases/tag/v0.2.0`.
- Published crate version: `bbdown-core` `0.2.0`.
- `v0.2.0` tag target:
  `49c023ffc40f48f64164fbee8ec0920a044ae845`.
- Project journal validation:
  `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/29f61f3e579e2a4166436b963eab301ac5d80d94/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
- Whitespace check: `git diff --check`.
- Full local gate: `just ci`.
- Local readonly review: helper-backed `codex-readonly` first found that the active roadmap was
  incorrectly listed under completed workstreams; after removing that duplicate state, the rerun
  returned `LGTM`.
- Independent Codex PR review found that the roadmap `Next Steps` should not contain the transient
  PR 1 merge step in a squash-only repository. The roadmap now records only the stable post-merge
  next action.
- Feed/list abstraction slice detail:
  `docs/project_journal/2026/06/2026-06-14-feed-list-resolver-abstraction-019ec6.md`.
- History parsing slice detail:
  `docs/project_journal/2026/06/2026-06-14-history-feed-list-parsing-019ec7.md`.

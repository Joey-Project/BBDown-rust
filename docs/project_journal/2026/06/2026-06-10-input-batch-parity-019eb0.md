---
id: 20260610-019eb0-input-batch-parity
title: Input And Batch Collection Parity
status: completed
created: 2026-06-10
updated: 2026-06-10
branch: wip/input-batch-parity
pr: 20
supersedes: []
superseded_by:
---

# Input And Batch Collection Parity

## Summary

- Next BBDown parity slice after the 0.1.0 release flow.
- Scope is more input parsing and batch content parsing across crate APIs and CLI surfaces.
- Cover download, single-download modes, ASS danmaku, UPOS host replacement, and PCDN handling are
  recorded as separate follow-up slices.

## Current State

- `Input` accepts PUGV/cheese episode and season ids, B23 short links, space uploads, favorite
  lists, collections, and series in both raw id shorthand and common URL forms.
- Canonical `/list/ml...`, path-based `/medialist/.../ml...`, and owner-scoped
  `/list/<mid>?sid=...` / `/space.bilibili.com/<mid>/lists/...` URL forms are accepted.
- `BiliClient::resolve_input` resolves B23 links through HTTP redirect before dispatching to the
  normal input parser.
- PUGV/cheese inputs resolve as `SeasonResolution`; download planning uses `StreamSource::PugvWeb`
  and the PUGV playurl endpoint. PUGV metadata follows `episode_page` pagination through the PUGV
  episode-list endpoint before season selection.
- Favorite lists, space uploads, collections, and series resolve as `ResolvedContent::Collection`,
  carrying full collection metadata plus selected items.
- Collection resolution deduplicates current-item cursor repeats. Favorite entries without
  `ugc.first_cid` and medialist entries without embedded `pages` fall back to archive metadata;
  owner-scoped space list URLs use newer space collection/series APIs.
- The crate and CLI versions move to `0.2.0` because `ResolvedContent::Collection` is a breaking
  output API addition after the published `0.1.0` crate.
- Collection inputs select all items by default. `Selection::Page` selects one collection item and
  `Selection::Latest` selects the newest parsed item. Empty collections resolve as empty item lists
  for default/all selection.
- Collection download planning maps selected items back to normal video entries, so stream planning,
  subtitle discovery, danmaku XML URLs, download execution, and archive duplicate handling continue
  through existing paths. Planning can fetch only the selected batch items because the plan surface
  does not expose collection metadata.
- CLI `info --json` serializes collection metadata under the `collection` enum tag, and human
  summary output shows collection title, kind, item count, and selected count.
- CLI `plan --json` accepts collection inputs and selected collection items.
- English and Simplified Chinese user-facing docs were updated for README, crate README, user
  guide, embedding guide, and architecture guide.
- `docs/PROJECT_TODO.md` records the remaining parity slices for cover download,
  single-download modes, ASS danmaku, UPOS host replacement, PCDN handling, and later BBDown parity
  items.

## Evidence

- Compile gate: `cargo test --workspace --locked --no-run`.
- Crate tests: `cargo test -p bbdown-core --locked`.
- CLI mock e2e tests: `cargo test -p bbdown-cli --test cli_e2e --locked`.
- Full local gate: `just ci`, including formatter check, clippy, MSRV check, workspace tests,
  mock e2e, and publish dry-run.
- Live e2e: `just live-e2e`.
- Project journal validation passed with the project-journal helper.
- Internal review: helper-backed `codex-readonly` frozen-range review returned `LGTM` after earlier
  findings for space WBI response shape, medialist pagination cursor progress, collection
  `latest`, batch `current`, and selection-aware collection fetching were fixed and retested.
- Independent PR review found PUGV episode pagination, selected batch metadata truncation, and empty
  batch selection handling issues; all three were fixed with targeted core regression tests, and
  `cargo test -p bbdown-core --locked` passed afterward.
- GitHub Codex review found the PUGV playurl route should use `/pugv/player/web/playurl` rather
  than `/pugv/player/web/v2/playurl`; a live endpoint probe confirmed the non-v2 route returns JSON
  while the v2 route returns an HTML error page, and the code plus mock test were updated.
- A rerun independent review found two PUGV pagination edge cases and incorrect documented JSON
  paths for collection metadata. The PUGV fetcher now keeps a stable page-size fallback, refetches
  non-current `cheese/ep` selections from the first season page, and the README/user guide paths now
  use `collection.collection.items` plus `collection.selected_items`.
- A final frozen-range review found that default/current `cheese/ep` resolution could renumber a
  later-page PUGV episode as `P001`. PUGV metadata now preserves the API `episode.index` when
  present, with a regression test covering `cheese/ep102` planning from page 2.
- A later GitHub Codex review rerun found current-item medialist duplicates, PUGV pagination should
  use `/pugv/view/web/ep/list`, owner mid must be preserved for space series URLs, canonical
  `/list` and path-based medialist URLs were missing, and favorite items cannot require
  `ugc.first_cid`. These were fixed with targeted core regression tests.
- The independent review rerun found two final issues: missing medialist `pages` silently dropped
  entries, and the batch collection output API was breaking while package manifests still said
  `0.1.0`. Medialist fallback now fetches archive metadata, and manifests/docs now identify this as
  the `0.2.0` development line.
- The GitHub Codex review gate still had a current-head medialist cursor thread; follow-up medialist
  page requests now send `with_current=false` when advancing with `oid`, while retaining duplicate
  suppression as a defensive guard.

## Next Steps

- After this PR lands, split the remaining parity work into independent PRs: cover download,
  single-download modes, ASS danmaku, UPOS host replacement, and PCDN handling.

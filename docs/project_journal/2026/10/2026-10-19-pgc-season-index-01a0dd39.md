---
id: 20261019-pgc-season-index-01a0dd39
title: PGC Movie and TV Category Index
status: completed
created: 2026-10-19
updated: 2026-10-19
branch: wip/pgc-season-index
pr:
supersedes: []
superseded_by:
---

# PGC Movie and TV Category Index

## Summary

- Add bounded anonymous metadata APIs and dedicated `pgc-index list` / `pgc-index filters` CLI
  commands for PGC movies and TV series.
- Keep category listings separate from generic inputs, page hydration, season resolution, planning,
  playurl routing, and downloads.
- Update the bilingual README, user guide, embedding guide, project TODO, and page-family roadmap.

## API and Limits

- `PgcIndexKind::{Movie, Tv}` maps to `season_type=2` and `season_type=5`; both requests also send
  `type=1` through the configured `pgc_base`.
- `BiliClient::pgc_index(PgcIndexQuery)` fetches one page from
  `/pgc/season/index/result`. Query order and sort are typed. Movie supports play, updated,
  release-date, and score ordering; TV supports play, updated, danmaku, follow, and score. Score is
  descending-only. Page numbers must be positive and page size is 1–20.
- `BiliClient::pgc_index_conditions(kind)` fetches ordered filter and order choices from
  `/pgc/season/index/condition`. Filter `field`, `keyword`, and display names remain source values;
  category filter IDs are not mapped or inferred. Release-date filter values remain opaque interval
  strings where the source uses them.
- Filter query values are non-empty, contain no control characters, and are limited to 256 UTF-8
  bytes. Each API call performs one request. Listing does not call conditions or automatically
  paginate. Both calls are anonymous, do not load credential profiles or send configured
  credentials/cookies, do not follow redirects, use the configured request timeout, and cap each
  JSON response at 1 MiB.
- Page metadata retains the actual page and page size, total, continuation state, and items. Items
  retain positive `season_id`, optional `media_id` and first-episode ID, title, and useful optional
  display metadata. The first-episode ID is not the latest episode; missing identifiers are not
  synthesized.
- `pgc-index` is metadata only. It adds no generic input, automatic page hydration, season
  resolution, or download path. Existing `ss<ID>` / `md<ID>` season workflows still require an
  explicit selection; `latest` keeps its last-main-episode meaning. Existing exact `ep<ID>` flows
  remain subject to their normal access rules.
- `PgcWebPlayurlRoute` remains limited to PGC Web playurl. A selected metadata service must explicitly
  support both index endpoints and their response shapes; playurl support does not imply metadata
  support.

## Upstream Probe

- Anonymous official endpoint requests on 2026-10-19 between 09:08 and 09:09 GMT returned code 0
  for the movie condition route
  `https://api.bilibili.com/pgc/season/index/condition?season_type=2&type=1`, with `data.filter` and
  `data.order`. Omitting `type=1` returned API code -400.
- A movie result request for `type=1`, `season_type=2`, play order, descending sort, page 1, and
  page size 2 returned code 0, `has_next=1`, `num=1`, `size=2`, and `total=780`; page 2 returned two
  different rows. The equivalent TV request with `season_type=5` reported `total=1096`.
- The probe used the documented official frontend bundle
  `https://s1.hdslb.com/bfs/static/bangumi-index/bangumi-index.580b8627f994a8e66a72f4c149c61c25045deb5e.js`
  as an endpoint/parameter reference. These are historical anonymous API observations only. They do
  not establish Rust-client behavior, current regional availability, media access, or playback/download
  availability.

## Validation

- Core focused validation passed (exit 0): 9 `pgc_index` module tests plus 1 public API reexport
  test passed.
- CLI focused tests passed (5 passed). Coverage includes using the listed first episode in the
  existing PGC proxy plan and a 64 KiB mock download with byte-for-byte output matching, plus no
  requests to season/UGC endpoints when no result is selected.
- `just ci` passed (exit 0, 83.221 seconds). `cargo fmt --all -- --check`, strict workspace Clippy,
  the Rust 1.95.0 MSRV gate, workspace tests (944 passed, 3 ignored), standalone CLI e2e (185 passed),
  and publication dry run completed. The standalone CLI run repeats the 185 tests already included
  in the 944-test workspace total; do not add them again. The dry run aborted before upload as
  expected; it warned that `bbdown-core@0.10.0` already
  exists on the crates.io index and `spin v0.9.8` is yanked. No dependency update or package upload
  was performed. Receipt: `/private/tmp/bbdown-pgc-index-20261019-01a0dd39/just-ci.receipt.json`.
- Four current-source anonymous Rust CLI metadata probes passed (exit 0 each, no retries, empty
  stderr): movie filters returned 4 filter groups and 4 order choices; movie page 1 returned actual
  page 1, next page 2, 2 rows, total 780; movie page 2 returned actual page 2, next page 3, 2 rows,
  total 780; TV page 1 returned actual page 1, next page 2, 2 rows, total 1096. Each used a 10-second
  request timeout and stayed within the 20-second wall limit and combined 128 KiB output cap.
- These probes used an explicit official `pgc_base` and a nonexistent credential-file path, with no
  cookie or key. They did not request `info`, season hydration, playurl, or media. This is current
  anonymous metadata evidence only; it is not live playback or download validation. Receipts,
  stdout, and stderr are in `/private/tmp/bbdown-pgc-index-20261019-01a0dd39/` as
  `movie-filters.*`, `movie-page1.*`, `movie-page2.*`, and `tv-page1.*`.
- Project journal validation passed with
  `python3 /Users/joey/.codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust` (exit 0).
- `git diff --check` passed with exit 0 and no output.

## Evidence

- Pre-feature page-family ordering and endpoint-capability requirements:
  `2026-10-11-page-family-roadmap.md`.
- Related slices: `2026-10-18-pgc-season-grouping-01a0dd39.md` and
  `2026-10-19-pgc-timetable-01a0dd39.md`.
- Feature base: `9d66e66970a1fb9fd7081be5c50c1dfc9ebcdfb1`.

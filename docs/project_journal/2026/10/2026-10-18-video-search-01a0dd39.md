---
id: 20261018-video-search-01a0dd39
title: Ordinary Video Keyword Search
status: completed
created: 2026-10-18
updated: 2026-10-18
branch: wip/video-search
pr: https://github.com/Joey-Project/BBDown-rust/pull/106
supersedes: []
superseded_by:
---

# Ordinary Video Keyword Search

## Summary

- Add bounded ordinary-video keyword search with explicit ordering and page selection, while keeping
  result enumeration separate from selected-video metadata resolution, planning, and download.
- Keep search results in the ordinary-video category. For a selected row only, recognize an
  official PGC episode redirect when PGC metadata confirms the episode and selected video identity.
- Keep search metadata `api_base`, PGC metadata `pgc_base`, and the PGC Web playurl route separate.

## Search API

- `VideoSearchQuery::new(keyword)` defaults to page 1 and `VideoSearchOrder::TotalRank`; callers may
  use `with_page` and `with_order` to choose another positive page and one of the supported orders.
- `BiliClient::search_videos(query)` requests exactly one upstream page. `VideoSearchPage` reports
  the actual page, upstream page size, total result count, total page count, a checked optional
  `next_page`, and valid deduplicated `VideoSearchItem` rows.
- Keywords are trimmed, must contain 1–200 UTF-8 bytes, and cannot contain control characters;
  invalid input is rejected rather than truncated. Before WBI signing and sending, normalization
  removes ASCII `!`, `'`, `(`, `)`, and `*` and trims surrounding whitespace again; a keyword
  that normalizes to empty is rejected.
- `VideoSearchPage.page` uses the positive page returned by the service, or the requested page if
  the response omits that field. `next_page` is derived from this actual page.
- Search uses at most two API requests per call (WBI navigation and search), accepts at most 20 raw
  rows, caps each response at 1 MiB, and uses the configured request timeout. Both requests are
  anonymous and never receive configured credentials or cookies. After explicit selection, video
  hydration follows the normal resolver's configured credential-profile rules.
- PGC recognition is a selected-result hydration behavior, not a PGC-specific search category.
  Search metadata continues to use `api_base`; PGC episode metadata is fetched through `pgc_base`,
  and `PgcWebPlayurlRoute` only controls Web playurl routing.
- The configured API base must support `/x/web-interface/nav` and
  `/x/web-interface/wbi/search/type`. This capability is independent of `PgcWebPlayurlRoute`; neither
  endpoint configuration nor a successful mock request guarantees regional availability.

## CLI And Selection

- `bbdown search <KEYWORD>` displays one metadata page. `--page` accepts positive page numbers and
  `--order` accepts `totalrank`, `click`, `pubdate`, `dm`, `stow`, or `scores`; `--json` serializes
  page metadata and rows.
- `Input::VideoSearch` accepts `search:<keyword>` and official Bilibili video-search URLs with
  keyword, page, and order parameters. Generic `info`, `plan`, `playback`, and `download` require an
  explicit numeric/index-range selection or `all`, scoped to the one fetched page. Only selected
  rows are hydrated through normal video resolution using its usual credential-profile behavior,
  and ordinary UGC planning uses the first video part (P1). Each search-input selection re-runs the query, so its
  index is not pinned to a previously displayed snapshot; use that result's BVID or aid to target
  its stable video identity.
- For PGC recognition, only a supported official episode redirect in `data.redirect_url` is
  accepted; the resolver extracts its episode ID without following the redirect URL.
  It requests `/pgc/view/web/season?ep_id=<ID>` through `pgc_base` and requires the season to include
  that episode with an aid matching the selected video. Unconfirmed redirects fail explicitly;
  ordinary errors are not treated as regional restrictions. Search `info` retains the existing
  `VideoCollectionResolution` / `VideoCollectionItem` shape; no public item `epid` is added. Planning,
  playback, and download preserve the confirmed PGC `epid` and `cid`. PGC Web uses its configured
  Web playurl route; Web and APP retain their existing restricted-area proxy fallback. TV semantics
  are unchanged.
- Direct BVID/aid inputs keep their existing behavior and do not inherit search-only PGC detection.
  For a confirmed episode that must be targeted again, use its plan `epid` with `ep<ID>` / `Input::Episode`.
- `Input::VideoSearch` and `VideoCollectionKind::Search` are new public enum variants. Downstream
  exhaustive matches need to handle them; existing input runtime behavior is unchanged.
- The standalone search command is metadata-only and does not resolve playback or download media.

## Next Steps

- Add the new-season anime timetable and film/TV listings. Continue the order in
  `2026-10-11-page-family-roadmap.md`; popular/ranking feeds, category/channel lists, and topic pages
  follow at their recorded lower priority.

## Validation

- Focused search tests passed: core 21/21 and CLI 15/15. The CLI set includes
  `download_search_selected_pgc_uses_episode_and_restricted_area_proxy`.
- The PGC mock e2e searches, selects a BVID whose official view redirects to `ep1000`, reads PGC
  season metadata containing an unrelated `ep999`, handles an official Web playurl region error,
  falls back to the restricted proxy, and verifies exact downloaded fixture bytes. It asserts that
  the unselected UGC result does not trigger view, ordinary playurl, or media requests. This test is
  mock-only; no real PGC/live request or credentials were used.
- Final `just ci` completed with exit 0 using rustc 1.99.1 and the Rust 1.95.0 MSRV check. The gate
  passed formatting, strict Clippy, MSRV check, workspace tests, CLI e2e, and publish dry run.
  Log: `/private/tmp/bbdown-video-search.20261018-01a0dd39/pgc-search-just-ci-final.log` (92,422 bytes).
  Workspace test targets reported 915 passed, 3 ignored, and 0 failed across counts of 67, 175,
  9 (2 ignored), 640, 21 (1 ignored), 1, and 2. The CLI e2e target passed 175 tests, including the
  PGC mock case. A separate CLI e2e rerun also passed 175 tests; this repeated run is not added to
  the workspace total. The `bbdown-core` publish dry run packaged and verified the crate, then
  aborted upload as expected; no upload was made.
- The anonymous official Rust CLI metadata smokes for `Rust!` and `Rust 教程` are observations from
  earlier heads, before this selected-PGC extension. The `Rust!` smoke returned 18 items with empty
  stderr and no media request; the credentials file was absent. These observations confirm metadata
  search only and provide no live PGC, playback, or download evidence.
- Documentation validation: `project_journal.py validate` and `git diff --check` passed.

## PR #106 Review Fixes

- Normalize WBI search keywords by removing ASCII `!`, `'`, `(`, `)`, and `*`, then trimming
  whitespace before validation, signing, and sending. Reject empty and whitespace-only keywords.
- Report the positive page returned by the service, falling back to the requested page when the
  response omits it; calculate `next_page` from the actual page.
- Reject missing and unsupported search selections before CLI credential strict/renew preflight
  can run, avoiding credential renewal side effects for invalid commands. Regression coverage
  spans `plan`, `playback`, and `download` with absent, `latest`, `current`, and `episode:1`
  selections, unchanged credential-file bytes, and zero API requests. Numeric, range, and `all`
  selections retain successful planning paths.
- Reject numeric selections and range endpoints above `VIDEO_SEARCH_MAX_ITEMS` before credential
  preflight; search result indices must be within the fixed 1–20 page bound.
- Validate shorthand and URL keywords through the same query validator before credential
  preflight, including control characters, the UTF-8 byte limit, and empty normalized keywords.
- Build the anonymous CLI search client without loading credential stores or selecting credential
  profiles. Malformed stores and absent named profiles do not block metadata-only searches.
- Identify selected PGC episodes only through the official view redirect and PGC season identity
  validation described above; preserve PGC episode identity through planning and playback. This
  selected-search behavior leaves direct BV/aid inputs and the public collection item shape intact.
- PR: [#106](https://github.com/Joey-Project/BBDown-rust/pull/106). Validation of these fixes is
  recorded above.

## Evidence

- API implementation: `crates/bbdown/src/client/video_search.rs`,
  `crates/bbdown/src/input.rs`, and the public re-exports in `crates/bbdown/src/client.rs`.
- CLI implementation and mock coverage: `crates/bbdown-cli/src/main.rs` and
  `crates/bbdown-cli/tests/cli_e2e.rs`.
- Bilingual command and Rust usage: `docs/user-guide.md`, `docs/user-guide.zh-CN.md`,
  `docs/embedding.md`, and `docs/embedding.zh-CN.md`.
- Page-family ordering: `2026-10-11-page-family-roadmap.md`.

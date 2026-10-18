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
- Keep the existing PGC Web playurl route separate from metadata search endpoint capability.

## Search API

- `VideoSearchQuery::new(keyword)` defaults to page 1 and `VideoSearchOrder::TotalRank`; callers may
  use `with_page` and `with_order` to choose another positive page and one of the supported orders.
- `BiliClient::search_videos(query)` requests exactly one upstream page. `VideoSearchPage` reports
  the actual page, upstream page size, total result count, total page count, a checked optional
  `next_page`, and valid deduplicated `VideoSearchItem` rows.
- Keywords are trimmed, must contain 1–200 UTF-8 bytes, and cannot contain control characters;
  invalid input is rejected rather than truncated. Before WBI signing and sending, normalization
  removes ASCII `!`, `'`, `(`, `)`, and `*`; a keyword that normalizes to empty is rejected.
- `VideoSearchPage.page` uses the positive page returned by the service, or the requested page if
  the response omits that field. `next_page` is derived from this actual page.
- Search uses at most two API requests per call (WBI navigation and search), accepts at most 20 raw
  rows, caps each response at 1 MiB, and uses the configured request timeout. Both requests are
  anonymous and never receive configured credentials or cookies. After explicit selection, video
  hydration follows the normal resolver's configured credential-profile rules.
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
  and planning uses the first video part (P1). Each search-input selection re-runs the query, so its
  index is not pinned to a previously displayed snapshot; use that result's BVID or aid to target
  its stable video identity.
- `Input::VideoSearch` and `VideoCollectionKind::Search` are new public enum variants. Downstream
  exhaustive matches need to handle them; existing input runtime behavior is unchanged.
- The standalone search command is metadata-only and does not resolve playback or download media.

## Next Steps

- Add the new-season anime timetable and film/TV listings. Continue the order in
  `2026-10-11-page-family-roadmap.md`; popular/ranking feeds, category/channel lists, and topic pages
  follow at their recorded lower priority.

## Validation

- `cargo test -p bbdown-core parses_keyword_video_search_inputs --locked` passed: 1 test covering
  both input forms and keyword boundaries. Focused CLI search tests passed: 12 tests.
- After the final input and credential fixes, `just ci` completed with exit 0 using rustc 1.99.1
  and the Rust 1.95.0 MSRV check. Workspace test targets reported 907 passed and 3 ignored across counts of 67, 172, 9
  (2 ignored), 635, 21 (1 ignored), 1, and 2. A separate CLI e2e run passed 172 tests; this is an
  additional run, not part of the workspace total. The `bbdown-core` publish dry run packaged and
  verified the crate, then aborted upload as expected for dry-run mode; no upload was made.
- After the WBI and response-page fixes, an anonymous official Rust CLI metadata smoke for
  `Rust!`, page 1, `totalrank`, exited 0.
  It returned `page_size=20`, `total=1000`, `total_pages=50`, `next_page=2`, and 18 items; the JSON
  output was 11,507 bytes and stderr was empty. The anonymous credentials file did not exist. The
  request made no media request, so this confirms live metadata search only, not playback access,
  planning, or download e2e. Later fixes changed input validation and CLI credential handling;
  this is metadata-only evidence from before those changes.
- The earlier initial-head `Rust 教程` metadata smoke returned 17 valid items. It is a separate
  observation and is not a validation result for the PR #106 fixes.
- Documentation validation: `project_journal.py validate` and `git diff --check` passed.

## PR #106 Review Fixes

- Normalize WBI search keywords by removing ASCII `!`, `'`, `(`, `)`, and `*` before signing and
  sending, and reject a keyword that becomes empty.
- Report the positive page returned by the service, falling back to the requested page when the
  response omits it; calculate `next_page` from the actual page.
- Reject missing and unsupported search selections before CLI credential strict/renew preflight
  can run, avoiding credential renewal side effects for invalid commands. Regression coverage
  spans `plan`, `playback`, and `download` with absent, `latest`, `current`, and `episode:1`
  selections, unchanged credential-file bytes, and zero API requests. Numeric, range, and `all`
  selections retain successful planning paths.
- Validate shorthand and URL keywords through the same query validator before credential
  preflight, including control characters, the UTF-8 byte limit, and empty normalized keywords.
- Build the anonymous CLI search client without loading credential stores or selecting credential
  profiles. Malformed stores and absent named profiles do not block metadata-only searches.
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

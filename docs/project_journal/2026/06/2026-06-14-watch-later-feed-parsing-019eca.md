---
id: 20260614-019eca-watch-later-feed-parsing
title: Watch-Later Feed Parsing
status: completed
created: 2026-06-14
updated: 2026-06-14
branch: wip/watch-later-feed-list-parsing
pr:
supersedes: []
superseded_by:
---

# Watch-Later Feed Parsing

## Summary
- Added authenticated watch-later parsing as the final planned `0.3.0` feed/list feature slice.
- The input resolves through `ResolvedContent::Collection` and reuses the shared feed/list
  selection semantics for all/default, latest, page, and index/range selection.

## Current State
- `Input::WatchLater` accepts `watchlater`, `watch-later`, `watch_later`, `later`, `toview`,
  `https://www.bilibili.com/watchlater`, and `https://www.bilibili.com/list/watchlater`.
- `VideoCollectionKind::WatchLater` identifies the collection metadata in Rust and JSON output.
- The resolver uses the WEB `/x/v2/history/toview` endpoint, requires a cookie in client
  credentials, skips entries without `aid` or `cid`, deduplicates by `aid/cid`, and maps selected
  items back through the normal video planning path.
- Human-facing English and Simplified Chinese docs describe the new CLI/API input family and cookie
  requirement.

## Next Steps
- After this PR lands and all CI/review gates are clean, prepare the `0.3.0` release candidate from
  the updated `master`.
- Keep credential health checks, renewal guidance, multi-account management, and the remaining
  BBDown parity backlog as separate post-`0.3.0` workstreams unless Joey reprioritizes them.

## Evidence
- Live schema probe: `https://api.bilibili.com/x/v2/history/toview` returned `code: 0`, `data.count`,
  and top-level `aid` / `bvid` / `cid` / `page` fields for list items when called with the configured
  WEB cookie; unauthenticated probes returned `code: -101`.
- Targeted tests:
  - `cargo test -p bbdown-core input::tests::parses_feed_inputs --locked`.
  - `cargo test -p bbdown-core resolves_watch_later_items --locked`.
  - `cargo test -p bbdown-core plans_watch_later_latest_as_normal_video_entry --locked`.
  - `cargo test -p bbdown-cli --test cli_e2e info_json_resolves_mock_watch_later_collection --locked`.
- Independent PR review found that modern `/list/watchlater?bvid=...` URLs were missing from the
  URL parser; the parser and docs now cover that URL family.
- GitHub Codex review found that watch-later `page` may be an object and top-level `cid` may be
  `0`; the resolver now accepts object-shaped page details and recovers the selectable `cid` from
  `page.cid`.

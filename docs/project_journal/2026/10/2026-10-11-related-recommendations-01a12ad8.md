---
id: 20261011-related-recommendations-01a12ad8
title: Related Video Recommendations
status: completed
created: 2026-10-11
updated: 2026-10-11
branch: wip/per-video-related-recommendations
pr: https://github.com/Joey-Project/BBDown-rust/pull/96
supersedes: []
superseded_by:
---

# Related Video Recommendations

## Summary

- Add single-video related recommendations through `Input::RelatedRecommendations(u64)`, JSON kind `related_recommendation`, and the `related:<video>` CLI input, accepting av/BV IDs, numeric aids, or official desktop/mobile ordinary-video URLs.
- Add the typed `BiliClient::resolve_related_recommendations(aid, Option<Selection>)` API while preserving existing homepage recommendation aliases.

## Behavior And Boundaries

- Fetch recommendations with one `GET https://api.bilibili.com/x/web-interface/archive/related` request and decode `data` as a vector.
- Preserve response order, deduplicate by aid, skip entries with invalid aid or cid and known OGV entries, exclude the seed video, and renumber the resulting entries from one.
- `Latest` selects the first retained recommendation, not the most recent publication date. `Page` and index selectors select recommendation positions. No selector or `All` selects every valid item and permits an empty result; out-of-range selectors return `MissingField`. `Current` and `Episode` are rejected before the request.
- Reuse existing planning and download paths when a recommendation includes a known cid.
- Do not add recursive recommendation-graph traversal, pagination, or a new result limit. Future page families may be recorded for later work but are outside this workstream.
- Preserve core error and authentication boundaries. Use only an optional caller-provided WebCookie; anonymous requests may encounter risk control. Do not add credential discovery or risk-control bypasses.

## Live Endpoint Evidence

- On 2026-10-11, this workstream made one unauthenticated public request for the `normal-playlist-video` fixture `BV1QtjA6BEB8`, with no cookie or token, a 20-second deadline, and a 1 MiB response ceiling. It returned HTTP 200, API code 0, and 40 items in `data`.
- The first item had numeric `aid`, `cid`, and `duration`; string `bvid` and `title`; and an `owner` object with string `face`, numeric `mid`, and string `name`. The response was summarized in memory; private item values and media URLs were not retained or reported.
- This is a single public sample probe, not a guarantee of stable response shape or behavior across accounts, videos, or future requests.

## Upstream Implementation Reference

- The [bpi-rs related-video implementation](https://docs.rs/bpi-rs/latest/src/bpi_rs/video/recommend.rs.html) is actual consumer-side source code. It defines the endpoint and a typed `RelatedVideo` model, and its contract test checks GET with a `bvid` query parameter.
- This source is not an official Bilibili implementation or API contract. No official public definition for this web-interface endpoint was found during this workstream's research.

## Validation

- `just ci` passed on the final candidate with Cargo/Rust 1.99.0, matching remote CI, with `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0` to limit disk use. It checked formatting, strict all-target Clippy, the locked Rust 1.95 MSRV check, workspace tests, CLI e2e, and crate publish dry run. Workspace tests reported 822 passed, 0 failed, and 3 ignored; the separate CLI e2e run reported 159 passed and 0 failed.
- Core unit/mock tests cover official mobile av/BV inputs and deceptive-host rejection, identity mapping, source/OGV/invalid-item filtering, aid deduplication and ordering, selection, empty results, invalid request rejection, upstream API/HTTP/decode errors, and selected-only planning. Five CLI mock e2e cases exercise info through a mobile URL, empty results, invalid input/API errors, selected-only planning, and selected-only media download, including media contents and request counts.
- The package dry run aborted upload as expected. Existing-version and yanked `spin 0.9.8` warnings were nonblocking; no package was published by this workstream.
- The implementation and user/API documentation diff was unchanged across the successful gate. Journal validation and `git diff --check` passed. English and Simplified Chinese user/API documentation was synchronized, including identical related-recommendation embedding snippets and current-source availability.

## Future Scope

- Additional page families remain separate future work. This workstream adds only single-video related recommendations and retains homepage recommendation behavior.

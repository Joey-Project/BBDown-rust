---
id: 20261019-popular-videos-01a0dd39
title: Popular Video Metadata Page
status: completed
created: 2026-10-19
updated: 2026-10-19
branch: wip/popular-videos
pr:
supersedes: []
superseded_by:
---

# Popular Video Metadata Page

## Summary

- Add one bounded page of anonymous popular-video metadata through a public Rust API and the
  dedicated `bbdown popular` CLI command.
- Keep ranking, generic listing inputs, selected-row PGC recognition, hydration, playurl
  resolution, and media requests outside this slice.
- Update the bilingual README, user guide, embedding guide, project TODO, and page-family roadmap.

## API and Limits

- `BiliClient::popular_videos(PopularVideoQuery)` returns `PopularVideoPage`. The query defaults to
  page 1 and page size 20; page must be positive and page size is 1–20. Builders are
  `with_page(u32)` and `with_page_size(u8)`; the maximum is exposed as
  `POPULAR_VIDEO_MAX_PAGE_SIZE`.
- The endpoint is `/x/web-interface/popular?pn=<page>&ps=<page-size>` through configured
  `EndpointConfig.api_base`. Each call performs one anonymous GET with redirects disabled, a 1 MiB
  response cap, and the configured timeout.
- The command/API uses only API base and request timeout. It does not inspect credential profiles or
  run credential preflight, renewal, resolver, restricted-area, or PGC configuration. Credentials,
  cookies, and access keys are not loaded or sent.
- The upstream response supplies only `data.list` and `data.no_more`; it does not echo requested
  page parameters or provide a total. `PopularVideoPage` preserves `requested_page` and
  `requested_page_size`; `has_next` and `next_page` are derived from `no_more`.
- `PopularVideoItem` includes positive `aid`, optional `bvid`, title, optional author and display
  metadata, duration, and optional counters. `stats` contains optional view, danmaku, reply,
  favorite, coin, share, and like counts.
- A present BVID is format-checked and its decoded aid must match the row's positive `aid`; a
  conflicting pair rejects the response. Missing BVID values remain absent and are not synthesized.
- The list is dynamic; separate requests do not promise a fixed snapshot, stable order, or total.
  There is no automatic pagination. The API adds no generic input and does not hydrate details,
  resolve playurl, request images, or request media. An application can explicitly pass a displayed
  row's BVID or aid to an existing direct-video flow; that does not carry selected-row PGC detection.
- This source-only feature is absent from published `v0.10.0`. Ranking remains a separate backlog
  item.

## Upstream Probe

- Anonymous requests to the official `/x/web-interface/popular?pn=1&ps=2` and
  `/x/web-interface/popular?pn=2&ps=2` endpoint returned two rows on each page, with distinct IDs
  and `no_more=false` in both responses. This confirms the observed response shape and that the
  requested page can be passed through; it does not establish stable ordering or a fixed snapshot.
- Probe artifacts: `/private/tmp/bbdown-popular-20261019-01a0dd39/popular.json`,
  `popular.headers.json`, `popular-pn2.json`, and `popular-pn2.headers.json`.
- A separate `/x/web-interface/ranking/v2?rid=0&type=all` response was observed for future
  planning only. Ranking is not implemented by this slice.

## Validation

- Focused validation passed (exit 0): 8 popular core tests plus 1 public API integration test; CLI
  popular tests passed 5/5. The CLI coverage includes passing a returned BVID explicitly into the
  existing plan flow. This did not run a media download e2e.
- Full `just ci` passed (exit 0, 84.143 seconds): formatting, strict Clippy, MSRV 1.95.0,
  workspace tests (958 passed, 3 ignored; this includes the CLI's 190 tests), standalone CLI e2e
  (190 passed), and publish dry run completed. The standalone CLI e2e repeats the same 190 tests
  already counted in the workspace total and must not be added again. The dry run aborted before
  upload as expected. Receipt and log:
  `/private/tmp/bbdown-popular-20261019-01a0dd39/just-ci-final-r3-receipt.json` and
  `/private/tmp/bbdown-popular-20261019-01a0dd39/just-ci-final-r3.log`.
- Two bounded anonymous Rust CLI page probes passed (exit 0 each, no retries, empty stderr):
  requested page 1 / size 2 returned 2 items, `has_next=true`, `next_page=2`; requested page 2 /
  size 2 returned 2 items, `has_next=true`, `next_page=3`. The two observed pages had different
  BVIDs; this is only a two-request observation and does not imply snapshot or stable-order
  guarantees. Both used `https://api.bilibili.com`, a 10-second request timeout, a 20-second wall
  limit, and a 128 KiB stdout cap. A deliberately absent credential file stayed absent before and
  after each command. The probe did not invoke `info`, `plan`, `playback`, `download`, or media/cover
  fetches. Receipts, stdout, stderr, and scope are in `/private/tmp/bbdown-popular-20261019-01a0dd39/`
  as `probe-page1-*`, `probe-page2-*`, and `probe-scope.json`.
- Focused project-journal validation and `git diff --check` passed after the documentation updates.
- No live video, playurl, image, media, or download validation is claimed.

## Evidence

- Page-family ordering and endpoint-capability requirements:
  `2026-10-11-page-family-roadmap.md`.
- Base commit after PGC index PR #108 merged: `8fafaec188d5d84e025343b8fbffefb91856bfe1`.

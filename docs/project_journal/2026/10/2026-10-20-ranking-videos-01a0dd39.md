---
id: 20261020-ranking-videos-01a0dd39
title: Global Ranking Video Metadata
status: completed
created: 2026-10-20
updated: 2026-10-20
branch: wip/ranking-videos
pr:
supersedes: []
superseded_by:
---

# Global Ranking Video Metadata

## Summary

- Add one bounded anonymous global ranking response through `BiliClient::ranking_videos()` and the dedicated `bbdown ranking` command.
- Keep category/type selection, pagination, generic listing inputs, selected-row PGC detection, detail hydration, playurl resolution, and media requests outside this slice.
- Synchronize bilingual README, user guide, embedding guide, project TODO, and page-family roadmap.

## API and Limits

- `BiliClient::ranking_videos()` returns `RankingVideoPage { note: Option<String>, items: Vec<RankingVideoItem> }`.
- The request uses the configured `EndpointConfig.api_base` for `/x/web-interface/ranking/v2?rid=0&type=all`. These full-site/default-type parameters are fixed; there is no pagination or configurable category/type.
- Each invocation performs one anonymous GET with redirects disabled, a 1 MiB response cap, a hard limit of 100 raw rows, and the configured timeout. Oversized responses or more than 100 raw rows reject the response rather than truncate it.
- Only API base and request timeout are used. Credentials, cookies, and access keys are not loaded or sent; credential renewal, resolver, restricted-area, PGC initialization, and navigation/WBI signing are not used.
- Each item has a one-based `rank` derived from source position, `aid`, optional `bvid`, `title`, optional author/display metadata, and `RankingVideoStats` with seven optional counters.
- The source order can change. Separate requests do not share a snapshot, and the response does not promise a total. An optional upstream note is preserved when present.
- This is metadata only. It does not hydrate details, resolve playurl, request images or media, add a generic input, or add selected-row PGC detection. A caller may explicitly pass an item's BVID or aid to an existing direct-video flow, which follows its existing rules.
- The published `v0.10.0` CLI and crate do not include this source-only feature.

## Probe Context

- The local anonymous official-endpoint sample at `/private/tmp/bbdown-popular-20261019-01a0dd39/ranking-next-slice.json` is historical response-shape evidence only. It is not a live validation of this implementation or today's service behavior.

## Validation

- Focused CLI ranking coverage passed 6/6. Focused core coverage passed 6 unit tests plus 1 public API integration test after mock/assertion guard fixes. Earlier failed-attempt logs remain private and are not used as passing evidence.
- `just ci` passed (exit 0, 82.08 seconds): 971 passed, 0 failed, and 3 ignored across the workspace. The reported groups were 67 CLI unit tests, 196 CLI e2e tests, 9 live-e2e tests (2 ignored), 671 core library tests, 21 CDN benchmark tests (1 ignored), 1 device-cookie test, 6 public API tests, and 0 doctests. Receipt and bounded log: `/private/tmp/bbdown-ranking-20261020-01a0dd39/just-ci-final.receipt.json` and `/private/tmp/bbdown-ranking-20261020-01a0dd39/just-ci-final.log`.
- A separate standalone `cargo test -p bbdown-cli --test cli_e2e --locked` run passed 196/196; this repeats the CLI e2e group from `just ci` and is not added to its totals.
- One authorized real Rust CLI probe used an isolated environment, `https://api.bilibili.com`, a 10-second request timeout, a 20-second wall limit, and a 256 KiB combined-output cap. The deliberately absent credential file remained absent before and after. The command returned exit 1 after 0.594 seconds with `API returned code -352: -352`; it produced no rows, note, or rank sequence. There was no retry and no follow-up `info`, `plan`, playurl, image, cover, or media request. This is an unsuccessful live metadata acceptance probe; the observed error code is recorded without attributing a cause. Receipt and output: `/private/tmp/bbdown-ranking-20261020-01a0dd39/live-ranking-probe.receipt.json` and `/private/tmp/bbdown-ranking-20261020-01a0dd39/live-ranking-probe.log`.
- Live acceptance of ranking rows and source-position ranks remains unverified. The historical sample above remains shape evidence only.

## Evidence

- Page-family ordering and validation expectations: `2026-10-11-page-family-roadmap.md`.
- Base commit after the popular-video slice: `4b3521c25f449f692ae51925863b428ecacac05a`.

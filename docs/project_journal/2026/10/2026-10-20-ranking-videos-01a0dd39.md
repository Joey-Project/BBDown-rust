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
- Each invocation may make at most one anonymous GET through `EndpointConfig.api_base` to `/x/web-interface/nav` for public `wbi_img` keys, accepting navigation response codes `0` and `-101` for key extraction. It then makes one WBI-signed GET through the same configured base to `/x/web-interface/ranking/v2?rid=0&type=all&web_location=333.934`; the existing signer adds `wts` and `w_rid`.
- Both requests use the configured per-request timeout, disable redirects, and cap each response at 1 MiB. More than 100 raw ranking rows rejects the response rather than truncating it. No navigation-key cache is introduced.
- Only API base and request timeout are used. Credentials, cookies, and access keys are not loaded or sent; credential renewal, account authentication, ticket requests, device-cookie initialization, resolver, restricted-area, and PGC initialization are not used.
- A configured API gateway must support both navigation and ranking endpoints, their query parameters and response shapes, and forwarding of the signed ranking query.
- Each item has a one-based `rank` derived from source position, `aid`, optional `bvid`, `title`, optional author/display metadata, and `RankingVideoStats` with seven optional counters. Missing, null, empty, and whitespace-only BVID values remain absent.
- The source order can change. Separate requests do not share a snapshot, and the response does not promise a total. An optional upstream note is preserved when present.
- This is metadata only. It does not hydrate details, resolve playurl, request images or media, add a generic input, or add selected-row PGC detection. A caller may explicitly pass an item's BVID or aid to an existing direct-video flow, which follows its existing rules.
- The published `v0.10.0` CLI and crate do not include this source-only feature.

## Probe Context

- The local anonymous official-endpoint sample at `/private/tmp/bbdown-popular-20261019-01a0dd39/ranking-next-slice.json` is historical response-shape evidence only. It is not a live validation of this implementation or today's service behavior.

## Historical Candidate Validation (Pre-Fix)

- Before the query and BVID normalization fixes, focused CLI ranking coverage passed 6/6; focused core coverage passed 6 unit tests plus 1 public API integration test. `just ci` passed (exit 0, 82.08 seconds): 971 passed, 0 failed, and 3 ignored across the workspace. The reported groups were 67 CLI unit tests, 196 CLI e2e tests, 9 live-e2e tests (2 ignored), 671 core library tests, 21 CDN benchmark tests (1 ignored), 1 device-cookie test, 6 public API tests, and 0 doctests. Receipt and bounded log: `/private/tmp/bbdown-ranking-20261020-01a0dd39/just-ci-final.receipt.json` and `/private/tmp/bbdown-ranking-20261020-01a0dd39/just-ci-final.log`.
- A separate pre-fix standalone `cargo test -p bbdown-cli --test cli_e2e --locked` run passed 196/196; it repeats the CLI e2e group from `just ci` and is not added to its totals.
- The pre-fix candidate's one authorized real Rust CLI probe used an isolated environment, `https://api.bilibili.com`, a 10-second request timeout, a 20-second wall limit, and a 256 KiB combined-output cap. The deliberately absent credential file remained absent before and after. It returned exit 1 after 0.594 seconds with `API returned code -352: -352`; it produced no rows, note, or rank sequence. There was no retry and no follow-up `info`, `plan`, playurl, image, cover, or media request. This was an unsuccessful live metadata acceptance probe; the observed error code is recorded without attributing a cause. Receipt and output: `/private/tmp/bbdown-ranking-20261020-01a0dd39/live-ranking-probe.receipt.json` and `/private/tmp/bbdown-ranking-20261020-01a0dd39/live-ranking-probe.log`.
- The historical sample at `/private/tmp/bbdown-popular-20261019-01a0dd39/ranking-next-slice.json` is response-shape evidence only and is neither current validation nor evidence about the observed `-352` response.

## Official Page Context

- The official ranking page `https://www.bilibili.com/v/popular/rank/all` exposes `meta.spm_prefix=333.934`. Its linked official script `https://s1.hdslb.com/bfs/static/2233-monorepo/popular/static/js/index.55b42adc.js` shows a common GET hook injecting `web_location`.
- The same script includes WBI middleware/bootstrap context, and the existing Rust WBI helper signs requests with `wts` and `w_rid`. This observed page/request and local implementation context supports the documented request flow. It does not establish the cause of the earlier `-352` response, nor prove that WBI signing or `web_location` is necessary or sufficient to avoid that error.

## Historical Intermediate Context-Fix Candidate (Pre-WBI)

- Focused CLI ranking tests passed 6/6. Focused core ranking tests passed 7 unit tests and 1 public API re-export test. Receipts: `/private/tmp/bbdown-ranking-20261020-01a0dd39/contextfix-cli-ranking-focused.receipt.json` and `/private/tmp/bbdown-ranking-20261020-01a0dd39/contextfix-core-ranking-focused.receipt.json`.
- `just ci` passed (exit 0, 56.713 seconds); workspace Cargo test result lines totaled 972 passed, 0 failed, and 3 ignored. Receipt and log: `/private/tmp/bbdown-ranking-20261020-01a0dd39/contextfix-just-ci.receipt.json` and `/private/tmp/bbdown-ranking-20261020-01a0dd39/contextfix-just-ci.log`.
- A separate standalone CLI e2e run passed 196/196; it is not added to the `just ci` total.
- No live probe was run on this intermediate context-fix candidate. The only real CLI probe remains the unsuccessful earlier `-352` observation above; it does not establish the cause of that response or validate the fixed query against the live service.

## Final WBI Candidate Validation

- Focused CLI ranking tests passed 6/6. Focused core ranking coverage passed 9 unit tests plus 1 public API re-export test. Receipts: `/private/tmp/bbdown-ranking-20261020-01a0dd39/wbifix-cli-ranking-focused-2.receipt.json` and `/private/tmp/bbdown-ranking-20261020-01a0dd39/wbifix-core-ranking-focused-2.receipt.json`.
- `just ci` passed (exit 0, 53.901 seconds); workspace Cargo test result lines totaled 974 passed, 0 failed, and 3 ignored. Receipt and log: `/private/tmp/bbdown-ranking-20261020-01a0dd39/wbifix-just-ci.receipt.json` and `/private/tmp/bbdown-ranking-20261020-01a0dd39/wbifix-just-ci.log`.
- A separate standalone CLI e2e run passed 196/196; this is a repeat and is not added to the `just ci` total.
- One authorized real Rust CLI probe against the official API succeeded in one invocation (exit 0, 1.373 seconds). It ran in an isolated environment with a deliberately absent credential file verified before and after, a 10-second per-request timeout, 30-second wall limit, and 256 KiB output cap. At most two anonymous requests were in scope: navigation-key fetch and signed ranking GET. The parsed response had 100 rows with source ranks 1 through 100 and note `根据稿件内容质量、近期的数据综合展示，动态更新`. There were no retries or follow-up media requests. Receipt and output: `/private/tmp/bbdown-ranking-20261020-01a0dd39/wbifix-live-ranking-probe.receipt.json` and `/private/tmp/bbdown-ranking-20261020-01a0dd39/wbifix-live-ranking-probe.log` (85,318 bytes).
- This is one successful observation from one host and request window. It does not establish stable ordering, a fixed snapshot, a total, general availability, or the cause of the earlier `-352` response.

## Evidence

- Page-family ordering and validation expectations: `2026-10-11-page-family-roadmap.md`.
- Base commit after the popular-video slice: `4b3521c25f449f692ae51925863b428ecacac05a`.

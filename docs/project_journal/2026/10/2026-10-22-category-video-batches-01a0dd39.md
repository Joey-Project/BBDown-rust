---
id: 20261022-category-video-batches-01a0dd39
title: Category Video Metadata Batch
status: completed
created: 2026-10-22
updated: 2026-10-22
branch: codex/category-video-metadata
pr:
supersedes: []
superseded_by:
---

# Category Video Metadata Batch

## Summary

- Implement one anonymous, bounded category-video metadata batch through `BiliClient::category_videos` and `bbdown category list`.
- Keep category catalogs, pagination, channel listings, topics, generic inputs, selection, detail hydration, playurl, and media downloads outside this slice.
- This is an addition in the current source checkout, not part of the `0.11.0` release.

## API and Limits

- The endpoint is `/x/web-interface/newlist?rid=<rid>&pn=1&ps=<batch-size>` through the configured API base. The command accepts a positive category RID, batch size, and maximum decoded-response byte count. Defaults are 20 and 1 MiB for batch size and response bytes; no guessed numeric upper limits are imposed.
- The Rust API returns the requested RID and batch size with the archive items. `pn` is fixed to 1. A valid batch preserves all returned rows in source order; malformed required fields fail the batch rather than being silently dropped. The upstream can return more or fewer rows than requested, so `ps` is not a returned-row guarantee.
- Items include ordinary archive metadata and independently optional row fields for `tid`/`tname` and `tidv2`/`tnamev2`. These fields describe each row; they are not assumed to match the requested RID or to form a category hierarchy.
- The request uses only the configured API base and timeout. It is anonymous, does not follow redirects, and does not initialize credential or PGC resolver state. A configured API base must support this exact endpoint, query, and native response envelope.
- The command/API does not expose page number, continuation, total, a latest guarantee, or stable ordering. It does not add generic input, selection, hydration, playurl, or media behavior.
- The feature is not a category catalog or channel/topic implementation. Full category catalogs, pagination, channel lists, and topic pages remain future work.

## Bounded Upstream Research

- Anonymous official `newlist` probes returned HTTP 200 / API code 0 with a `data.archives` array and a `data.page` object. On tested requests, `page.num` and `page.size` echoed the request, while `page.count` was zero alongside nonempty archives. `pn=1` and `pn=2` returned the same ordered ID hashes for the tested RID and size; changing `ps` from 2 to 4 changed the number of returned rows. These observations support a fixed first-batch interface and do not establish pagination, a total, latest semantics, or stable ordering.
- In six official anonymous requests, `type=0` and `type=1` produced identical ordered ID hashes at sizes 2 and 4; changing `pn` from 1 to 2 likewise did not advance sample rows. The tested rows had distinct category metadata from some requested RIDs; the API therefore preserves row values without interpreting them as a category catalog or parent mapping.
- Official `/x/web-interface/dynamic/region` probes returned API code `-404`; `/x/web-interface/region/list` returned HTTP 404. These results do not supply a catalog source. Official page and script inspection showed a modern region-feed candidate, but two source-shaped WBI-signed requests returned HTTP 200 / API code `-400`. This does not prove whether authentication or another requirement is involved; the route is not part of this implementation.
- Endpoint research made 21 anonymous GETs to `api.bilibili.com` and the selected `atri.ink` service (8 initial endpoint checks, 4 schema checks, 6 type/page checks, and 3 WBI feasibility checks). These probes used no credentials or cookies, disabled redirects, bypassed proxy environment settings, used a 10-second request timeout, and capped decoded bodies at 1 MiB; the aggregate run windows were each bounded. A separate four-request frontend/source inspection used `www.bilibili.com`, the official static-script host, and one unsigned API check; its asset fetch used a 4 MiB cap. Private safe summaries and scripts are under `/private/tmp/bbdown-category-20261022-01a0dd39/` (`official-probe-evidence.json`, `newlist-schema-evidence.json`, `newlist-type-pagination-evidence.json`, and `signed-region-feed-evidence.json`). No raw titles, IDs, media URLs, WBI keys, or signed URLs were retained in those summaries.
- The primary frontend source inspected was `https://s1.hdslb.com/bfs/static/shanks/channel/assets/index-bedd2c00.js`; it documents a modern region-feed caller but its failed API probes are not validation of a working listing contract.

## Validation

- Full CI passed on the current implementation (exit 0, 2026-10-22T21:25:55Z–21:27:24Z). Workspace tests reported 1,029 passed and 4 ignored across the reported groups (67 + 210 + 15 + 707 + 21 + 2 + 7); separate CLI e2e passed 210/210 and repeats the workspace CLI e2e group. Formatting, strict Clippy, MSRV 1.95.0, and publish dry run passed; the dry run did not publish. Receipt and bounded log: `/private/tmp/bbdown-category-video-metadata-20261022-01a0dd39/ci-attempt3.log.json` and `ci-attempt3.log`.
- One bounded actual CLI acceptance run passed all four commands at 2026-10-22T21:31:45Z–21:31:49Z (3.278 seconds): official `rid=1`, batch size 4 returned 4 rows; official `rid=13`, size 2 returned 2; selected `atri.ink` service returned 4 and 2 rows for the same requests. All commands exited successfully with empty stderr. This confirms the built CLI's anonymous endpoint/shape behavior for those four samples only; it does not establish pagination, stable ordering, a total, or latest semantics. The helper cleared auth/proxy environment and made no credential, detail, playurl, image, or media request.
- The tested binary SHA-256 was `ff83b68679dfb277634f75f0d88871951d60b23d049a283fb6173752e0550f61`; its hash and the six Rust source-file hashes remained unchanged through acceptance. The binary was built from the working tree based on `ed41ff…` before commit, so this is not a clean-base binary validation. Safe evidence, outer log, and receipt are `/private/tmp/bbdown-category-20261022-01a0dd39/cli-acceptance-evidence.json`, `cli-acceptance-run.log`, and `cli-acceptance-run.log.json`.
- No media, playurl, detail-hydration, credentialed, or authenticated validation is in scope.

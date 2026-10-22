---
id: 20261022-pgc-index-service-route-01a0dd39
title: PGC Movie and TV Index Metadata Service Route
status: completed
created: 2026-10-22
updated: 2026-10-22
branch: codex/pgc-index-service-route
pr:
supersedes: []
superseded_by:
---

# PGC Movie and TV Index Metadata Service Route

## Summary

- Extend capability-declared `PgcMetadataRoute::ServiceOnly` to movie and TV index pages and filter/order metadata with an independent `Index` capability.
- Keep the default `Configured` route and existing list/filter parsing, query validation, pagination, ordering, and metadata-only behavior unchanged.

## Current State

- The service capability covers `/pgc/season/index/result` and `/pgc/season/index/condition` for movie (`season_type=2`) and TV (`season_type=5`) categories. Result requests preserve existing query parameters; conditions preserve their existing type/category mapping.
- A ServiceOnly request requires `Index` before HTTP, is anonymous, does not follow redirects, and does not fall back to the configured PGC base or proxy. The decoded response cap is the smaller of the service's configured/default 8 MiB limit and the index's existing 1 MiB cap. Existing filter, order, row, and page bounds remain unchanged.
- `Index` is independent from `Season`, `MediaReview`, and `Timetable`. Index metadata adds no selected-row hydration, playurl, or media behavior; ordinary search, PUGV, and international PGC remain outside this route.
- Preliminary anonymous HTTP shape probe only: eight direct GETs to `api.bilibili.com` and `atri.ink` covered both endpoints for both categories, with no credentials, redirects, or proxy. From `2026-10-22T17:47:04.624112Z` to `17:47:11.898932Z`, all returned HTTP 200 and API code 0. All bodies were 1,348–3,259 bytes, below the 1 MiB observation cap. Both condition projections matched in ordered fields, values, and order options. Result page 1 (`order=2`, `sort=0`, `page=1`, `pagesize=2`) returned two valid season IDs and `has_next=1` for each category; ordered ID projections matched between hosts. Movie totals differed (780 official, 792 atri); TV totals differed (1,099 official, 1,135 atri). These are endpoint-shape observations, not identical-catalog claims or current-source CLI validation.
- The full `just ci` gate passed at `2026-10-22T18:07:59.853436Z`–`18:08:44.096699Z` (44.24 s): formatting, Clippy, Rust 1.95 MSRV, publish dry run, and 1,014 unique workspace tests passed with 4 ignored. The separate CLI suite passed all 206 tests and is not added again to the unique test count. No package was published.
- Eight current-source CLI commands passed once at `2026-10-22T18:09:13.250161Z`–`18:09:19.719030Z` (6.424 s): configured official and `ServiceOnly(atri.ink)` for movie and TV filters and page 1 lists. The four filter results each had 4 groups, 58 movie or 44 TV values, and 4 movie or 5 TV order options. Both service condition projections matched their official counterparts, including ordered filter values and order options. Each list had two positive season IDs, page 1, size 2, `has_next=true`, and `next_page=2`; ordered ID and pagination projections matched within each category. Totals differed between services: movie 780 official vs 792 service, TV 1,099 official vs 1,135 service. The result projection difference was only the total; no identical-catalog claim is made.
- Service-only commands set `--pgc-base http://127.0.0.1:1`; success demonstrates they did not fall back to that base. All runs used an explicit absent private credential path, credential preflight off, a 10-second request timeout, cleared proxy and BBDown environment, and a 2 MiB combined-output cap per command. The sentinel credential file remained absent. No playurl or media request was made.
- Earlier full-gate attempts exposed three strict Clippy length warnings, a mock-lifetime issue, and a stale CLI help assertion; these were corrected before the passing full gate. A separate core Clippy attempt with an OS file-size limit terminated a compiler process with `SIGXFSZ` and produced no valid diagnostic; that isolated runner issue is archived under the private task directory, not a product failure.

## Validation

- Preliminary HTTP summary and exact settings: `/private/tmp/bbdown-index-service-20261022-01a0dd39/http-probe-evidence.json`; bounded probe source: `/private/tmp/bbdown-index-service-20261022-01a0dd39/probe.py`. Raw response bodies were not retained.
- Bounded current-source CLI acceptance runner: `/private/tmp/bbdown-index-service-20261022-01a0dd39/run_cli_acceptance.py`; it completed once against the fresh `target/debug/bbdown` build.
- Safe current-source CLI summary and projection comparisons: `/private/tmp/bbdown-index-service-20261022-01a0dd39/cli-acceptance-evidence.json`; bounded execution receipt: `/private/tmp/bbdown-index-service-20261022-01a0dd39/cli-acceptance-run.log.json`.
- Full CI receipt and output: `/private/tmp/bbdown-index-service-20261022-01a0dd39/ci-attempt4.log.json` and `/private/tmp/bbdown-index-service-20261022-01a0dd39/ci-attempt4.log`.

## Evidence

- Existing PGC index API and parser behavior: `2026-10-19-pgc-season-index-01a0dd39.md`.
- Independent season/review and timetable service routes: `2026-10-21-pgc-metadata-service-routes-01a0dd39.md` and `2026-10-22-pgc-timetable-service-route-01a0dd39.md`.
- Public API and routing guide: `docs/embedding.md` and `docs/embedding.zh-CN.md`.

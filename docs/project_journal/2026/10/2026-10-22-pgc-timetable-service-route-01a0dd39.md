---
id: 20261022-pgc-timetable-service-route-01a0dd39
title: PGC Timetable Metadata Service Route
status: completed
created: 2026-10-22
updated: 2026-10-22
branch: codex/pgc-timetable-service-route
pr:
supersedes: []
superseded_by:
---

# PGC Timetable Metadata Service Route

## Summary

- Extend capability-declared `PgcMetadataRoute::ServiceOnly` to anime and Chinese-animation timetable metadata with an independent `Timetable` capability.
- Keep the default `Configured` route, timetable parsing and metadata-only behavior unchanged.

## Current State

- The service route uses `/pgc/web/timeline/v2?season_type=1|4`; it checks `Timetable` before HTTP, sends an anonymous request, follows no redirects, and has no official or proxy fallback.
- Its decoded response cap is the smaller of the service's configured/default 8 MiB limit and the timetable's existing 1 MiB cap. Existing 32-day-group and 4096-row limits remain in force.
- `Season` and `MediaReview` do not imply `Timetable`. Movie/TV index, search, PUGV, international PGC, playurl, and media remain outside this service route.
- The full `just ci` gate passed from `2026-10-22T15:07:13.247203Z` to `15:07:54.558556Z` (41.31 s): formatting, strict Clippy, MSRV 1.95, publish dry run, and 1004 unique workspace test passes with 4 ignored. The standalone CLI suite's 201 passes were included in that total. No package was published.
- Four bounded CLI runs passed at `2026-10-22T15:08:53Z`–`15:08:56Z`: configured official and `ServiceOnly(atri.ink)` for anime and guochuang. Each returned typed JSON with the expected kind, `latest` and `days` arrays, positive season IDs, valid optional IDs/fields, no more than 32 day groups or 4096 aggregate rows. The timetable client enforces its 1 MiB decoded-response cap; the CLI harness independently capped each command's combined output at 2 MiB. Anime returned 12 latest rows, 7 days, and 28 aggregate rows, with an empty `10-25` group; guochuang returned 16 latest rows, 7 days, and 39 aggregate rows. Both date windows were `10-19` through `10-25`.
- Official and service projections matched for both kinds: ordered day labels and date fields, ordered episode/season IDs, `published`/`delayed` state, and the separate latest block. Projection equality does not assert byte-for-byte equality of titles, cover URLs, or other display metadata. Service-only commands used `--pgc-base http://127.0.0.1:1`; their success demonstrates the route did not fall back to that base. The runs used a nonexistent private credential path, preflight off, 10-second request timeout, and cleared proxy/BBDown environment for each CLI process. They did not request playurl or media.
- Earlier gate attempts exposed a `const`/`mut` compilation conflict, seven strict Clippy `expect_err` violations, and an overlapping mock expectation; those were corrected before the passing full gate.
- Preliminary anonymous HTTP probes to `api.bilibili.com` and `atri.ink` completed at `2026-10-22T14:53:33Z`–`14:53:41Z`. All four requests returned HTTP 200 and API code 0. This remains endpoint-shape evidence separate from the later current-source CLI result.

## Evidence

- Safe HTTP summary and request settings: `/private/tmp/bbdown-timetable-service-20261022-01a0dd39/evidence.json`.
- Bounded probe source: `/private/tmp/bbdown-timetable-service-20261022-01a0dd39/probe.py`.
- Safe current-source CLI summary and projection comparisons: `/private/tmp/bbdown-timetable-service-20261022-01a0dd39/cli-acceptance-evidence.json`.
- Bounded CLI validation runner: `/private/tmp/bbdown-timetable-service-20261022-01a0dd39/run_cli_acceptance.py`.
- Existing timetable API and bounds: `2026-10-19-pgc-timetable-01a0dd39.md`.
- Prior service capability route: `2026-10-21-pgc-metadata-service-routes-01a0dd39.md`.

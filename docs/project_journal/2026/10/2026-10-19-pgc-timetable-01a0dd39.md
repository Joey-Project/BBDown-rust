---
id: 20261019-pgc-timetable-01a0dd39
title: PGC Anime Timetable Metadata
status: completed
created: 2026-10-19
updated: 2026-10-19
branch: wip/pgc-timetable
pr:
supersedes: []
superseded_by:
---

# PGC Anime Timetable Metadata

## Summary

- Add a bounded anonymous metadata API and dedicated `timetable` CLI for anime and Chinese animation.
- Keep timetable results separate from generic `Input`, collection selection, media planning, and downloads.
- Preserve the upstream latest block, day order and empty days, episode identity, publication/delay state, and cover metadata.
- Film and TV category listings remain a separate follow-up.

## API and Limits

- `PgcTimetableKind::{Anime, Guochuang}` maps to `season_type=1` and `season_type=4`.
- `BiliClient::pgc_timetable(kind)` requests `/pgc/web/timeline/v2` through configured `pgc_base` with only the selected `season_type` query parameter.
- `PgcTimetable` exposes `kind`, `latest`, and `days`. Day records retain `date`, `date_ts`, `day_of_week`, `is_today`, and their ordered episode rows, including empty day groups.
- Episode rows retain optional positive `episode_id`, positive `season_id`, title, `pub_index`, `pub_time`, `pub_ts`, `published`, `delayed`, optional `delay_index`, optional `delay_reason`, and optional `cover_url`.
- `latest` is kept as a separate upstream block without inferred freshness semantics. Date and publication-time strings are service labels; the client does not convert them to local time.
- The request is anonymous, follows no redirects, and is capped at 1 MiB. The decoded response is limited to 32 day groups and 4096 aggregate raw rows; exceeding a limit is an error, not silent truncation.
- Metadata routing through `pgc_base` is independent of `PgcWebPlayurlRoute`, which covers PGC Web playurl only. Compatible service support and regional availability are endpoint-specific.
- An available exact episode ID can be used through existing `ep<ID>` flows subject to their normal access rules. Missing IDs are not synthesized; `published: false` does not promise playability. Film/TV listing inputs are not added.

## Upstream Probe

- Anonymous `GET https://api.bilibili.com/pgc/web/timeline/v2?season_type=1` returned HTTP 200 and API code 0 at `2026-10-19T07:26:12Z`.
- The observed result contained separate `latest` and `timeline` arrays, seven server-defined day groups from October 16 through October 22, 16 rows in `latest`, and 26 rows across `timeline`.
- This probe confirms only the anonymous metadata response for that request. It does not establish Rust client behavior, PGC playurl access, playback planning, or download availability.

## Validation

- `just ci` passed with exit code 0 in 31.68 seconds using `just 1.51.0`, stable `rustc`/`cargo 1.99.1`, and the separate MSRV `rustc 1.95.0` gate. Formatting, strict workspace Clippy, the MSRV check, workspace tests (929 passed, 3 ignored, 0 failed, including 180 CLI e2e tests), standalone CLI e2e (180 passed), and publication dry run passed. No package was uploaded.
- Focused tests passed: 8 core tests and 3 CLI tests. The selected-episode PGC proxy mock download transferred 64 KiB and matched the expected bytes; this is mock coverage, not a real PGC download.
- Bounded current-source CLI metadata probes passed for both timetable kinds with explicit official `pgc_base`, a 10-second timeout, preflight disabled, and no credentials. Anime returned 16 `latest` rows, 7 days, and 26 timeline rows (42 total); Chinese animation returned 16 `latest` rows, 7 days, and 23 timeline rows (39 total). Each command exited 0. Bounded logs were 20,416 and 18,305 bytes respectively (131,072-byte cap per log). These probes establish anonymous timetable metadata only; they did not call live playurl or download real PGC media.
- The separate upstream probe recorded above was an anonymous HTTP 200 / API code 0 observation at `2026-10-19T07:26:12Z`; it is historical API evidence, distinct from current-source CLI validation.
- Project-journal validation passed. `git diff --check` exited 0 with no output.

## Evidence

- Feature priority and endpoint-capability requirement: `2026-10-11-page-family-roadmap.md`.
- Related preceding slices: `2026-10-18-pgc-season-grouping-01a0dd39.md` and `2026-10-18-video-search-01a0dd39.md`.

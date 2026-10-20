---
id: 20261011-page-family-roadmap
title: Page Family and Season Grouping Roadmap
status: active
created: 2026-10-11
updated: 2026-10-20
branch:
pr:
supersedes: []
superseded_by:
---

# Page Family and Season Grouping Roadmap

## Summary

- Record Joey's page-family priorities and the shared requirement to use explicitly configured API services only for endpoint capabilities they support.
- Preserve PGC season section titles and grouping before ordinary video search; keep search metadata selection separate from selected-video resolution and planning.

## Current State

- Domestic PGC season resolution preserves main and supplementary groups in `SeasonMetadata.episode_groups`, including section titles, available upstream IDs, empty groups, duplicate titles, and membership positions in the unchanged flat episode list. See `2026-10-18-pgc-season-grouping-01a0dd39.md`.
- Ordinary video keyword search returns one bounded metadata page through a dedicated API and CLI surface. Generic input requires explicit selection from that page before selected results hydrate through the ordinary video resolver. See `2026-10-18-video-search-01a0dd39.md`.
- Anime and Chinese-animation PGC timetable metadata is available through one bounded anonymous request and a dedicated CLI/Rust API. It preserves the upstream latest block and day groups without adding listing inputs or download behavior. Movie and TV category-index metadata is in `2026-10-19-pgc-season-index-01a0dd39.md`. See `2026-10-19-pgc-timetable-01a0dd39.md` for the timetable slice.
- PGC movie and TV category-index metadata is available through bounded anonymous list and filter-values APIs/commands. It does not add listing inputs, page hydration, or downloads. See `2026-10-19-pgc-season-index-01a0dd39.md`.
- One bounded anonymous page of popular-video metadata is available through `BiliClient::popular_videos` and `bbdown popular`. It does not add generic inputs, hydration, playurl resolution, or media downloads. See `2026-10-19-popular-videos-01a0dd39.md`.
- Bounded anonymous global ranking metadata is available through `BiliClient::ranking_videos` and `bbdown ranking`, with fixed `rid=0&type=all&web_location=333.934` parameters. It may fetch public WBI keys with one anonymous navigation request before one signed ranking request; each response is capped at 1 MiB, with a 100-raw-row hard cap. Missing, null, empty, and whitespace-only BVID values remain absent. It does not add generic inputs, hydration, playurl resolution, or media downloads. See `2026-10-20-ranking-videos-01a0dd39.md`.
- Intl modules and PUGV courses retain their existing flat metadata behavior; this domestic PGC section slice does not infer groups for those sources.
- Standard UGC collections, series, favorites, and medialist inputs already have collection resolution and planning paths. Ordinary BV input keeps its single-video behavior; membership expansion is explicit.
- Existing `PgcWebPlayurlRoute` controls PGC Web playurl requests only. It does not route season metadata or page listings through the restricted-area resolver chain.
- PGC metadata requests use the configurable `EndpointConfig.pgc_base`; changing an endpoint base alone does not establish that a resolver supports its metadata endpoints, response shape, or regional access.

## Ordered Backlog

1. **Completed: preserve domestic PGC season grouping.** Main episodes and supplementary sections retain their titles, upstream order, episode membership, and available stable identifiers through the public API and CLI metadata output. Existing flat episode order, index selection, exact episode selection, and the main-episode-only `Latest` behavior remain compatible.
2. **Completed: add ordinary video search results.** Provide one bounded upstream page at a time, ordering, metadata output, and explicit result selection before ordinary video resolution, planning, or download.
3. **Completed: PGC listings.** Anime and Chinese-animation timetable metadata is in `2026-10-19-pgc-timetable-01a0dd39.md`; bounded movie and TV category-index metadata is in `2026-10-19-pgc-season-index-01a0dd39.md`. Both remain metadata-only and separate from generic inputs, planning, and downloads.
4. **Completed: popular video metadata.** Fetch one bounded anonymous page through the dedicated API and CLI command.
5. **Completed: global ranking metadata.** Fetch one bounded anonymous response through the dedicated API and CLI command, with fixed full-site parameters and no pagination or category/type configuration.
6. **Category and channel lists.** These are the next page-family priority.
7. **Topic pages.** Schedule them after category and channel lists.

- Standalone dynamic detail pages remain a separate candidate without a selected priority.

## Shared Reverse Resolver Requirement

- Aim to support caller-selected API services for existing season/collection/list pages and the new page families only where their endpoint capabilities permit it, so metadata enumeration as well as playback resolution can work when official regional access is unavailable.
- Cover metadata and listing endpoints explicitly; do not assume the existing PGC playurl fallback also resolves season metadata, search, timetable, or category data. Ordinary video search uses `api_base` for `/x/web-interface/nav` and `/x/web-interface/wbi/search/type`; it does not inherit `PgcWebPlayurlRoute` behavior or forward configured credentials.
- Prefer reusing endpoint configuration, the bundled resolver catalog, and existing route conventions where compatible. Define service capabilities and caller-selectable routing; a service that only supports playurl must not be presented as a metadata/list resolver.
- Preserve required request parameters, signing, pagination, group titles, and episode identities when a service supports the route. Make unsupported endpoints and regional access failures visible in diagnostics.
- Keep credential requirements and forwarding scope explicit for each configured service. Resolver support depends on the service's endpoint coverage and access rights; it does not guarantee availability for every region or account.

## Validation Expectations

- Grouping: mock normal and supplementary sections, duplicate or empty titles, missing optional identifiers, and API/CLI serialization; verify existing selection and planning behavior remains compatible.
- Timetable: mock anime and Chinese-animation kind mapping, upstream ordering, empty day groups, optional episode IDs, aggregate bounds, anonymous metadata routing, and CLI JSON/human output. The endpoint provides one server-defined response without pagination.
- Movie/TV index: mock category mappings, category-specific sort support, opaque condition values, one-page continuation metadata, query limits, anonymous endpoint routing, and CLI JSON/human output. Listing rows do not promise a playable or downloadable season.
- Popular videos: mock requested page and page-size query parameters, one-request anonymous routing, response bounds, `data.no_more` continuation behavior, item parsing, and CLI JSON/human output. Verify there is no automatic pagination or detail/playurl/media request; the upstream does not provide a page echo, total, or fixed-order snapshot.
- Ranking: mock fixed `rid=0&type=all` anonymous routing, the 1 MiB response limit, rejection above 100 raw rows, optional note and item parsing, source-position one-based ranks, and CLI JSON/human output. Verify no pagination, configurable category/type, credential or resolver initialization, detail hydration, playurl, or image/media request; ordering and totals are not guaranteed.
- Search: mock filtering, ordering, bounded request behavior, and explicit result selection before selected video hydration. Search fetches one requested page per call, caps each response at 1 MiB and each page at 20 rows.
- Reverse resolvers: mock supported and unsupported endpoints, official/proxy routing, response compatibility, and failures; use opt-in live fixtures for exact configured service routes when execution is authorized.
- Synchronize English and Simplified Chinese user and embedding documentation with each implementation slice.

## Evidence

- Joey's 2026-10-11 instruction places season grouping before search, anime timetable before film/TV category listings, and topic pages after category/channel lists.
- The bounded global ranking slice is recorded in `2026-10-20-ranking-videos-01a0dd39.md`; it does not change the next category/channel and topic priorities.
- Current implementation references: `crates/bbdown/src/client.rs` (`PgcWebPlayurlRoute`, search API, and PGC season methods) and `crates/bbdown/src/models.rs` (`SeasonMetadata`).
- The domestic PGC grouping implementation and its validation are recorded in `2026-10-18-pgc-season-grouping-01a0dd39.md`; ordinary search is recorded in `2026-10-18-video-search-01a0dd39.md`; timetable metadata is recorded in `2026-10-19-pgc-timetable-01a0dd39.md`; movie/TV category-index metadata is recorded in `2026-10-19-pgc-season-index-01a0dd39.md`. Broader endpoint-specific API service support remains capability-dependent.

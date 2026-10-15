---
id: 20261011-page-family-roadmap
title: Page Family and Season Grouping Roadmap
status: active
created: 2026-10-11
updated: 2026-10-11
branch:
pr:
supersedes: []
superseded_by:
---

# Page Family and Season Grouping Roadmap

## Summary

- Record Joey's page-family priorities and the shared requirement to support explicitly configured reverse API resolver services where their endpoint capabilities permit it.
- Preserve PGC season section titles and grouping before adding ordinary video search results.

## Current State

- Domestic PGC season resolution combines the main episode list and every returned section's episodes into one flat list. It retains the main episode count but discards section titles and grouping.
- Standard UGC collections, series, favorites, and medialist inputs already have collection resolution and planning paths. Ordinary BV input keeps its single-video behavior; membership expansion is explicit.
- Existing `PgcWebPlayurlRoute` controls PGC Web playurl requests only. It does not route season metadata or page listings through the restricted-area resolver chain.
- PGC metadata requests use the configurable `EndpointConfig.pgc_base`; changing an endpoint base alone does not establish that a resolver supports its metadata endpoints, response shape, or regional access.

## Ordered Backlog

1. **Preserve PGC season grouping.** Retain main episodes and supplementary sections with their titles, upstream order, episode membership, and available stable identifiers. Expose the structure through the public API and CLI metadata output. Preserve existing flat episode order, index selection, exact episode selection, and the main-episode-only `Latest` behavior for compatibility.
2. **Add ordinary video search results.** Define explicit input, pagination, ordering, selection, and bounded metadata behavior on top of the shared feed/list layer.
3. **Add the new-season anime timetable and film/TV category listings.** Treat these as listing inputs distinct from a single `ss`/`md` season page. Define their upstream listing scope and continuation behavior before implementation.
4. **Popular and ranking feeds.** These are not urgent.
5. **Category and channel lists.** These have lower priority than the preceding items.
6. **Topic pages.** Schedule them after category and channel lists.

- Standalone dynamic detail pages remain a separate candidate without a selected priority.

## Shared Reverse Resolver Requirement

- Aim to support caller-selected reverse API resolver services for existing season/collection/list pages and the new page families, so metadata enumeration as well as playback resolution can work when official regional access is unavailable.
- Cover metadata and listing endpoints explicitly; do not assume the existing PGC playurl fallback also resolves season metadata, search, timetable, or category data.
- Prefer reusing endpoint configuration, the bundled resolver catalog, and existing route conventions where compatible. Define service capabilities and caller-selectable routing; a service that only supports playurl must not be presented as a metadata/list resolver.
- Preserve required request parameters, signing, pagination, group titles, and episode identities when a service supports the route. Make unsupported endpoints and regional access failures visible in diagnostics.
- Keep credential requirements and forwarding scope explicit for each configured service. Resolver support depends on the service's endpoint coverage and access rights; it does not guarantee availability for every region or account.

## Validation Expectations

- Grouping: mock normal and supplementary sections, duplicate or empty titles, missing optional identifiers, and API/CLI serialization; verify existing selection and planning behavior remains compatible.
- New listings: mock pagination, filtering, ordering, continuation, and bounded request behavior.
- Reverse resolvers: mock supported and unsupported endpoints, official/proxy routing, response compatibility, and failures; use opt-in live fixtures for exact configured service routes when execution is authorized.
- Synchronize English and Simplified Chinese user and embedding documentation with each implementation slice.

## Evidence

- Joey's 2026-10-11 instruction places season grouping before search, the anime timetable and film/TV category listings after search, and topic pages after category/channel lists.
- Current implementation references: `crates/bbdown/src/client.rs` (`PgcWebPlayurlRoute`, `fetch_season_by_ep`, `fetch_season_by_season_id`, `fetch_season_by_media_id`, and `season_from_pgc`) and `crates/bbdown/src/models.rs` (`SeasonMetadata`).
- This update records requirements and priority only; it does not claim new implementation or test execution.

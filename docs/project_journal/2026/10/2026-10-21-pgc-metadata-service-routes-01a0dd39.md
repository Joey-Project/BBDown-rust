---
id: 20261021-pgc-metadata-service-routes-01a0dd39
title: Capability-Declared PGC Metadata Service Routes
status: completed
created: 2026-10-21
updated: 2026-10-22
branch: codex/pgc-metadata-service-routes
pr:
supersedes: []
superseded_by:
---

# Capability-Declared PGC Metadata Service Routes

## Scope
- Add caller-selected service-only routing for domestic PGC season hydration and media-review hydration.
- Keep the existing configured `pgc_base` route as the default. Keep `PgcWebPlayurlRoute` limited to PGC Web playurl.
- Expose `PgcMetadataCapability::{Season, MediaReview}`, `PgcMetadataService`, and `PgcMetadataRoute::{Configured, ServiceOnly}`. The default decoded response-body cap is 8 MiB; a custom limit must be positive.
- Require both `Season` and `MediaReview` capabilities up front for `md<ID>`; `ep<ID>` and `ss<ID>` require `Season`. Missing capabilities fail before HTTP.
- Add paired CLI options `--pgc-metadata-service-base` and `--pgc-metadata-service-capabilities`; the optional response-byte limit requires this pair.
- Preserve existing grouping, ordering, and `Latest` selection semantics when the service returns compatible payloads.

## Current State
- Bilingual README and embedding documentation describe the API, CLI configuration, endpoint capability contract, anonymous requests, redirect and fallback behavior, and per-response decoded-body limit.
- The original service slice covers `/pgc/view/web/season` and `/pgc/review/user`. The later independent `Timetable` capability is recorded in [the timetable service follow-up](2026-10-22-pgc-timetable-service-route-01a0dd39.md). Neither slice establishes support for index listings, search, PUGV, international PGC, playurl, or media. The original implementation gate did not verify a compatible external service; subsequent anonymous season/review CLI acceptance is recorded in [the October 22 follow-up](2026-10-22-pgc-metadata-live-acceptance-01a0dd39.md).
- Full `just ci` passed on the final fixture-adjusted tree in 231.03 seconds, following the earlier implementation gate in 93.285 seconds. The stable toolchain was Rust 1.99.1 / Cargo 1.99.0; the explicit Rust 1.95.0 MSRV check, formatting, workspace all-target Clippy with `-D warnings`, and publish dry run also passed. No publication occurred.
- Workspace test segments totaled 992 passed, zero failed, and four ignored (67 + 199 + 13 + 684 + 21 + 2 + 0 + 6, including six public API integration tests). The CLI E2E suite's 199 passed tests are already included in this total; its separate recipe rerun is not added again. Additional focused gates passed: seven core tests and two CLI subprocess tests.
- Attempt 1 stopped at Clippy on an unused `self`, a test `expect_err` lint, and an existing test-length threshold. Those issues were corrected before the successful full CI rerun.
- Two existing QR poll tests now load the catalog-approved `refresh-a` value from a native JSON fixture and insert it into their mock response data. Their successful URL fallback and missing-CSRF assertions are preserved; both exact tests passed again. This makes the fixture value available to exact secret admission without changing production login behavior or scanner policy. A sandboxed gate attempt stopped at Cargo build-cache permissions before tests; the bounded nonsandbox gate above passed.
- No metadata-service live test was run during the original implementation gate. The October 22 follow-up records bounded `ep`/`ss`/`md` CLI acceptance against the official API and `atri.ink`, with the exact tested capabilities and limitations. No publish or deployment was performed.

## Next Steps
- Broader service routing remains separate work: movie/TV index, ordinary search, PUGV, and international PGC continue using existing endpoint configuration. Timetable service routing requires its independent `Timetable` capability; do not infer it from `Season` or `MediaReview`.
- The page-family roadmap's next priorities remain category and channel lists, followed by topic pages. Add service capabilities only in separately scoped slices that declare and validate their exact endpoints and response shapes.

## Evidence
- Public API and routing guide: `docs/embedding.md` and `docs/embedding.zh-CN.md`.
- CLI overview: `README.md` and `README.zh-CN.md`.
- Page-family scope: `2026-10-11-page-family-roadmap.md`.

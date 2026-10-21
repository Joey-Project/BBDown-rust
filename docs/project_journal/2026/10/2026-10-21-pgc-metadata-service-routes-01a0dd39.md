---
id: 20261021-pgc-metadata-service-routes-01a0dd39
title: Capability-Declared PGC Metadata Service Routes
status: completed
created: 2026-10-21
updated: 2026-10-21
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
- The new service route only covers `/pgc/view/web/season` and `/pgc/review/user`. It does not establish support for timetable, index listings, search, PUGV, international PGC, playurl, or media. No compatible external service or live behavior is claimed as verified.
- Full `just ci` passed on the final fixture-adjusted tree in 231.03 seconds, following the earlier implementation gate in 93.285 seconds. The stable toolchain was Rust 1.99.1 / Cargo 1.99.0; the explicit Rust 1.95.0 MSRV check, formatting, workspace all-target Clippy with `-D warnings`, and publish dry run also passed. No publication occurred.
- Workspace test segments totaled 992 passed, zero failed, and four ignored (67 + 199 + 13 + 684 + 21 + 2 + 0 + 6, including six public API integration tests). The CLI E2E suite's 199 passed tests are already included in this total; its separate recipe rerun is not added again. Additional focused gates passed: seven core tests and two CLI subprocess tests.
- Attempt 1 stopped at Clippy on an unused `self`, a test `expect_err` lint, and an existing test-length threshold. Those issues were corrected before the successful full CI rerun.
- Two existing QR poll tests now load the catalog-approved `refresh-a` value from a native JSON fixture and insert it into their mock response data. Their successful URL fallback and missing-CSRF assertions are preserved; both exact tests passed again. This makes the fixture value available to exact secret admission without changing production login behavior or scanner policy. A sandboxed gate attempt stopped at Cargo build-cache permissions before tests; the bounded nonsandbox gate above passed.
- No compatible metadata-service live test was run. Service compatibility beyond the mock-tested endpoint and capability contract remains unverified. No publish or deployment was performed.

## Next Steps
- Broader service routing remains separate work: PGC timetable and movie/TV index, ordinary search, PUGV, and international PGC continue using existing endpoint configuration. Do not infer support for them from `Season` or `MediaReview`.
- The page-family roadmap's next priorities remain category and channel lists, followed by topic pages. Add service capabilities only in separately scoped slices that declare and validate their exact endpoints and response shapes.

## Evidence
- Public API and routing guide: `docs/embedding.md` and `docs/embedding.zh-CN.md`.
- CLI overview: `README.md` and `README.zh-CN.md`.
- Page-family scope: `2026-10-11-page-family-roadmap.md`.

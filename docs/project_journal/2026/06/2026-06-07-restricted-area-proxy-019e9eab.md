---
id: 20260607-019e9eab-restricted-area-proxy
title: Restricted-Area Proxy Resolver Ordering And Diagnostics
status: completed
created: 2026-06-07
updated: 2026-06-07
branch: wip/restricted-area-proxy
pr: https://github.com/Joey-Project/BBDown-rust/pull/5
supersedes: []
superseded_by:
---

# Restricted-Area Proxy Resolver Ordering And Diagnostics

## Summary

- Fifth PR slice for the BBDown Rust rewrite.
- Scope is configured PGC playurl proxy fallback, deterministic candidate ordering, typed resolver
  diagnostics, CLI flags/env, mock e2e coverage, and docs.
- The slice keeps public proxy hosts out of the codebase; callers must provide their own proxy
  endpoints.

## Current State

- `ClientConfig::restricted_area` exposes a per-client `RestrictedAreaConfig` with an optional area
  hint and configured proxy candidates.
- `RestrictedAreaProxy` supports BBDown/BiliPlus-style playurl proxy endpoints and proxies that
  mirror `api.bilibili.com` path layout. API-style proxy base query parameters are preserved before
  PGC playurl query parameters are appended.
- Candidate ordering is hint match first, generic candidates, then `cn`, `th`, `hk`, and `tw`, with
  duplicate configured candidates removed.
- PGC stream planning tries the official PGC web playurl endpoint first. If it reports a region/area
  restriction and proxy candidates are configured, the client tries the ordered proxy chain until one
  returns a usable DASH or FLV stream shape. Non-area official failures keep their original error.
- Proxy requests include the generic `Credentials::access_key` when present. The TV-specific access
  key is not reused for PGC proxy fallback, and Bilibili cookies are not forwarded to proxy hosts.
- Proxy fallback success changes `DownloadEntry.source` to `PgcProxy` and adds
  `DownloadEntry.diagnostics` with the official failed attempt and proxy attempt metadata.
- Resolver diagnostic endpoint fields are reduced to URL origins, so path/query/userinfo secrets are
  not printed. Diagnostic error messages also redact common sensitive key-value patterns before JSON
  output or final access-restricted errors expose them.
- CLI flags:
  - `--restricted-area <cn|th|hk|tw>`
  - `--restricted-area-proxy [AREA=]URL`
  - `--restricted-api-proxy [AREA=]URL`
- Environment support:
  - `BBDOWN_RESTRICTED_AREA`
  - comma-separated `BBDOWN_RESTRICTED_AREA_PROXY`
  - comma-separated `BBDOWN_RESTRICTED_API_PROXY`
- Mock e2e coverage verifies official PGC failure followed by local proxy success, confirms JSON
  output does not leak configured access keys or cookies, and asserts proxy requests omit Cookie.
- CLI unit coverage verifies cross-flag declaration order for proxy candidates and redacted URL parse
  errors, uppercase URL schemes, CLI/env proxy merging, and non-UTF-8 non-proxy argv values.
- Review fixes tightened fallback eligibility so non-area `403`/`-40301` errors keep their original
  failure, and invalid proxy URL diagnostics now drop path/query/userinfo even for crate-level
  public API inputs that bypass CLI URL validation.
- Follow-up review fixes made diagnostic URL scanning case-insensitive for mixed-case schemes and
  made CLI/env proxy source priority explicit before area-hint grouping.
- CLI proxy parsing now treats URL-like `scheme://` inputs as URLs before `area=` parsing, rejects
  non-HTTP(S) schemes through the redacted URL error path, and covers typo-scheme leak regressions.
- Final review fix keeps environment playurl proxies ahead of environment API-path proxies even when
  an area-matched API proxy and a generic playurl proxy are configured together.
- App-only/mobile proxy response conversion remains out of scope for this slice.

## Evidence

- Targeted crate tests: `cargo test -p bbdown restricted_area`.
- Targeted CLI unit test: `cargo test -p bbdown-cli restricted_area_cli_builds_proxy_chain`.
- Targeted mock e2e test: `cargo test -p bbdown-cli --test cli_e2e plan_json_uses_restricted_area_proxy_after_official_pgc_failure`.
- Targeted CLI proxy tests: `cargo test -p bbdown-cli restricted_area_proxy`.
- Local gate: `just ci` with formatter check, clippy, 86 library tests, 17 CLI unit tests, 8 mock
  CLI e2e tests, and 2 ignored live e2e tests in the default workspace test run.
- Live gate env preflight: `just live-e2e` without `BBDOWN_LIVE_URL` exits with code 2 before
  running tests.
- Project journal validation: `python3 /Users/joey/.codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
- Whitespace check: `git diff --check`.

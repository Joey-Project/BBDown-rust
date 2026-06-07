---
id: 20260607-019e9eab-restricted-area-proxy
title: Restricted-Area Proxy Resolver Ordering And Diagnostics
status: completed
created: 2026-06-07
updated: 2026-06-07
branch: wip/restricted-area-proxy
pr:
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
  errors.
- App-only/mobile proxy response conversion remains out of scope for this slice.

## Evidence

- Targeted crate tests: `cargo test -p bbdown restricted_area`.
- Targeted CLI unit test: `cargo test -p bbdown-cli restricted_area_cli_builds_proxy_chain`.
- Targeted mock e2e test: `cargo test -p bbdown-cli --test cli_e2e plan_json_uses_restricted_area_proxy_after_official_pgc_failure`.
- Local gate: `just ci` with formatter check, clippy, 84 library tests, 10 CLI unit tests, 8 mock
  CLI e2e tests, and 2 ignored live e2e tests in the default workspace test run.
- Live gate env preflight: `just live-e2e` without `BBDOWN_LIVE_URL` exits with code 2 before
  running tests.
- Project journal validation: `python3 /Users/joey/.codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
- Whitespace check: `git diff --check`.

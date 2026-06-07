---
id: 20260607-019e9eab-live-e2e-matrix
title: Live E2E Sample Matrix
status: completed
created: 2026-06-07
updated: 2026-06-07
branch: wip/live-e2e-matrix
pr: https://github.com/Joey-Project/BBDown-rust/pull/7
supersedes: []
superseded_by:
---

# Live E2E Sample Matrix

## Summary

- Seventh PR slice for the BBDown Rust rewrite continuation track.
- Scope is a local-only live e2e matrix driven by an ignored JSON manifest instead of ad hoc
  environment variables.
- The slice uses Joey-provided public and region-limited samples while keeping cookie and access-key
  values out of tracked files.

## Current State

- `just live-e2e` now requires a local `live-e2e.samples.json` and runs the ignored
  `bbdown-cli` live e2e test only when that manifest exists.
- `live-e2e.samples.example.json` records the tracked manifest shape for normal public, restricted
  PGC Hong Kong/Macau/Taiwan, and restricted PGC Mainland samples.
- The local manifest can point to a credential JSON file and an `access_key_file`; the harness
  copies only recognized cookie/access-key fields into an isolated temporary credential file per
  case.
- Telegram Video Downloader's `bilibili-auth.json` shape is accepted because it contains a `cookie`
  field; `access_key.txt` remains ignored.
- Cases can run `info`, `plan`, or both; set `selection`; set a restricted-area hint; configure
  per-case or manifest-level restricted proxy candidates; and assert JSON info kind, allowed or
  required plan sources, minimum entries, and stream presence.
- Restricted `plan` cases can explicitly set `allow_plan_error` and declare expected diagnostic
  fragments. This lets the local gate accept either a successful proxy/official stream plan or a
  deterministic access-restricted failure that proves the official and proxy resolver attempts were
  exercised without accepting unrelated plan failures.
- `restricted_api_proxy_all_areas` and `restricted_area_proxy_all_areas` expand each configured URL
  into `cn`, `th`, `hk`, and `tw` proxy specs.
- The live harness removes CLI override environment variables before each command so manifest data,
  not inherited shell state, controls the run.
- API-path restricted proxy fallback now tries `/pgc/player/web/playurl` below the proxy base first,
  matching the BALH-style host-only proxy path used by `https://atri.ink`, and then falls back to
  `/pgc/player/web/v2/playurl` for existing API proxies. The official PGC playurl request remains on
  the upstream v2 endpoint.

## Evidence

- Targeted live harness tests: `cargo test -p bbdown-cli --test live_e2e`.
- Targeted API proxy URL and compatibility tests: `cargo test -p bbdown pgc_bilibili_api_proxy`.
- Local live gate: `just live-e2e` with the ignored manifest containing Joey's public sample,
  Hong Kong/Macau/Taiwan-restricted PGC sample, Mainland-restricted PGC sample, Telegram Video
  Downloader credential path, `access_key.txt`, and `https://atri.ink`.
- Local default gate: `just ci` with formatter check, clippy, 88 library tests, 19 CLI unit tests,
  8 mock CLI e2e tests, 4 live harness unit tests, and 1 ignored live case in the default workspace
  test run.

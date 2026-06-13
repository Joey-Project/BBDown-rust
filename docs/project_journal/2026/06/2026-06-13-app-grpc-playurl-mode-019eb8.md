---
id: 20260613-019eb8-app
title: APP gRPC Playurl Mode
status: completed
created: 2026-06-13
updated: 2026-06-13
branch: wip/app-grpc-playurl-mode
pr: 31
supersedes: []
superseded_by:
---

# APP gRPC Playurl Mode

## Summary
- Added BBDown-compatible APP gRPC playurl resolution without changing the default web playurl
  behavior.
- Kept the protobuf transport internal to `bbdown-core`; downstream users select it through the
  existing `PlayurlMode` API and receive normalized `StreamSet` / `PlaybackPlan` output.

## Current State
- `ClientConfig::with_playurl_mode(PlayurlMode::App)` selects APP gRPC playurl mode.
- `EndpointConfig::with_app_grpc_base` configures the normal-video APP gRPC host for mocks,
  proxies, or the upstream default.
- `EndpointConfig::with_app_pgc_grpc_base` configures the PGC APP gRPC host for mocks, proxies, or
  the upstream default.
- Normal video planning can now emit `StreamSource::NormalApp`.
- PGC episode planning can now emit `StreamSource::PgcApp`.
- CLI callers can use `--playurl-mode app`, `--app-grpc-base`, and `--app-pgc-grpc-base`;
  environment overrides are `BBDOWN_PLAYURL_MODE`, `BBDOWN_APP_GRPC_BASE`, and
  `BBDOWN_APP_PGC_GRPC_BASE`.
- APP playurl requests use `Credentials::tv_access_key` first and fall back to
  `Credentials::access_key`; WEB cookies are not sent to APP gRPC endpoints.
- APP gRPC non-zero `grpc-status` responses are surfaced as API errors with decoded
  `grpc-message` text.
- PGC APP region-limit errors can fall back to the existing restricted-area HTTP playurl proxy
  resolver and record `PgcApp` then `PgcProxy` diagnostics.

## Next Steps
- Continue feed/list resolver work for history, following/UP pages, recommendation pages, and
  watch-later.
- Consider a later request-level APP codec preference API if downstreams need to influence the
  protobuf `preferCodecType` field before response-time playback selection.

## Evidence
- Targeted validation: `cargo test -p bbdown-core app_playurl --lib`.
- Targeted validation: `cargo test -p bbdown-core app_grpc_status_error_is_reported --lib`.
- Targeted validation:
  `cargo test -p bbdown-core pgc_app_streams_fall_back_to_restricted_area_proxy --lib`.
- Targeted validation: `cargo test -p bbdown-cli --test cli_e2e playback_json_uses_app_playurl_mode`.
- Full validation: `just ci`.
- Journal validation:
  `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/29f61f3e579e2a4166436b963eab301ac5d80d94/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
- Internal review fix: helper-backed `codex-readonly` found that APP gRPC requests need reqwest
  HTTP/2 support when default reqwest features are disabled; fixed by enabling the `http2` feature
  in the workspace reqwest dependency and rerunning `just ci`.
- Independent review fix: the PR readiness `independent-codex-pr-review` found that APP PGC
  region-limit failures bypassed restricted-area proxy fallback and that non-zero gRPC status
  headers were not reported clearly; fixed both paths and added regression tests.

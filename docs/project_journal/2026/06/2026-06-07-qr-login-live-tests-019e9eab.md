---
id: 20260607-019e9eab-qr-login-live-tests
title: QR Login And Live Test Harness
status: completed
created: 2026-06-07
updated: 2026-06-07
branch: wip/qr-login-live-tests
pr:
supersedes: []
superseded_by:
---

# QR Login And Live Test Harness

## Summary

- Fourth PR slice for the BBDown Rust rewrite.
- Scope is WEB/TV QR login state-machine support, CLI credential-store updates, passport endpoint
  overrides for mock tests, and an opt-in live e2e harness.
- Restricted-area proxy resolver ordering and diagnostics remain the next planned slice.

## Current State

- `EndpointConfig` includes `passport_base`, `tv_passport_base`, and `tv_passport_poll_base` so QR
  login endpoints can be mocked or routed through controlled proxies while preserving the upstream
  TV split-host default.
- The crate exposes `QrLoginTicket`, `QrLoginKind`, and `QrLoginState`.
- `BiliClient::create_web_qr_login` and `BiliClient::poll_web_qr_login` model the WEB QR flow:
  waiting for scan, waiting for confirmation, expired, and succeeded cookie credential.
- `BiliClient::create_tv_qr_login` and `BiliClient::poll_tv_qr_login` model the TV QR flow using
  BBDown-compatible signed app parameters, waiting-for-scan, waiting-for-confirmation, expired, and
  succeeded TV-specific access-key credential output. TV QR tickets retain the generated device
  session context so polling reuses the same device identity.
- QR login HTTP requests use anonymous headers even when the caller has stored credentials.
- QR ticket debug output is redacted because ticket keys and scan URL query strings can act as
  pre-authentication secrets.
- CLI `auth login-web` and `auth login-tv` print the scan URL in human mode, poll with
  deadline-based timeout handling, and save resulting credentials without printing token values.
- QR login JSON output is newline-delimited event output: `ticket` exposes the scan URL before
  polling and must be treated as a temporary login secret, while `saved` reports only redacted
  credential-presence booleans.
- QR login saves reload the current credential store after scan success before merging returned
  credentials, so long QR waits do not overwrite another command's fresh credential update with a
  stale pre-wait snapshot. TV QR tokens are stored separately from the generic intl/Bstar
  `access_key` because app tokens are appkey-bound.
- CLI mock e2e coverage verifies WEB QR cookie import and TV QR access-key import through a local
  credential file without overwriting a generic access key, plus expired and hung-poll failure paths
  that do not save credentials.
- `crates/bbdown-cli/tests/live_e2e.rs` contains ignored opt-in live tests for `info --json` and
  `plan --json` using `BBDOWN_LIVE_URL`, optional `BBDOWN_LIVE_SELECTION`, `BBDOWN_LIVE_COOKIE`,
  and `BBDOWN_LIVE_ACCESS_KEY`.
- `just live-e2e` fails fast unless `BBDOWN_LIVE_URL` is set, then runs the ignored live test
  target. Default CI remains formatter, clippy, unit tests, and mock e2e tests only.

## Next Steps

- Add restricted-area proxy resolver ordering and diagnostics.

## Evidence

- Targeted QR unit tests: `cargo test -p bbdown login`.
- Targeted mock e2e tests: `cargo test -p bbdown-cli --test cli_e2e auth_qr_login`.
- Targeted timeout helper test: `cargo test -p bbdown-cli next_poll_sleep_caps_interval_by_deadline`.
- Targeted credential merge test: `cargo test -p bbdown-cli save_qr_credentials_merges_with_current_store`.
- Local gate: `just ci` with 77 library tests, 3 CLI unit tests, 7 mock CLI e2e tests, and 2
  ignored live e2e tests in the default workspace test run.
- Live gate env preflight: `just live-e2e` without `BBDOWN_LIVE_URL` exits with code 2 before
  running tests.

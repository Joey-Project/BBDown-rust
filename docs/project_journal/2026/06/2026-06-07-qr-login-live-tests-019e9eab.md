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
  succeeded access-key credential output.
- QR login HTTP requests use anonymous headers even when the caller has stored credentials.
- CLI `auth login-web` and `auth login-tv` print the scan URL in human mode, poll with
  deadline-based timeout handling, and save resulting credentials without printing token values.
- QR login JSON output is newline-delimited event output: `ticket` exposes the scan URL before
  polling, while `saved` reports only `has_cookie` and `has_access_key` booleans.
- CLI mock e2e coverage verifies WEB QR cookie import and TV QR access-key import through a local
  credential file.
- `crates/bbdown-cli/tests/live_e2e.rs` contains ignored opt-in live tests for `info --json` and
  `plan --json` using `BBDOWN_LIVE_URL`, optional `BBDOWN_LIVE_SELECTION`, `BBDOWN_LIVE_COOKIE`,
  and `BBDOWN_LIVE_ACCESS_KEY`.
- `just live-e2e` runs the ignored live test target; default CI remains formatter, clippy, unit
  tests, and mock e2e tests only.

## Next Steps

- Add restricted-area proxy resolver ordering and diagnostics.

## Evidence

- Targeted QR unit tests: `cargo test -p bbdown login`.
- Targeted mock e2e test: `cargo test -p bbdown-cli --test cli_e2e auth_qr_login_web_and_tv_use_local_store`.
- Targeted timeout helper test: `cargo test -p bbdown-cli next_poll_sleep_caps_interval_by_deadline`.
- Local gate: `just ci` with 75 library tests, 1 CLI unit test, 6 mock CLI e2e tests, and 2
  ignored live e2e tests in the default workspace test run.
- Opt-in harness smoke without live env: `just live-e2e`.

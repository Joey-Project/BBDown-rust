---
id: 20260615-019ed0-login-qr-output-model
title: Login QR Output Model
status: completed
created: 2026-06-15
updated: 2026-06-15
branch: wip/login-qr-output-model
pr:
supersedes: []
superseded_by:
---

# Login QR Output Model

## Summary
- This is PR 2 in the `0.4.0` credential and danmaku sequence.
- The slice narrows to unified QR login ticket/output surfaces for existing WEB and TV QR login
  flows.
- Credential health diagnostics, profile support, generic `access_key` acquisition, and append-only
  danmaku updates remain separate follow-up PRs in the eight-PR sequence.

## Current State
- `QrLoginTicket` now exposes a stable `QrLoginTicketOutput` conversion for serialized scan URL and
  QR payload output.
- CLI JSON `ticket` events keep the existing top-level `url` field and add `qr_payload`.
- Current WEB and TV login flows use the scan URL itself as the QR payload.
- QR ticket output debug formatting is redacted so scan URL query strings are not leaked through
  ordinary debug logs.

## Next Steps
- Cut PR 3 from updated `master` for credential health-check diagnostics.

## Evidence
- Targeted core login tests:
  `cargo test -p bbdown-core login::tests --locked`.
- Targeted CLI QR login e2e tests:
  `cargo test -p bbdown-cli auth_qr_login --test cli_e2e --locked`.
- Targeted CLI auth status/import/logout e2e test:
  `cargo test -p bbdown-cli auth_import_status_and_logout_use_local_store --test cli_e2e --locked`.

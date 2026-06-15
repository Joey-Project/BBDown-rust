---
id: 20260615-019ed1-credential-health-diagnostics
title: Credential Health Diagnostics
status: completed
created: 2026-06-15
updated: 2026-06-15
branch: wip/credential-health-diagnostics
pr: https://github.com/Joey-Project/BBDown-rust/pull/44
supersedes: []
superseded_by:
---

# Credential Health Diagnostics

## Summary
- Adds a read-only credential health surface for WEB cookie, generic `access_key`, and TV
  `tv_access_key` credentials.
- Exposes `CredentialHealthReport` and `BiliClient::check_credential_health()` for embedding callers.
- Adds `bbdown auth health [--json]` for CLI diagnostics without printing raw credential values.

## Current State
- WEB cookie health is checked through the web nav endpoint.
- Generic `access_key` and TV `tv_access_key` health are checked through OAuth token info using a
  signed `access_key` app query and without sending cookies.
- Probe reports include `kind` for the credential slot and `scope` for the checked consumer; the
  generic `access_key` probe currently covers intl/Bstar health only.
- Generic token probes use the configured `passport_base`; TV token probes use the configured
  `tv_passport_poll_base`.
- Each probe reports `missing`, `valid`, `rejected`, or `request_failed`; API messages are sanitized
  before serialization.

## Next Steps
- After this PR merges, cut PR 4 from updated `master` for credential profile storage while
  preserving default-profile behavior.

## Evidence
- Targeted core health tests cover missing credentials, valid probes, rejected token probes, request
  shape, and report redaction.
- Targeted CLI e2e covers `auth health --json` with all three credential slots configured.

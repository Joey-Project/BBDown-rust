---
id: 20260614-019ec8-credential-lifecycle-roadmap
title: Credential Lifecycle Roadmap
status: active
created: 2026-06-14
updated: 2026-06-14
branch: wip/following-up-feed-list-parsing
pr:
supersedes: []
superseded_by:
---

# Credential Lifecycle Roadmap

## Summary
- Current credential storage supports three fields: WEB `cookie`, generic `access_key`, and TV
  `tv_access_key`.
- Users can obtain WEB cookie through `auth login-web` or `auth import-cookie`, and TV token through
  `auth login-tv`.
- Generic `access_key` is currently import-only through `auth import-access-key` or
  `BBDOWN_ACCESS_KEY`.

## Current State
- History and other WEB-only inputs depend on the stored WEB cookie.
- Restricted-area intl/Bstar and proxy flows use the generic `access_key`.
- TV playurl uses `tv_access_key`; APP/gRPC playurl prefers `tv_access_key` and falls back to the
  generic `access_key`.
- The project does not yet have credential health checks, automatic renewal, profile management, or
  in-project generic `access_key` acquisition.

## Next Steps
- Add credential health checks that can verify WEB cookie, generic `access_key`, and TV
  `tv_access_key` independently, with account metadata and expiration/error diagnostics where the
  upstream API exposes them.
- Add renewal or re-login guidance before attempting automatic refresh, so expired credentials do not
  cause destructive overwrites of still-valid stored secrets.
- Add multi-account profile support with a default profile that preserves current behavior, plus CLI
  and embedding API hooks for selecting profiles.
- Investigate generic `access_key` acquisition by studying the previous
  `https://github.com/JoeyTeng/bilibili-helper` approach. The remembered implementation used a
  biliplus.com-based flow; verify that behavior and current viability before designing a Rust
  implementation.

## Evidence
- `crates/bbdown/src/credentials.rs` defines `Credentials { cookie, access_key, tv_access_key }`.
- `crates/bbdown/src/login.rs` implements WEB QR login returning cookie credentials and TV QR login
  returning `tv_access_key`.
- `crates/bbdown-cli/src/main.rs` exposes `auth import-cookie`, `auth import-access-key`,
  `auth login-web`, `auth login-tv`, `auth status`, and `auth logout`.
- User request on 2026-06-14 asked to record future automatic renewal, health checks, multi-account
  management, and later investigation of the `bilibili-helper` / biliplus.com `access_key` flow.

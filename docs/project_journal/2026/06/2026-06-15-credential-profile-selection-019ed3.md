---
id: 20260615-019ed3-credential-profile-selection
title: Credential Profile Selection
status: completed
created: 2026-06-15
updated: 2026-06-15
branch: wip/credential-profile-selection
pr:
supersedes:
superseded_by:
---

# Credential Profile Selection

## Summary
- Adds a shared `CredentialProfileSelection` API for default-profile versus named-profile credential
  routing.
- Adds selected-profile load/save helpers on `CredentialStore` so embedding callers can reuse the
  same routing and migration behavior as the CLI.
- Adds global CLI `--credential-profile` / `BBDOWN_CREDENTIAL_PROFILE` support for info, plan,
  playback, download, and auth flows.
- Keeps legacy behavior when no profile is selected: `CredentialStore::load()` / `save()` stay on
  the default profile, and `auth logout` clears the whole store.
- When a named profile is selected, auth import, QR-login save, status, health, and logout operate on
  that profile only.

## Current State
- Named profile writes migrate legacy flat credential files to profile documents while preserving
  default credentials.
- `auth logout` removes only the selected named profile when `--credential-profile` is present.
- Resolver commands load credentials through the selected profile before building `ClientConfig`.

## Next Steps
- Cut PR 6 from updated `master` and add the core generic `access_key` acquisition flow after
  validating the historical BiliPlus URL/QR authorization behavior.

## Evidence
- Core credential tests cover selected-profile load/save helpers and blank profile rejection.
- CLI tests cover `--credential-profile` parsing, named-profile QR credential saves, auth
  import/status/logout isolation, and selected-profile use for watch-later metadata requests.

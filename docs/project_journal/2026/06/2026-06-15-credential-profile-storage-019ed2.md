---
id: 20260615-019ed2-credential-profile-storage
title: Credential Profile Storage
status: completed
created: 2026-06-15
updated: 2026-06-15
branch: wip/credential-profile-storage
pr: https://github.com/Joey-Project/BBDown-rust/pull/45
supersedes: []
superseded_by:
---

# Credential Profile Storage

## Summary
- Adds a versioned `CredentialProfiles` storage document for multiple named credential sets.
- Preserves the existing default `CredentialStore::load()` and `CredentialStore::save()` behavior
  for legacy flat credential files.
- Exposes profile load, save, and remove helpers for embedding callers without adding CLI profile
  selection yet.

## Current State
- New credential stores still write the legacy flat `Credentials` JSON shape when callers use
  `CredentialStore::save()`.
- `CredentialStore::load_profiles()` wraps legacy flat credentials as the `default` profile.
- Saving a named profile migrates a legacy flat store to the profile document and preserves default
  credentials.
- When the on-disk file is already a profile document, `CredentialStore::save()` updates the
  configured default profile and preserves other named profiles.
- Profile names are trimmed and blank names are rejected by profile-aware APIs.
- `Credentials` and `CredentialProfiles` debug output only expose redacted booleans.

## Next Steps
- Cut PR 5 from updated `master` and make CLI and embedding credential selection profile-aware.
- Keep generic `access_key` acquisition and append-only danmaku updates in later slices of the
  eight-PR `0.4.0` sequence.

## Evidence
- Core credential tests cover legacy wrapping, legacy write preservation, named-profile migration,
  default-profile loading, profile-name validation, file permission tightening, bare relative paths,
  legacy flat files with unrelated `version` or profile-like unknown fields, and redacted debug
  output.

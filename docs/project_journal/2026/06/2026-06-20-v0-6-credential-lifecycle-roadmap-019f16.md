---
id: 20260620-019f16-v0-6-credential-lifecycle-roadmap
title: v0.6.0 Credential Lifecycle Roadmap
status: active
created: 2026-06-20
updated: 2026-06-21
branch: feature/v0.6-credential-lifecycle-status
pr: 63
supersedes: []
superseded_by:
---

# v0.6.0 Credential Lifecycle Roadmap

## Summary

- `v0.5.0` shipped through the protected release candidate and promotion workflow as a GitHub
  Release and crates.io `bbdown-core` package.
- The next release line focuses on credential lifecycle behavior instead of downloader polish or
  broader page parsing.
- Existing credential storage supports WEB cookies, generic `access_key`, TV `tv_access_key`, named
  profiles, QR login ticket output, and read-only health diagnostics. Access-key login parsing can
  already capture `refresh_token`, `oauth_expires_at`, and `expires_in`, but the persisted
  `Credentials` model does not yet carry lifecycle metadata.

## Planned Slices

- PR 1: post-release housekeeping and this roadmap, marking `v0.5.0` complete and making the
  `v0.6.0` credential lifecycle line the active follow-up.
- PR 2: credential lifecycle metadata model. Add versioned, backward-compatible profile metadata for
  credential provenance, checked-at timestamps, expiry hints, refresh eligibility, and redacted
  summaries without leaking raw tokens.
- PR 3: lifecycle status and health policy API. Extend current health reports with profile-level
  lifecycle status, stale/expiring policy evaluation, and JSON-friendly output that embedders can use
  before planning or downloading.
- PR 4: CLI lifecycle UX. Add commands for profile status, default-profile visibility, selected/all
  profile health checks, and clear guidance when credentials require re-login rather than silent
  refresh.
- PR 5: access-key refresh or reacquisition orchestration. Persist safe refresh metadata where
  available, attempt refresh only when the source and stored metadata support it, and otherwise fall
  back to explicit QR/login renewal.
- PR 6: optional credential preflight integration for planning/downloading. Let CLI and embedding
  callers choose whether to fail fast, warn, or try renewal before restricted-area, TV, APP, or WEB
  request paths.
- PR 7: multi-account lifecycle polish. Support lifecycle reporting across profiles, selected-profile
  renewal, and non-destructive profile updates that avoid overwriting concurrently refreshed
  credentials.
- PR 8: release prep for `v0.6.0`, including bilingual docs, public API checks, deterministic CI,
  live-e2e notes, release notes, and protected RC readiness.

## Completed Slices

- PR 2 implemented the credential lifecycle metadata model:
  - Profile documents can now carry optional per-profile, per-credential-kind lifecycle metadata.
  - Metadata records track provenance, acquired/checked/expiry timestamps, and whether a refresh
    token is present without duplicating raw credential values.
  - Empty metadata is omitted, orphan metadata is normalized away on save, and legacy flat
    credential stores continue to load without lifecycle metadata.
  - Unknown or malformed optional metadata is ignored on load, while malformed required profile data
    still fails fast.
  - Replacing a credential value drops lifecycle metadata for that credential kind so old token
    expiry/source hints are not rebound to a new token.
- PR 3 implemented lifecycle status and health policy APIs:
  - `CredentialLifecyclePolicy` evaluates profile metadata with explicit `now_unix_millis`,
    stale-after, and expiring-within windows.
  - `CredentialProfiles::profile_lifecycle_status` and `lifecycle_statuses` expose redacted
    profile-level and per-credential status output for embedding UI and preflight decisions.
  - `CredentialHealthReport::summary` and `probe` add compact UI summaries while preserving exact
    per-kind probe inspection.
  - Embedding and architecture docs now describe the difference between local lifecycle evaluation
    and network-backed credential health probes.

## Out Of Scope For This Line

- Per-video related recommendations and additional Bilibili page-family parsing remain planned for
  `v0.7.0` or a later feed/page release.
- Downloader parity items such as aria2 or multi-thread download integration, MP4Box muxing, and
  subtitle-to-SRT conversion stay available for reprioritization.
- No credential command should silently upload, print, or log raw tokens; any refresh path must keep
  the current redaction behavior.

## Evidence

- GitHub Release `v0.5.0`: `https://github.com/Joey-Project/BBDown-rust/releases/tag/v0.5.0`.
- Release candidate workflow `27882887144` created `v0.5.0-rc.1`.
- Promotion workflow `27883242286` completed successfully from `v0.5.0-rc.1`.
- Published crate version: `bbdown-core` `0.5.0`.
- Current credential model: `crates/bbdown/src/credentials.rs`.
- Current login model: `crates/bbdown/src/login.rs`.
- PR 2 local validation:
  - `cargo test -p bbdown-core credentials --locked`.
  - `cargo test -p bbdown-core --test public_api --locked`.
  - `just ci`.
- PR 3 local validation:
  - `cargo test -p bbdown-core credentials --locked`.
  - `cargo test -p bbdown-core --test public_api --locked`.
  - `cargo clippy -p bbdown-core --all-targets --locked -- -D warnings`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/07780f1c323453fd738330fbf8fd70e2899d4409/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `just ci`.

## Next Steps

- Cut PR 4 from updated `master` for CLI lifecycle UX after PR 3 lands.

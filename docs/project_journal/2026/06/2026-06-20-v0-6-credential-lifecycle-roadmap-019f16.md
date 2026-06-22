---
id: 20260620-019f16-v0-6-credential-lifecycle-roadmap
title: v0.6.0 Credential Lifecycle Roadmap
status: active
created: 2026-06-20
updated: 2026-06-22
branch: feature/v0.6-credential-preflight
pr:
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
  `Credentials` model deliberately remains runtime-only while lifecycle metadata and provider-scoped
  refresh secrets live beside profiles.

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
- PR 5: access-key renewal/reacquisition orchestration. Persist safe refresh metadata where
  available, report why silent refresh is unavailable, and otherwise fall back to explicit QR/login
  renewal.
- PR 6: provider-aware plaintext refresh-secret storage. Store access-key refresh secrets in
  `profile_secrets.<profile>.access_key.<provider>` without moving raw secrets into runtime
  `Credentials`; record provider/keypair metadata for BALH/BiliPlus handoff tokens.
- PR 7: provider-specific access-key refresh clients. Implement mocked and documented refresh
  clients for Bilibili main OAuth2 and BiliIntl OAuth2, preserving old tokens on transient failures
  and returning reauthorization-required outcomes for invalid refresh tokens.
- PR 8: optional credential preflight integration for planning/downloading. Let CLI and embedding
  callers choose whether to fail fast, warn, or try renewal before restricted-area, TV, APP, or WEB
  request paths.
- PR 9: multi-account lifecycle polish. Support lifecycle reporting across profiles, selected-profile
  renewal, and non-destructive profile updates that avoid overwriting concurrently refreshed
  credentials.
- PR 10: release prep for `v0.6.0`, including bilingual docs, public API checks, deterministic CI,
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
  - CLI WEB/TV QR login now records lifecycle source and acquisition time, and access-key login
    persists callback-provided absolute or relative expiry metadata plus refresh-token presence in
    the selected credential profile.
  - Embedding and architecture docs now describe the difference between local lifecycle evaluation
    metadata, saved login provenance, and network-backed credential health probes.
- PR 4 implemented CLI lifecycle UX:
  - `auth status` keeps the legacy no-flag selected-profile redacted summary, while
    `auth status --profiles` reports selected/default profile markers, lifecycle status,
    per-credential lifecycle metadata, and guidance.
  - `auth status --profiles --all-profiles` expands the same local lifecycle evaluation across all
    saved profiles without printing raw credentials.
  - `auth health --all-profiles` checks every saved profile with network health probes, and
    `--json --all-profiles` returns each profile's redacted lifecycle status, raw health report,
    compact summary, and guidance for downstream UI.
  - All-profile status and health output now also includes the explicitly selected profile when it
    has no saved credentials, so downstream callers can still identify the selected empty profile
    and prompt for login.
  - Human `auth health` output now adds guidance when credentials are stale, expired, rejected, or
    failed by request, while preserving the raw `auth health --json` schema for single-profile use.
- PR 5 implements access-key renewal orchestration without pretending silent refresh is available:
  - `AccessKeyRenewalDecision` maps selected profile lifecycle status to `NoAction` or
    `Reauthorize` and exposes `automatic_refresh_readiness` so embedders can distinguish missing
    credentials, unsupported sources, missing refresh-token metadata, and metadata-only refresh
    token evidence.
  - `auth renew-access-key` emits newline-delimited JSON `decision`, `ticket`, and optional `saved`
    events, or equivalent human output, while reusing the existing safe BALH parser and
    `save_credentials_with_lifecycle` path.
  - Fresh access-key metadata returns `no_action`; missing, unknown, stale, expiring, expired, or
    forced credentials return a BiliPlus/BALH reauthorization ticket.
  - Providing `--stdin` or `--file` to `auth renew-access-key` completes the reauthorization and
    refreshes the selected profile's generic access key and lifecycle metadata without printing raw
    access or refresh tokens.
  - Bilingual README, user guide, embedding guide, and architecture docs now describe the difference
    between metadata-only refresh-token evidence and a future stored refresh secret.
- PR 6 implemented provider-aware plaintext refresh-secret storage:
  - The implementation keeps runtime `Credentials` limited to cookie/access-key values and stores raw
    refresh tokens under `CredentialProfileSecrets`.
  - Access-key lifecycle metadata records the active provider and whether a refresh token was seen,
    while `AccessKeyRenewalDecision` reports refresh-secret presence without leaking raw token
    values.
  - BALH/BiliPlus callbacks are stored under the `balh_biliplus` provider with refresh provider
    `bilibili_main_oauth2` and keypair family `bili_tv`; BiliPlus itself remains treated as an
    acquisition provider rather than a proven refresh endpoint.
- PR 7 implements provider-specific access-key refresh clients:
  - `AccessKeyRefreshRequest` and `BiliClient::refresh_access_key(...)` expose a core API for
    provider-specific refresh without routing embedders through the CLI.
  - Bilibili main OAuth2 refresh uses the configured `passport_base`, signed app keypairs
    (`bili_tv`, `android`, or `android_b`), and the stored provider refresh token.
  - BiliIntl OAuth2 refresh uses the new configurable `intl_passport_base` and the intl refresh
    form.
  - `auth renew-access-key` now attempts non-destructive automatic refresh for ready selected
    profiles when no `--force`, `--stdin`, or `--file` override is supplied; failures emit
    `refresh_failed` and fall back to the existing reauthorization ticket path.
  - `automatic_refresh_readiness=ready` now requires not only a raw refresh secret but also a refresh
    provider and, for Bilibili main OAuth2, a refresh keypair.
  - PR review follow-up redacts exact access-key and refresh-token values from automatic refresh
    failure output, including server error messages that echo request form values.
  - PR review follow-up keeps lifecycle `refresh_token_present=true` when automatic refresh succeeds
    without returning a replacement refresh token and the saved provider secret falls back to the
    previous refresh token.
  - GitHub Codex review follow-up sends both `access_key` and `access_token` aliases in main
    Bilibili OAuth2 refresh forms while preserving the selected app keypair signer.
  - GitHub Codex review follow-up filters empty refresh-response token aliases before falling back
    to alternate fields, preventing an empty alias from overwriting a usable access key.
  - GitHub Codex review follow-up preserves explicit
    `refresh_token_secret_present=false` lifecycle status when provider metadata exists but no
    provider secret is stored.
  - GitHub Codex review follow-up routes `bili_tv` main-provider refresh requests to the TV OAuth
    refresh path under the configured passport base, while Android-family keypairs continue using the
    main passport refresh path.
- PR 8 implements optional credential preflight for media planning/downloading:
  - `CredentialPreflightReport` exposes a pure core evaluator for selected-profile lifecycle status,
    media request-path requirements, warnings/blockers, and the selected access-key renewal decision.
  - Request-path requirements distinguish WEB optional cookies, TV `tv_access_key`, APP generic
    `access_key` with `tv_access_key` fallback, intl/Bstar generic `access_key`, and optional
    restricted-area proxy generic `access_key` when that proxy fallback may run.
  - CLI `plan`, `playback`, and `download` support global `--credential-preflight off|warn|fail|renew`
    plus lifecycle window overrides.
  - `warn` writes diagnostics to stderr while keeping JSON stdout as a single payload; `fail`
    aborts before stream resolution when required credentials are missing or relevant lifecycle
    metadata is non-fresh.
  - Preflight parses media input once, reuses that parsed `Input` for plan/playback/download
    planning, avoids b23 short-link double resolution, and prevents renewal before raw input
    validation succeeds.
  - Core embedders can use `CredentialPreflightReport::from_media_paths_context(...)` to express
    fixed-source paths such as intl/Bstar without inheriting unrelated WEB/TV/APP playurl
    requirements.
  - Sidecar-only download modes skip TV/APP/restricted-proxy stream preflight, fixed-source
    intl/Bstar and PUGV inputs avoid unrelated global TV/APP credential requirements, intl/Bstar does
    not inherit unrelated WEB cookie lifecycle failures, and `download --progress-json` suppresses
    plaintext preflight diagnostics while wrappers still parse only JSON object lines because final
    CLI errors may also appear on stderr.
  - Archive downloads defer `renew` automatic access-key refresh until after duplicate handling when
    initial planning succeeds, so `--on-duplicate cancel` does not call refresh endpoints or mutate
    stored credentials.
  - If initial archive planning fails with an auth-like credential error before duplicate preflight
    can be inspected, the CLI refreshes a ready generic access key and retries planning once,
    including when local lifecycle metadata had still considered the key fresh.
  - Sidecar-only cover/danmaku downloads skip playurl credential preflight so stale WEB cookie
    metadata does not block operations that do not fetch playurl streams.
  - `renew` attempts provider-specific generic access-key refresh for refresh-ready selected
    profiles, saves the refreshed credential non-interactively, reloads credentials, and then
    continues with media resolution.
  - Bilingual README, user guide, embedding guide, and architecture docs now describe the preflight
    strategy and embedding surface.

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
  - `cargo fmt --all -- --check`.
  - `cargo test -p bbdown-core credentials --locked`.
  - `cargo test -p bbdown-core --test public_api --locked`.
  - `cargo test -p bbdown-cli save_credentials --locked`.
  - `cargo test -p bbdown-cli lifecycle_metadata --locked`.
  - `cargo test -p bbdown-cli auth_login_access_key --test cli_e2e --locked`.
  - `cargo test -p bbdown-cli auth_qr_login_web_and_tv_use_local_store --test cli_e2e --locked`.
  - `cargo clippy -p bbdown-core --all-targets --locked -- -D warnings`.
  - `cargo clippy -p bbdown-cli --all-targets --locked -- -D warnings`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/07780f1c323453fd738330fbf8fd70e2899d4409/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `git diff --check`.
  - `just ci`.
- PR 4 local validation:
  - `cargo fmt --all -- --check`.
  - `cargo test -p bbdown-cli auth_status_profiles --test cli_e2e --locked`.
  - `cargo test -p bbdown-cli auth_health_all_profiles --test cli_e2e --locked`.
  - `cargo test -p bbdown-cli auth_health_escapes_control_characters_in_human_probe_messages --test cli_e2e --locked -- --exact`.
  - `cargo test -p bbdown-cli auth_health_all_profiles_escapes_control_characters_in_human_profile_names --test cli_e2e --locked -- --exact`.
  - `cargo test -p bbdown-cli auth_status_all_profiles_includes_selected_empty_profile --test cli_e2e --locked -- --exact`.
  - `cargo test -p bbdown-cli auth_health_all_profiles_includes_selected_empty_profile --test cli_e2e --locked -- --exact`.
  - `cargo test -p bbdown-cli auth_health_reports_redacted_credential_probe_statuses --test cli_e2e --locked`.
  - `cargo clippy -p bbdown-cli --all-targets --locked -- -D warnings`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/07780f1c323453fd738330fbf8fd70e2899d4409/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `git diff --check`.
  - `just ci`.
- PR 5 local validation:
  - `cargo test -p bbdown-core access_key_renewal --locked`.
  - `cargo test -p bbdown-cli auth_renew_access_key --test cli_e2e --locked`.
  - `cargo fmt --all -- --check`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/07780f1c323453fd738330fbf8fd70e2899d4409/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `git diff --check`.
  - `just ci`.
- PR 6 local validation:
  - `cargo test -p bbdown-cli access_key --locked`.
  - `cargo test -p bbdown-cli save_credentials_with_lifecycle_and_secrets_clears_stale_provider_secret --locked`.
  - `cargo clippy -p bbdown-cli --all-targets --locked -- -D warnings`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/07780f1c323453fd738330fbf8fd70e2899d4409/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `git diff --check`.
  - `just ci`.
- PR 7 local validation:
  - `cargo test -p bbdown-core access_key_refresh --locked`.
  - `cargo test -p bbdown-core refreshes_ --locked`.
  - `cargo test -p bbdown-cli auth_renew_access_key_auto_refreshes_ready_provider_secret --locked`.
  - `cargo test -p bbdown-cli auth_renew_access_key_auto_refresh --test cli_e2e --locked`.
  - `cargo test -p bbdown-cli intl_passport_base --locked`.
  - `cargo test -p bbdown-cli auth_renew_access_key --test cli_e2e --locked`.
  - `cargo test -p bbdown-core login::tests --locked`.
  - `cargo fmt --all -- --check`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/c192ee2af594cc9cb64cf151261c58b2695513fb/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `git diff --check`.
  - `just ci`.
- PR 7 review-fix validation:
  - `cargo fmt --all -- --check`.
  - `cargo test -p bbdown-core access_key_refresh --locked`.
  - `cargo test -p bbdown-core refresh_credentials_ignore_zero_expiry_aliases_when_falling_back --locked`.
  - `cargo test -p bbdown-cli auth_renew_access_key_auto_refresh --test cli_e2e --locked`.
  - `cargo test -p bbdown-core refreshes_ --locked`.
  - `cargo test -p bbdown-cli auth_renew_access_key --test cli_e2e --locked`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/c192ee2af594cc9cb64cf151261c58b2695513fb/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `git diff --check`.
  - `just ci`.
- PR 7 whitespace-secret review-fix validation:
  - `cargo fmt --all`.
  - `cargo fmt --all -- --check`.
  - `cargo test -p bbdown-core whitespace_refresh_token_secret_is_not_lifecycle_ready --locked`.
  - `cargo test -p bbdown-core credentials --locked`.
  - `cargo test -p bbdown-cli auth_renew_access_key --test cli_e2e --locked`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/c192ee2af594cc9cb64cf151261c58b2695513fb/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `git diff --check`.
  - `just ci`.
- PR 7 current-head GitHub Codex review-fix validation:
  - `cargo fmt --all`.
  - `cargo test -p bbdown-core missing_refresh_secret_reports_false_when_provider_metadata_exists --locked`.
  - `cargo test -p bbdown-core refreshes_tv_access_key_with_tv_oauth_endpoint --locked`.
  - `cargo test -p bbdown-core refreshes_android_access_key_with_main_oauth_endpoint --locked`.
  - `cargo test -p bbdown-core access_key_refresh --locked`.
  - `cargo test -p bbdown-core refreshes_ --locked`.
  - `cargo test -p bbdown-cli auth_renew_access_key_auto_refresh --test cli_e2e --locked`.
  - `cargo test -p bbdown-core credentials --locked`.
  - `cargo test -p bbdown-cli auth_renew_access_key --test cli_e2e --locked`.
  - `cargo fmt --all -- --check`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/c192ee2af594cc9cb64cf151261c58b2695513fb/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `git diff --check`.
- PR 8 local validation:
  - `cargo fmt --check`.
  - `cargo test -p bbdown-core credential_preflight --locked`.
  - `cargo test -p bbdown-core --test public_api --locked`.
  - `cargo test -p bbdown-cli credential_preflight --locked`.
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/5f1ab3fa5d9f7d534507216a2d6f765694f9b710/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
  - `git diff --check`.
  - `just ci`.
- PR 8 review-fix validation:
  - `cargo fmt --all -- --check`.
  - `git diff --check`.
  - `cargo test -p bbdown-cli --test cli_e2e download_archive_retries_plan_after_deferred_credential_refresh --locked`.
  - `cargo test -p bbdown-cli --test cli_e2e download_archive_cancel_defers_credential_preflight_renewal --locked`.
  - `cargo test -p bbdown-cli --test cli_e2e download_progress_json_reports_credential_preflight_failure --locked`.
  - `cargo test -p bbdown-core --lib credential_preflight --locked`.
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- PR 8 second review-fix checkpoint:
  - `--progress-json --on-duplicate cancel` now suppresses duplicate preflight plaintext on stderr while still emitting the `plan_cancelled` event.
  - Archive downloads now distinguish "no duplicate decision was required" from a real `replace` decision, then rerun duplicate preflight decision handling when deferred credential refresh changes the plan/preflight into a conflict.
  - Archive credential retry now treats generic API `-400` as refresh-worthy only when the message looks auth/access-key related, avoiding credential mutation for ordinary invalid-parameter failures.
  - Archive credential retry now also message-filters API `-403`, gRPC `7`, and gRPC `16`, so
    region/permission failures such as `area restricted` do not trigger forced access-key refresh or
    rewrite local credentials.
  - User Guide APP-mode prose now matches the implemented credential order: generic
    `access_key` first, then `tv_access_key` fallback.
  - If deferred credential refresh changes archive preflight into a duplicate conflict, explicit
    `--on-duplicate cancel` now re-enters the CLI cancel-report path instead of letting the library
    executor convert it into an error.
  - Added mock e2e coverage for these review follow-ups:
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_progress_json_cancel_suppresses_plaintext_preflight --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_reruns_duplicate_preflight_after_deferred_refresh --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_cancel_reports_duplicate_after_deferred_refresh --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_for_generic_api_bad_request --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_for_app_area_restricted_status --locked`.

## Next Steps

- Finish PR 8 local full validation, review gates, CI, and merge.
- Continue PR 9 multi-account lifecycle polish after preflight behavior is stable on `master`.

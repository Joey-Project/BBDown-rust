---
id: 20260620-019f16-v0-6-credential-lifecycle-roadmap
title: v0.6.0 Credential Lifecycle Roadmap
status: active
created: 2026-06-20
updated: 2026-06-23
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
  - Sidecar-only cover/danmaku downloads skip TV/APP/restricted-proxy stream preflight so stale WEB
    cookie metadata does not block operations that do not fetch playurl streams.
  - Authenticated feed inputs such as history, watch-later, following, and space dynamic now add a
    required WEB cookie preflight requirement before hitting account-scoped WEB APIs.
  - Stale optional WEB playurl cookie metadata is warning-only rather than blocking in `fail`, so
    public anonymous WEB playurl requests can continue when a stored cookie is non-fresh.
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
  - User-facing and architecture docs now describe APP-mode credential order as provider-aware:
    Bilibili main/BALH generic `access_key` values are checked before `tv_access_key`, while
    `bili_intl_oauth2` generic keys and legacy profiles without provider metadata yield to TV keys.
  - If deferred credential refresh changes archive preflight into a duplicate conflict, explicit
    `--on-duplicate cancel` now re-enters the CLI cancel-report path instead of letting the library
    executor convert it into an error.
  - Current-head review fixes after GitHub Codex review:
    - Metadata-only intl/Bstar downloads such as `download --only cover` and `download --only danmaku`
      now skip required intl generic access-key preflight; media and subtitle modes still preflight
      the generic key because they request intl playurl or subtitle endpoints.
    - Selection-required inputs (`ss`, `md`, and cheese season links without `--select`) now fail
      before credential preflight can refresh or rewrite stored credentials.
    - APP playurl access-key selection now accepts access-key provider metadata, so intl OAuth
      generic keys do not preempt a usable TV token.
    - Archive forced-refresh retry no longer refreshes generic `access_key` when an authenticated
      feed request is missing a usable WEB cookie.
    - Auth-like archive retry classification now matches auth/OAuth words instead of a raw `auth`
      substring, so non-auth errors such as `author id invalid` do not refresh access keys.
    - Bare HTTP 401/403 and restricted-area proxy `401 Unauthorized` / `403 Forbidden` summaries no
      longer trigger automatic generic access-key refresh unless the error also carries explicit
      access-key/token/login evidence.
    - Authenticated WEB feed `-101 not logged in` failures are now attributed to the WEB cookie path
      for history/watch-later/following inputs, so archive deferred or forced access-key retry does
      not refresh and persist a generic `access_key` when the selected profile's cookie was rejected.
    - Generic API `-101`, `-400`, `-403`, `7`, and `16` failures now require explicit access-key,
      access-token, or credential wording before archive retry refreshes a generic `access_key`, so
      private favorite or other optional-cookie WEB API login/auth failures do not mutate stored
      tokens; restricted-area PgcProxy `-101` remains refreshable because that resolver path is
      access-key-bearing even after diagnostics redact token wording.
    - Immediate credential preflight renewal now skips generic access-key refresh when the same
      report still has an unsatisfied required non-access-key requirement, such as missing WEB
      cookies for history/watch-later/following inputs.
    - Local offline review follow-up narrows that guard to missing required non-access-key
      credentials, so stale/expiring/expired/unknown but present WEB cookies no longer block a
      refresh-ready generic access-key renewal; subsequent network requests still prove whether the
      WEB cookie works.
    - Local independent review follow-up treats whitespace-only stored cookie/access-key values as
      missing for lifecycle status and redacted presence booleans, matching the request builders that
      trim token values before sending them.
    - Current-head independent review follow-up aligns request-side credential normalization with
      lifecycle/preflight semantics: request builders now trim stored cookie/access-key values before
      sending them, omit values that become empty, and fall back across APP generic/TV keys instead
      of letting whitespace-only keys shadow usable alternatives.
    - Current-head offline review follow-up widens archive retry refresh classification for common
      access-key expiry surfaces that do not spell out `access_key`, including Bilibili Chinese
      `account not logged in` API messages and APP gRPC code 16 without a grpc-message, while
      preserving proxy-owned credential-error negatives.
    - Current-head independent review follow-up removes the broad `PgcProxy` + `-101` archive retry
      shortcut, so restricted-area proxy failures still need access-key-specific diagnostic evidence
      before refreshing and rewriting the generic access key. The core resolver now preserves that
      evidence as a non-secret `access key diagnostic` marker after redacting raw `access_key`
      diagnostics, allowing true access-key failures to retry without exposing key names or values.
    - GitHub Codex review follow-up gates archive retry refresh on the failed request path: generic
      access-key refresh is allowed for APP/intl access-key request failures or restricted-area
      proxy resolver failures, but not for official PGC WEB failures that happen before the proxy
      request can send the generic access key.
    - Independent review follow-up tightens the path gate further so account-scoped feed inputs can
      still refresh APP access-key `-101` failures, while plain WEB cookie `-101` failures remain
      non-refreshable. Restricted-area proxy `-101` summaries with Bilibili `账号未登录` wording now
      also count as access-key refresh evidence.
    - Offline review follow-up trims stored access-key and refresh-token values before constructing
      automatic refresh requests, aligning `--credential-preflight renew` with request-side
      credential normalization.
    - Local independent review follow-up removes bare `credential` wording from access-key-specific
      archive retry classification, preventing proxy-owned errors such as `invalid proxy credential`
      from refreshing and rewriting the generic Bilibili access key.
    - Current-head review follow-up removes generic API `-101 账号未登录` from the access-key retry
      classifier so optional cookie-carrying WEB API failures such as favorite-list login rejection
      do not rotate generic access keys; restricted-area resolver summaries still treat proxy-owned
      bare HTTP 401/403 and `账号未登录` as retryable because that path can send the generic
      access key.
    - Current-head independent review follow-up preserves URL context only for known
      cookie-authenticated feed JSON HTTP status failures, then suppresses generic access-key
      archive retry for those authenticated feed endpoints when they return bare HTTP 401/403. APP
      playurl and intl HTTP status failures still redact URLs and remain eligible for access-key
      refresh when the configured request path can actually send the generic access key.
    - Current-head Codex and independent review follow-up expands safe JSON HTTP-status URL
      preservation to known non-generic-access-key WEB/PGC/PUGV/list/playurl metadata paths while
      stripping query, fragment, and userinfo before storing the URL on the error. Archive retry now
      uses those paths to suppress generic access-key refresh for metadata, account-feed, and
      official WEB playurl HTTP 401/403 failures that did not send the generic key.
    - Restricted-area proxy bare HTTP 401/403 summaries no longer trigger generic access-key refresh
      unless the diagnostic includes explicit access-key or access-token evidence. APP and intl
      access-key request failures that surface as URL-redacted bare HTTP 401/403 remain refreshable
      when the selected request path can actually send the generic key.
    - Crate-local README files now document the provider-aware APP credential order for embedders,
      matching the top-level and embedding docs.
  - Added mock e2e coverage for these review follow-ups:
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_progress_json_cancel_suppresses_plaintext_preflight --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_reruns_duplicate_preflight_after_deferred_refresh --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e playback_app_uses_tv_access_key_when_generic_key_is_intl_provider --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_only_cover_skips_intl_access_key_credential_preflight --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e plan_credential_preflight_renew_skips_access_key_refresh_when_required_cookie_is_missing --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_generic_key_for_authenticated_feed_cookie_failure --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_complete_deferred_refresh_for_authenticated_feed_cookie_failure --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_for_optional_web_api_auth_like_failure --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e plan_selection_required_input_fails_before_credential_preflight_renewal --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_cancel_reports_duplicate_after_deferred_refresh --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_for_generic_api_bad_request --locked`.
    - `cargo test -p bbdown-core --lib credential --locked`.
    - `cargo test -p bbdown-cli --bin bbdown plan_failure_classifier --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_retries_app_access_key_when_required_cookie_is_stale_but_present --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e auth_renew_access_key --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e credential_preflight --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh --locked`.
    - `just ci`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_for_app_area_restricted_status --locked`.
    - `cargo test -p bbdown-cli --bin bbdown plan_failure_classifier --locked`.
    - `cargo test -p bbdown-cli --bin bbdown generic_access_key_retry_requires_failure_path_that_uses_access_key --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_for_pgc_web_failure_before_proxy --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_retries_restricted_proxy_after_deferred_credential_refresh --locked`.
    - `cargo test -p bbdown-cli --bin bbdown plan_failure_classifier --locked`.
    - `cargo test -p bbdown-cli --bin bbdown access_key_refresh_request_trims_stored_tokens --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_for_optional_web_api_not_logged_in --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_generic_key_for_authenticated_feed_http_status --locked`.
    - `cargo test -p bbdown-core --lib renew_mode_skips_access_key_refresh_when_required_cookie_is_missing --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_generic_key_for_video_metadata_http_status --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_retries_app_access_key_after_http_unauthorized_status --locked`.
    - `cargo test -p bbdown-cli --test cli_e2e download_archive_does_not_refresh_for_restricted_proxy_http_auth_status_without_key_evidence --locked`.
    - `cargo test -p bbdown-core --lib intl_access_key_is_redacted_from_http_errors --locked`.

## Next Steps

- Finish PR 8 local full validation, review gates, CI, and merge.
- Continue PR 9 multi-account lifecycle polish after preflight behavior is stable on `master`.

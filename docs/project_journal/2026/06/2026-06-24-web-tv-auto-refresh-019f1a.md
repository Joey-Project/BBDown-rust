---
id: 20260624-019f1a
title: WEB And TV Credential Auto Refresh
status: completed
created: 2026-06-24
updated: 2026-06-24
branch: feature/web-tv-auto-refresh
pr: https://github.com/Joey-Project/BBDown-rust/pull/72
supersedes: []
superseded_by:
---

# WEB And TV Credential Auto Refresh

## Summary
- Added saved-refresh-token support for WEB cookie and TV `tv_access_key` credentials as the final
  planned pre-release feature in the `v0.6.0` credential lifecycle line.

## Current State
- QR login can persist refresh-token secrets for WEB and TV credential slots while lifecycle
  metadata records only presence and timestamps.
- `bbdown auth renew-web` and `bbdown auth renew-tv` can refresh the selected profile explicitly.
- Credential preflight `renew` can automatically refresh stale WEB cookie or TV `tv_access_key`
  requirements when the selected profile has the matching stored secret.
- Runtime credentials remain separate from persisted refresh secrets, and credential-store writes
  verify the selected profile and original credential under the update lock before saving refreshed
  values.

## Next Steps
- Prepare the final `v0.6.0` release-prep PR after this slice lands.

## Evidence
- Branch: `feature/web-tv-auto-refresh`.
- Validation:
  - `cargo test -p bbdown-core login --locked`
  - `cargo test -p bbdown-cli --test cli_e2e auth --locked`
  - `cargo test -p bbdown-core refresh_secret --locked`
  - `cargo test -p bbdown-core web_cookie_refresh --locked`
  - `cargo test -p bbdown-core polls_web_qr_login_states --locked`
  - `cargo test -p bbdown-core creates_and_polls_tv_qr_login --locked`
  - `cargo test -p bbdown-core refreshed_cookie_merge_deduplicates_replaced_cookie_pairs --locked`
  - `cargo test -p bbdown-core refreshes_web_cookie --locked`
  - `cargo test -p bbdown-cli stored_credential_refresh_request_debug_redacts_secrets --locked`
  - `cargo test -p bbdown-cli --test cli_e2e web_cookie --locked`
  - `cargo test -p bbdown-cli --test cli_e2e auth_renew_web --locked`
  - `cargo test -p bbdown-cli auth_renew --locked`
  - `cargo test -p bbdown-cli credential_preflight_renew --locked`
  - `cargo test -p bbdown-cli download_archive_cancel_defers --locked`
  - `cargo test -p bbdown-cli deferred_preflight_forces_failed_kind_after_unrelated_stored_refresh_noop --locked`
  - `cargo test -p bbdown-cli deferred_preflight_preserves_stored_refresh_when_access_key_refresh_fails --locked`
  - `cargo test -p bbdown-cli download_archive_retries_fresh_web_cookie_after_authenticated_feed_failure --test cli_e2e --locked`
  - `cargo test -p bbdown-cli download_archive_retries_plan_after_auth_failure_with_fresh_refreshable_access_key --test cli_e2e --locked`
  - `cargo test -p bbdown-cli passport_base_controls_tv_poll_when_tv_base_is_implicit --locked`
  - `cargo test -p bbdown-cli auth_renew_tv_uses_passport_base_when_tv_base_is_implicit --test cli_e2e --locked`
  - `cargo test -p bbdown-core web_qr_poll_falls_back_to_url_when_header_cookie_is_not_refreshable --locked`
  - `cargo test -p bbdown-core web_qr_poll_rejects_success_url_without_csrf --locked`
  - `cargo test -p bbdown-cli auth_renew_web_fails_when_refresh_secret_is_missing --locked`
  - `cargo test -p bbdown-cli auth_renew_tv_fails_when_refresh_secret_is_missing --locked`
  - `cargo test -p bbdown-core tv_keypair_access_key_refresh_uses_tv_passport_poll_base --locked`
  - `cargo test -p bbdown-cli auth_renew_web_fails_when_cookie_refresh_request_fails --locked`
  - `cargo test -p bbdown-cli auth_renew_tv_uses_tv_passport_base_for_refresh --locked`
  - `cargo test -p bbdown-cli auth_renew_tv_fails_when_profile_changes_before_save --locked`
  - `cargo test -p bbdown-cli download_archive_does_not_refresh_tv_key_for_metadata_http_status --test cli_e2e --locked`
  - `cargo check -p bbdown-cli --locked`
  - `cargo fmt --all -- --check`
  - `git diff --check`
  - `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/5f1ab3fa5d9f7d534507216a2d6f765694f9b710/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`
  - `just ci`
- Review:
  - Helper-backed Codex review found that WEB cookie refresh reused the old refresh token when the
    server did not return a replacement token; fixed by failing the refresh before confirmation and
    adding regression coverage.
  - Helper-backed Codex review found that `refresh=false` cookie-info responses still rewrote the
    credential store; fixed by adding an explicit no-op refresh result and CLI/core regression
    coverage.
  - A later whole-diff rerun was inconclusive after a bounded wait; a focused Codex fallback review
    over the repaired refresh paths returned `LGTM`.
  - PR-readiness independent Codex review returned `LGTM`; offline frozen diff review then found
    that WEB refresh tokens should not be sent in URL query strings and that WEB no-op refresh
    should update lifecycle checked time. Both were fixed with targeted regression coverage.
  - A current-head independent Codex rerun found three follow-up issues: WEB cookie refresh could
    save a successful refresh without a refreshed `SESSDATA` `Set-Cookie`, WEB/TV preflight did not
    re-evaluate after a stale-response skip, and an internal stored-refresh request had a
    non-redacted `Debug` implementation. These were fixed with core and CLI regression coverage.
  - The next current-head independent Codex rerun found that `Set-Cookie: SESSDATA=` empty values
    still counted as refreshed auth cookies. This was fixed by requiring a non-empty `SESSDATA`
    value and extending WEB cookie refresh regression coverage.
  - A subsequent current-head independent Codex rerun found that a later empty `SESSDATA`
    `Set-Cookie` could still overwrite an earlier non-empty one after merge, and that internal
    token-bearing refresh DTOs/endpoints still derived `Debug`. These were fixed by validating the
    merged cookie header before confirmation and removing sensitive `Debug` derives.
  - The next current-head independent Codex rerun found that duplicate original `SESSDATA` cookie
    pairs could leave an older non-empty value behind after an empty replacement. This was fixed by
    deduplicating replaced cookie names during merge and adding duplicate-cookie regression coverage.
  - The paired offline frozen diff review found that provider refresh secrets were hidden behind the
    optional lifecycle `refresh_token_present` metadata flag. This was fixed so stored provider
    secrets make legacy/manual profiles refresh-ready even when old metadata omitted the presence
    flag, with core regression coverage.
  - The helper-backed offline frozen diff review on `57bd18d..9382f6b` found that
    `QrLoginState::Succeeded` changed its public payload type before the release bump. The follow-up
    fix restored the legacy `QrLoginState` payload and added detail polling methods for QR refresh
    metadata.
  - Current-head GitHub Codex review for `29c68be` then corrected an earlier local review
    assumption about WEB `correspond/1`: the public Bilibili refresh documentation uses
    RSA-OAEP/SHA-256 for the encrypted challenge. The fix restored RSA-OAEP/SHA-256 and added CLI
    e2e coverage so stored WEB/TV refresh server failures emit `refresh_failed` and return a
    non-zero exit status.
  - The next offline frozen diff review on `57bd18d..0925559` found that the direct `auth renew-web`
    `refresh=false` path treated a stale checked-save guard as a failed command. The fix now treats
    `SkippedStaleRequest` as handled for that no-op path and adds unit coverage that the stale guard
    does not overwrite a concurrently updated WEB cookie profile or refresh secret.
  - The current-head independent Codex rerun on `30cd646` found that direct `auth renew-web` /
    `auth renew-tv` setup failures, such as metadata-only refresh tokens without stored refresh
    secrets, emitted `refresh_failed` but still exited successfully. The fix makes these direct
    renewal commands exit non-zero and adds WEB/TV CLI e2e coverage for missing refresh secrets.
  - The current-head independent Codex rerun on `4015759` found that WEB cookie refresh error
    redaction could leak cookie pair/value fragments and that BiliTV access-key refresh ignored a
    configured TV passport poll endpoint. The fix now redacts cookie pairs and values in refresh
    errors, sends BiliTV keypair refresh requests to `tv_passport_poll_base` when overridden while
    preserving legacy `passport_base` compatibility, and documents the endpoint behavior.
  - The current-head independent Codex rerun on `b37db07` found that direct `auth renew-web` /
    `auth renew-tv` could emit `refresh_skipped` after a stale-response save guard but still exit
    successfully. The fix makes direct renewal treat `SkippedStaleRequest` as a failed renewal while
    preserving the no-overwrite guard, adds a TV direct-renew CLI e2e regression, and documents the
    direct-command versus media-preflight behavior.
  - The follow-up current-head independent Codex rerun on `a6923ac` found that preflight could keep
    using stale profile/status input after a stale-save skip, that generic `auth renew-access-key`
    still treated a stale-save skip as success, and that direct WEB stale-save behavior lacked e2e
    coverage. The fix reloads lifecycle state for each stored preflight refresh attempt, makes
    direct generic access-key renewal fail after `refresh_skipped`, and adds direct WEB/generic
    access-key stale-save CLI e2e coverage.
  - GitHub Codex review and the current-head independent Codex rerun on `dd04f75` found four
    follow-up issues: stored WEB/TV refresh failures should have an explicit non-zero command path,
    stored preflight refresh should not run when another required credential is missing, archive
    duplicate/cancel flows should defer stored WEB/TV refresh just like generic access-key refresh,
    and `auth renew-access-key --stdin/--file` should not save a browser handoff into a different
    default profile if the selected profile changes while waiting for input. The fix tightens direct
    refresh failure handling, gates unrelated preflight refresh attempts, defers stored refresh for
    archive duplicate handling, binds handoff saves to the initial renewal decision profile, and adds
    CLI e2e coverage for these cases.
  - A follow-up independent Codex rerun on `7792402` found that deferred archive preflight could
    lose a successful stored WEB/TV refresh result when a later generic access-key refresh attempt
    returned `false`, preventing archive downloads from rebuilding the client and plan with the
    refreshed stored credential. The fix preserves the earlier stored-refresh success and adds a
    regression covering stored TV refresh success plus generic access-key refresh failure.
  - GitHub Codex review on `111e8fe` found two additional credential retry gaps: BiliTV keypair
    access-key refresh still routed to a customized WEB `passport_base` when TV poll base was left
    at its default, and archive planning could not force-refresh a saved WEB cookie or TV key after
    the server rejected a credential that local lifecycle metadata still considered fresh. The fix
    routes BiliTV keypair refresh through `tv_passport_poll_base` unconditionally, adds forced
    stored WEB/TV refresh on media retry failures, and covers the fresh WEB cookie history retry
    path with CLI e2e coverage.
  - The paired offline frozen diff review on `eba8713` found that deferred archive retry used a
    global stored-refresh gate, so an unrelated WEB cookie no-op could prevent forced TV key refresh
    for the actual playurl failure. The fix tracks stored-refresh handling per credential kind and
    adds regression coverage for the unrelated no-op plus forced TV refresh path.
  - A follow-up independent Codex rerun on `7a473ab` found that BiliTV keypair refresh no longer
    honored a CLI `--passport-base` override when no TV-specific passport override was supplied,
    despite the documented compatibility behavior. The fix makes the implicit TV poll base follow
    `--passport-base` unless `--tv-passport-base` or `--tv-passport-poll-base` is provided, with
    unit and CLI e2e coverage.
  - The final independent Codex rerun found two WEB QR cookie validation gaps: successful polling
    preferred any `Set-Cookie` header over the success URL fallback, and WEB QR login could save a
    refresh token with a cookie that lacked `bili_jct`/csrf, making later auto-refresh impossible.
    The fix only trusts refreshable header cookies and requires the selected WEB QR cookie to contain
    non-empty `SESSDATA` plus csrf, with core and CLI fixture coverage.
  - The next independent Codex rerun found that TV forced retry could rotate a stored TV key after
    unrelated JSON metadata HTTP 401/403 failures, because the TV HTTP branch ignored the preserved
    classification URL. The fix now rejects preserved non-TV-playurl JSON HTTP failures before TV
    refresh and adds archive CLI e2e coverage with a fresh TV refresh secret and metadata `403`.

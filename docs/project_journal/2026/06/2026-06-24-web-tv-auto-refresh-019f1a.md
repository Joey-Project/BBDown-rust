---
id: 20260624-019f1a
title: WEB And TV Credential Auto Refresh
status: completed
created: 2026-06-24
updated: 2026-06-24
branch: feature/web-tv-auto-refresh
pr:
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
  - `cargo test -p bbdown-core web_cookie_refresh --locked`
  - `cargo test -p bbdown-core refreshes_web_cookie --locked`
  - `cargo test -p bbdown-cli stored_credential_refresh_request_debug_redacts_secrets --locked`
  - `cargo test -p bbdown-cli --test cli_e2e web_cookie --locked`
  - `cargo test -p bbdown-cli --test cli_e2e auth_renew_web --locked`
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
  - The helper-backed offline frozen diff review on `57bd18d..32faac5` returned `LGTM`; final PR
    readiness evidence is tracked on PR #72.

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

---
id: 20260609-019eac-release-environment-names
title: Release Environment Variable Names
status: completed
created: 2026-06-09
updated: 2026-06-09
branch: wip/release-app-env-names
pr:
supersedes:
superseded_by:
---

# Release Environment Variable Names

## Summary

- Scope is a small release automation follow-up before rerunning the 0.1.0 RC workflow.
- The release GitHub App identifier is intentionally configured as an environment variable, while
  the private key remains an environment secret.

## Current State

- The `release-candidate` and `production-release` environments are expected to expose
  `RELEASE_APP_CLIENT_ID` as a variable and `RELEASE_APP_PRIVATE_KEY` as a secret.
- `.github/workflows/create-release-candidate.yml` now passes
  `vars.RELEASE_APP_CLIENT_ID` to `actions/create-github-app-token` and keeps the private key under
  `secrets.RELEASE_APP_PRIVATE_KEY`.
- `.github/workflows/promote-release-candidate.yml` uses the same variable and secret names for
  the `production-release` job.
- `docs/release.md` and `docs/release.zh-CN.md` document the variable/secret split and keep the
  environment names aligned with the repository settings.

## Validation

- Checked workflow references for `release-candidate`, `production-release`, `RELEASE_APP_*`, and
  the old `RELEASE_GITHUB_APP_*` names.
- Confirmed `actions/create-github-app-token@v2` still names its identifier input `app-id`.
- Workflow lint: `actionlint .github/workflows/*.yml`.
- Project journal validation:
  `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/f6ee40d956eb4261c5e414f8ef791f0bc8dd588d/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
- Whitespace check: `git diff --check`.

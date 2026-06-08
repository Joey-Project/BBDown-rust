---
id: 20260608-019ea7-release-governance-codeowners
title: Release Governance Code Owners
status: completed
created: 2026-06-08
updated: 2026-06-08
branch: wip/release-codeowners
pr:
supersedes: []
superseded_by:
---

# Release Governance Code Owners

## Summary

- Scope is CODEOWNERS coverage for release-critical automation before adding the release candidate
  and crates.io promotion workflows.
- Future release workflow changes should request review from `@JoeyTeng`, while implementation
  commits in this repository are authored by `JoeyTeng-Codex <codex@mahane.me>`.

## Current State

- `.github/CODEOWNERS` now assigns `@JoeyTeng` to the CODEOWNERS file itself, all GitHub Actions
  workflows, release packaging scripts, Cargo manifests and lockfile, and the planned release
  runbook docs.
- GitHub branch/ruleset configuration still needs to enable code owner review enforcement for
  matching pull requests.
- Offline frozen review found and the branch fixed missing ownership for `.github/CODEOWNERS`
  itself and `.github/workflows/ci.yml`.
- A follow-up offline frozen review found that the high-permission `codex-review-gate.yml`
  workflow also needed ownership, so the branch now covers all `.github/workflows/*.yml` and
  `.github/workflows/*.yaml` files.

## Validation

- Diff whitespace check: `git diff --check`.
- Project journal validation:
  `python3 .../project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.

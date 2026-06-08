---
id: 20260608-019ea7-release-automation
title: Release Automation
status: completed
created: 2026-06-08
updated: 2026-06-08
branch: wip/release-automation
pr:
supersedes:
  - 20260607-019e9eab-release-packaging
  - 20260607-019e9eab-crate-publish-readiness
superseded_by:
---

# Release Automation

## Summary

- Scope is the 0.1.0 release governance flow after CODEOWNERS landed.
- The release path is now RC-first: validate `master`, create a protected RC tag, then promote that
  RC tag to the final GitHub Release and `bbdown-core` crates.io publication.

## Current State

- `.github/workflows/release.yml` is now `Release Artifacts`, a reusable/manual artifact builder. It
  no longer publishes on arbitrary `v*` tag pushes.
- `.github/workflows/create-release-candidate.yml` validates `master`, runs formatter, clippy,
  tests, crates.io dry run, builds release archives, and creates an annotated `vX.Y.Z-rc.N` tag
  through the release GitHub App inside the `release-candidate` environment.
- `.github/workflows/promote-release-candidate.yml` must be dispatched from an RC tag. It reruns the
  same validation, rebuilds final artifacts, creates the final annotated `vX.Y.Z` tag, publishes the
  GitHub Release inside the `production-release` environment, and publishes `bbdown-core` inside the
  `crates-io` environment.
- Release archives now include the English and Simplified Chinese release runbooks alongside the
  existing README, user, embedding, architecture, and license files.
- `docs/release.md` and `docs/release.zh-CN.md` document required environments, secrets, tag
  rulesets, RC creation, promotion, and failure recovery.

## Validation

- Workflow lint: `actionlint .github/workflows/*.yml`.
- Shell syntax: `bash -n scripts/package-release.sh`.
- Shell lint: `shellcheck scripts/package-release.sh`.
- Release build: `cargo build -p bbdown-cli --bin bbdown --release --locked`.
- Local release package smoke:
  `scripts/package-release.sh target/release/bbdown bbdown-local-smoke .codex-tmp/release-package-smoke`.
- Local package content check: `tar -tzf .codex-tmp/release-package-smoke/bbdown-local-smoke.tar.gz`.
- Local package checksum check:
  `shasum -a 256 -c bbdown-local-smoke.tar.gz.sha256` from the package output directory.
- Local default gate: `just ci`.

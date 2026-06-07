---
id: 20260607-019e9eab-release-packaging
title: Release Packaging
status: completed
created: 2026-06-07
updated: 2026-06-07
branch: wip/release-packaging
pr: https://github.com/Joey-Project/BBDown-rust/pull/8
supersedes: []
superseded_by:
---

# Release Packaging

## Summary

- Eighth PR slice for the BBDown Rust rewrite continuation track.
- Scope is GitHub Actions binary release packaging for the `bbdown` CLI.
- The slice keeps crate publishing separate; crates.io dry-run readiness remains the next planned
  workstream.

## Current State

- `.github/workflows/release.yml` builds release artifacts on `v*` tag pushes and on manual
  workflow dispatch.
- Tag pushes publish a GitHub Release with generated notes; manual dispatch builds downloadable
  workflow artifacts without publishing a release.
- The release matrix covers Linux x86_64, macOS x86_64, macOS aarch64, and Windows x86_64.
- Unix archives are `.tar.gz`; Windows archives are `.zip`.
- Each package contains the `bbdown` CLI binary, `README.md`, and `docs/user-guide.md`; a `LICENSE`
  file is included automatically if one is added later.
- Each archive has an adjacent `.sha256` checksum file.
- `scripts/package-release.sh` packages Unix archives and is locally syntax/lint validated.
- `scripts/package-release.ps1` packages Windows archives for the GitHub Windows runner; local
  PowerShell execution is not available in this workspace.
- PR review follow-up pinned release workflow action references to commit SHAs and normalized package
  version fragments to `[A-Za-z0-9._-]` so legal tags such as SemVer build metadata do not fail
  during packaging.

## Evidence

- Shell syntax: `bash -n scripts/package-release.sh`.
- Shell lint: `shellcheck scripts/package-release.sh`.
- Workflow lint: `actionlint .github/workflows/release.yml`.
- Local release build: `cargo build -p bbdown-cli --bin bbdown --release --locked`.
- Local package smoke: `scripts/package-release.sh target/release/bbdown bbdown-local-smoke .codex-tmp/release-package-smoke`.
- Local default gate: `just ci`.
- Review follow-up checks also covered a `v1.2.3+build.1` package-name normalization smoke.

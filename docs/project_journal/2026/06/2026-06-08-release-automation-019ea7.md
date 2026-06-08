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
- The release path is now RC-first: validate the repository default branch, create a protected RC
  tag, then promote the latest RC tag for a version to the final GitHub Release and `bbdown-core`
  crates.io publication.

## Current State

- `.github/workflows/release.yml` is now `Release Artifacts`, a reusable/manual artifact builder. It
  no longer publishes on arbitrary `v*` tag pushes.
- `.github/workflows/create-release-candidate.yml` validates the repository default branch, runs
  formatter, clippy, declared MSRV check, tests, crates.io dry run, builds release archives,
  auto-selects the next `vX.Y.Z-rc.N` tag, and creates that annotated tag through the release GitHub
  App inside the `release-candidate` environment.
- `.github/workflows/promote-release-candidate.yml` must be dispatched from an RC tag. It reruns the
  same validation, rebuilds final artifacts, verifies the selected RC is the latest for that version,
  creates the final annotated `vX.Y.Z` tag, publishes the GitHub Release inside the
  `production-release` environment, and publishes `bbdown-core` inside the `crates-io` environment.
- RC and promotion validation both require `bbdown-core` and `bbdown-cli` Cargo versions to match
  the requested release version so GitHub Release archive names, CLI package metadata, and the
  crates.io package do not drift.
- RC creation is serialized per requested release version, and promotion is serialized per final
  release version, so concurrent manual runs cannot race the automatic RC number or duplicate the
  same final release.
- RC creation rejects versions that already have a final tag or GitHub Release, so maintainers cannot
  create dead-end RC tags after a version has already shipped.
- Promotion workflow runs are now serialized per final release version through the manual `version`
  input, and the selected RC is rechecked as latest immediately before final tag and GitHub Release
  creation.
- Release archives now include the English and Simplified Chinese release runbooks alongside the
  existing README, user, embedding, architecture, and license files.
- CI and release workflows now use runner-provided `rustup` with the floating stable channel from
  `rust-toolchain.toml`; third-party Rust setup and cache actions are intentionally not used. They
  also run `cargo +1.95.0 check --workspace --locked` to protect the declared crate `rust-version`.
- `docs/release.md` and `docs/release.zh-CN.md` document required environments, secrets, tag
  rulesets, RC creation, promotion, and failure recovery.
- Offline frozen review found that promotion recovery would fail if final tag creation succeeded but
  GitHub Release creation failed. The promotion workflow now reuses an existing final tag when it
  points at the same RC target commit and the release is still missing.

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
- Offline frozen review:
  `isolated_review stateful start --entrypoint codex-readonly --base-ref master --head-ref HEAD`.
- Independent Codex PR review found the automatic RC numbering concurrency window; the workflow now
  uses GitHub Actions concurrency groups to serialize runs by release version.
- GitHub Codex review flagged a hard-coded release branch check; the workflow now checks the
  repository default branch, which is currently `master`.
- Independent Codex PR review found stale RC promotion and missing MSRV coverage risks; promotion now
  rejects non-latest RC tags before publication and CI/release gates check the declared MSRV.
- Offline frozen review found that RC creation still allowed already-shipped versions; RC validation
  now rejects existing final tags and GitHub Releases before creating another candidate.

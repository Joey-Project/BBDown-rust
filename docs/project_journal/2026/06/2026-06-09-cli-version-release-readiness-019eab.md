---
id: 20260609-019eab-cli-version-release-readiness
title: CLI Version Release Readiness
status: completed
created: 2026-06-09
updated: 2026-06-09
branch: wip/cli-version-flag
pr:
supersedes:
superseded_by:
---

# CLI Version Release Readiness

## Summary

- The first 0.1.0 RC attempt reached release verification and artifact build successfully, then
  failed before tag creation because the release GitHub App input names did not match the configured
  environment secret and variable names.
- The CLI now exposes `bbdown --version`, so release archive users can confirm the installed binary
  version without needing a subcommand.

## Current State

- `release-candidate` and `production-release` currently use `RELEASE_APP_CLIENT_ID` as an
  environment variable and `RELEASE_APP_PRIVATE_KEY` as an environment secret.
- `crates-io` already has the expected `CARGO_REGISTRY_TOKEN` secret.
- The RC workflow run `27193553821` passed source verification and built all four artifacts, but did
  not create `v0.1.0-rc.1`.
- Downloaded RC artifacts from that failed run verified against their `.sha256` sidecars and
  contained the expected binary, README, bilingual user/release/architecture docs, and `LICENSE`.

## Validation

- `actionlint .github/workflows/*.yml`.
- `bash -n scripts/release/package-release.sh`.
- `bash -n scripts/release/common.sh`.
- `shellcheck scripts/release/package-release.sh scripts/release/common.sh`.
- `just ci`.
- `cargo build -p bbdown-cli --bin bbdown --release --locked`.
- Local package smoke with `scripts/release/package-release.sh`.
- Artifact checksum and content checks for run `27193553821`.

## Next Steps

- Align the release GitHub App secret names before retrying the failed RC tag creation job.
- After this PR lands, rerun the failed RC job and verify `bbdown --version` from one downloaded
  archive before promoting the RC.

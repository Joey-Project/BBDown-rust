---
id: 20260608-019ea775-bilingual-docs-live-e2e
title: Bilingual User Docs And Live E2E Verification
status: completed
created: 2026-06-08
updated: 2026-06-08
branch: wip/bilingual-docs-live-e2e
pr:
supersedes: []
superseded_by:
---

# Bilingual User Docs And Live E2E Verification

## Summary

- Scope is user-facing bilingual documentation and real live e2e verification before the 0.1.0
  release cut.
- Agent-facing project trackers and workstream journals remain English-only.
- The planned release strategy is GitHub Release first, then crates.io publish through a separate
  manually approved GitHub Actions environment with `CARGO_REGISTRY_TOKEN`.

## Current State

- English remains the default documentation path.
- Simplified Chinese companion docs exist for the root README, crate README, user guide, embedding
  guide, and architecture document.
- Every human-facing documentation pair starts with a `[ English | Simplified Chinese ]` language
  switch that links to the other version.
- Chinese documentation uses Chinese navigation labels for user guide, embedding guide,
  architecture, and related docs.
- The root README links to the original `nilaoda/BBDown` project and thanks it as a practical
  Bilibili behavior reference.
- Release packaging scripts now include English and Simplified Chinese README, user guide,
  embedding guide, and architecture docs so archive language links work after extraction.

## Live E2E Evidence

- Live manifest path: ignored `live-e2e.samples.json`.
- Access key path: ignored `access_key.txt`.
- Credentials source: Telegram Video Downloader's BBDown credential store.
- Restricted proxy: `https://atri.ink`, expanded to `cn`, `th`, `hk`, and `tw`.
- Covered samples:
  - normal public `https://www.bilibili.com/video/BV15hdwBKEMG`;
  - Hong Kong/Macau/Taiwan PGC `https://www.bilibili.com/bangumi/play/ep664928`;
  - mainland PGC `https://www.bilibili.com/bangumi/play/ep323085`.
- A first live run hit a transient `API response did not contain selected episode` failure.
- Per-case follow-up passed for the public sample and HK/MO/TW sample; the CN sample produced the
  manifest-allowed access-restricted proxy diagnostics.
- Final live gate passed: `just live-e2e`.

## Validation

- Shell syntax: `bash -n scripts/package-release.sh`.
- Shell lint: `shellcheck scripts/package-release.sh`.
- Release build: `cargo build -p bbdown-cli --bin bbdown --release --locked`.
- Release package smoke:
  `scripts/package-release.sh target/release/bbdown bbdown-bilingual-smoke .codex-tmp/bilingual-package-smoke`.
- Release package contents:
  `tar -tzf .codex-tmp/bilingual-package-smoke/bbdown-bilingual-smoke.tar.gz | sort`.
- Crate package listing: `cargo package --list -p bbdown --allow-dirty`.
- Default local gate: `just ci`.
- Live gate: `just live-e2e`.

## Notes

- Local `pwsh` is unavailable in this workspace, so `scripts/package-release.ps1` syntax was not
  parsed locally. It remains covered by the GitHub Windows release runner.
- `live-e2e.samples.json`, `access_key.txt`, release smoke artifacts, and build outputs remain
  ignored local files and must not be committed.

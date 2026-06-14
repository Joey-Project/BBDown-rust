# Project State

## Current State

- The Rust BBDown rewrite has a crate/CLI/CI foundation plus typed metadata, download planning, and
  download execution APIs.
- Latest completed workstreams:
  - `docs/project_journal/2026/06/2026-06-06-rust-rewrite-foundation-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-07-stream-planning-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-07-download-execution-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-07-qr-login-live-tests-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-07-restricted-area-proxy-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-07-live-e2e-matrix-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-07-release-packaging-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-07-crate-publish-readiness-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-08-stream-quality-selection-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-08-restricted-area-response-compat-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-08-integration-api-docs-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-08-download-archive-dedup-019e9eab.md`.
  - `docs/project_journal/2026/06/2026-06-08-bilingual-docs-live-e2e-019ea775.md`.
  - `docs/project_journal/2026/06/2026-06-08-release-automation-019ea7.md`.
  - `docs/project_journal/2026/06/2026-06-09-release-0-1-0-019ead.md`.
- The originally planned rewrite continuation slices are complete through download archive and
  duplicate decision handling.
- Human-facing docs now have English and Simplified Chinese versions, and the latest real live e2e
  gate passed against the configured public and restricted-area samples.
- The publishable library package is named `bbdown-core`; Rust code imports it as `bbdown_core`.
- First-release automation now uses a protected RC tag workflow followed by RC promotion to GitHub
  Release and crates.io.
- Version `0.1.0` has shipped through that flow as GitHub Release `v0.1.0` and crates.io package
  `bbdown-core` `0.1.0`.
- This branch is the `0.2.0` release source. After the RC workflow provenance backport lands here,
  run `Create Release Candidate` from `release/0.2.0` and verify the created RC tag points at the
  release branch HEAD used by that workflow run.

## Recovery Pointers

- Run `just ci` for the local default gate after dependencies are restored.
- Workstream detail and PR-local state should live under `docs/project_journal/`.
- User-facing CLI behavior is documented in `docs/user-guide.md`.
- Crate embedding guidance is documented in `docs/embedding.md`.
- Simplified Chinese companion docs use `*.zh-CN.md` next to the English originals.
- Maintainer release steps are documented in `docs/release.md` and `docs/release.zh-CN.md`.

## Global Blockers

- None currently recorded.

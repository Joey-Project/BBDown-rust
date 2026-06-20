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
  - `docs/project_journal/2026/06/2026-06-13-app-grpc-playurl-mode-019eb8.md`.
  - `docs/project_journal/2026/06/2026-06-13-version-0-3-0-bump-019eb8.md`.
  - `docs/project_journal/2026/06/2026-06-13-release-0-2-0-branch-rc2-019ec2.md`.
  - `docs/project_journal/2026/06/2026-06-14-feed-list-resolver-abstraction-019ec6.md`.
  - `docs/project_journal/2026/06/2026-06-14-history-feed-list-parsing-019ec7.md`.
  - `docs/project_journal/2026/06/2026-06-14-following-dynamic-feed-parsing-019ec8.md`.
  - `docs/project_journal/2026/06/2026-06-14-recommendation-feed-parsing-019ec9.md`.
  - `docs/project_journal/2026/06/2026-06-14-watch-later-feed-parsing-019eca.md`.
- The originally planned rewrite continuation slices are complete through download archive and
  duplicate decision handling.
- Human-facing docs now have English and Simplified Chinese versions, and the opt-in real live e2e
  harness remains available for configured public and restricted-area samples.
- The publishable library package is named `bbdown-core`; Rust code imports it as `bbdown_core`.
- First-release automation now uses a protected RC tag workflow followed by RC promotion to GitHub
  Release and crates.io.
- Versions `0.1.0`, `0.2.0`, `0.3.0`, `0.4.0`, and `0.5.0` have shipped through that flow as GitHub
  Releases and crates.io package versions for `bbdown-core`.
- The `0.4.0` line shipped credential lifecycle improvements, access-key acquisition, unified login
  QR output, and append-only danmaku update workflows.
- The completed `0.5.0` development line shipped downloader and embedding polish. Progress
  callbacks, terminal progress events/report summaries, cancellation-aware download execution,
  chapter metadata muxing, audio language selection, AI subtitle filtering, and release-note
  archive packaging are now published.
- The active `0.6.0` planning line is credential lifecycle work: automatic credential refresh,
  health policy/reporting, profile-level lifecycle status, and multi-account lifecycle UX.
- The release-prep deterministic gate passed. The latest local live e2e rerun is still blocked by
  upstream restricted PGC proxy `502 Bad Gateway` responses in the ignored manifest.
- Repo-local skill `$bbdown-live-e2e-fixtures` and `live-e2e.samples.example.json` record the current
  real Bilibili fixtures for opt-in normal, multi-page, and restricted-area live e2e validation.

## Recovery Pointers

- Run `just ci` for the local default gate after dependencies are restored.
- Workstream detail and PR-local state should live under `docs/project_journal/`.
- Completed v0.4.0 feature roadmap:
  `docs/project_journal/2026/06/2026-06-15-v0-4-credential-danmaku-roadmap-019ecf.md`.
- Completed v0.4.0 release prep:
  `docs/project_journal/2026/06/2026-06-18-v0-4-release-prep-019f0a.md`.
- Completed v0.5.0 roadmap:
  `docs/project_journal/2026/06/2026-06-18-v0-5-downloader-embedding-roadmap-019f0b.md`.
- Active v0.6.0 roadmap:
  `docs/project_journal/2026/06/2026-06-20-v0-6-credential-lifecycle-roadmap-019f16.md`.
- Repo-local live e2e fixture skill:
  `.agents/skills/bbdown-live-e2e-fixtures/SKILL.md`.
- User-facing CLI behavior is documented in `docs/user-guide.md`.
- Crate embedding guidance is documented in `docs/embedding.md`.
- Simplified Chinese companion docs use `*.zh-CN.md` next to the English originals.
- Maintainer release steps are documented in `docs/release.md` and `docs/release.zh-CN.md`.

## Global Blockers

- None for deterministic release validation. Opt-in restricted PGC live e2e remains dependent on
  upstream proxy/fixture health.

[ English | [简体中文](README.zh-CN.md) ]

# Release Notes

This directory keeps human-written release notes for the Rust rewrite. GitHub Releases remain the
source for downloadable assets and generated changelogs; these notes summarize the user-facing and
embedding-facing shape of each source line and shipped release.

## Versions

- [v0.11.0 (published 2026-10-20; source-preparation notes)](v0.11.0.md): domestic PGC episode
  groups, ordinary-video search, anime/Chinese-animation timetable, movie/TV category indexes,
  popular videos, and global ranking metadata. [GitHub Release](https://github.com/Joey-Project/BBDown-rust/releases/tag/v0.11.0),
  [crates.io](https://crates.io/crates/bbdown-core/0.11.0).
- [v0.10.0 source preparation (unpublished as of 2026-10-14)](v0.10.0.md): related
  recommendations, bounded metadata pages and SpaceDynamic cookie initialization, plus expanded
  opt-in CDN measurements.
- [v0.9.0 (published as of 2026-10-14)](v0.9.0.md): structured CDN transfer diagnostics for
  progress callbacks and CLI JSON, plus WBI signing for personal-space dynamic feed requests and
  continuation pages.

For current publication status, see
[official GitHub Releases](https://github.com/Joey-Project/BBDown-rust/releases).

- [v0.8.0 (published)](v0.8.0.md): opt-in danmaku refresh that preserves XML history, rebuilds
  selected ASS outputs from merged XML, and stages generated sidecars with the updated archive for
  publication. Existing custom ASS styles and events are discarded.

- [v0.7.0](v0.7.0.md): opt-in CDN host selection, bounded probing, parallel Range transfer,
  bundled network catalogs, and independent PGC Web playurl routing.
- [v0.6.0](v0.6.0.md): credential lifecycle integration for profile status, provider-aware
  access-key refresh, credential preflight, and safer multi-account updates.
- [v0.5.0](v0.5.0.md): downloader and embedding polish for progress callbacks, cancellation,
  chapter metadata, audio language selection, and AI subtitle filtering.
- [v0.4.0](v0.4.0.md): credential lifecycle groundwork, credential profiles, access-key login, and
  append-only danmaku update workflows.
- [v0.3.0](v0.3.0.md): feed/list parsing for history, following/dynamic feeds, homepage
  recommendations, and watch-later.
- [v0.2.0](v0.2.0.md): BBDown parity expansion for batch inputs, sidecars, single-output modes,
  host controls, playback request planning, and APP/TV playurl modes.
- [v0.1.0](v0.1.0.md): first protected release with the crate/CLI foundation, planning, downloads,
  restricted-area proxy fallback, live e2e, archive duplicate handling, bilingual docs, and release
  automation.

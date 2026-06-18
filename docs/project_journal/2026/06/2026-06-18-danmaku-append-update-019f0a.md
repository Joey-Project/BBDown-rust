---
id: 20260618-019f0a-danmaku-append-update
title: Append-Only Danmaku Update
status: completed
created: 2026-06-18
updated: 2026-06-18
branch: wip/danmaku-append-update
pr: 49
supersedes: []
superseded_by:
---

# Append-Only Danmaku Update

## Summary

- Completes PR 8 of the `0.4.0` credential and danmaku sequence.
- Adds archive-backed append-only danmaku updates for already downloaded entries.
- Exposes the workflow through both `bbdown-core` and the CLI while keeping XML as the canonical
  update target and regenerating selected derived formats such as ASS.

## Current State

- `bbdown-core` exposes `merge_xml_append_only`, `DanmakuXmlMerge`, `DanmakuUpdateOptions`,
  `DanmakuUpdateReport`, and `BiliClient::update_danmaku_for_archive`.
- XML merge works at the `<d ...>...</d>` comment-block level, deduplicating by the `p` attribute and
  decoded comment text so equivalent XML entity spellings do not append duplicates.
- Archive-backed updates match selected plan entries to existing archive records by stable aid/cid
  identity, download the current XML danmaku payload, append only new comments into `danmaku.xml`,
  regenerate requested derived formats such as `danmaku.ass`, and record updated sidecar paths back
  into the archive entry. Updates also refresh matching archive content keys for the selected
  danmaku formats while keeping partially updated multi-entry records conservative.
- CLI `bbdown danmaku update <input> --archive-file <path>` exposes the same flow with `--select`,
  repeatable/comma-delimited `--danmaku-format`, retry/idle-timeout controls, and JSON reporting.
- English and Simplified Chinese README, user guide, embedding guide, and architecture docs describe
  the user-facing command, crate API, and architecture boundary.

## Evidence

- `cargo fmt --all --check` passed.
- `cargo test -p bbdown-core danmaku --locked` passed: 12 related tests.
- `cargo test -p bbdown-core write_generated_text_file_replaces_existing_target --locked` passed.
- `cargo test -p bbdown-cli danmaku_update --locked` passed: 9 CLI e2e tests plus filtered targets.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- Internal `codex-readonly` review found that CLI archive overlap checks ran after danmaku update
  writes. The CLI now rejects `--archive-file` paths that overlap `danmaku.xml`,
  `danmaku.xml.bbdown-source`, `danmaku.xml.bbdown-replace`, selected derived sidecars such as
  `danmaku.ass`, and generated sidecar replace paths such as `danmaku.ass.bbdown-replace` before
  any update writes begin.
- Independent PR review found that generated text staging reused the same `.bbdown-replace` path as
  the Windows fallback backup path. Generated text writes now use a distinct `.bbdown-generated`
  staging path, and the CLI preflight rejects archive paths that overlap those generated staging
  sidecars before any update writes begin.
- GitHub Codex review found that CLI danmaku updates used full download planning and could fail when
  media stream playurl resolution was unavailable but danmaku metadata was still accessible. The CLI
  now plans updates with `DownloadMode::DanmakuOnly`, and CLI e2e coverage uses metadata-only mocks
  for `danmaku update` so the command does not depend on playurl access.
- Independent PR review found that the embedding guide still used full `plan_download` for archive
  danmaku updates. The English and Simplified Chinese examples now use
  `plan_download_with_mode(..., DownloadMode::DanmakuOnly)`.
- Offline frozen `codex-readonly` review found that CLI archive overlap preflight missed source XML
  download's `.bbdown-download` and `.bbdown-replace` temporary paths. The preflight now guards those
  paths, and CLI e2e coverage rejects archive files at both derived locations before network writes.
- Offline frozen `codex-readonly` review found that append-only updates refreshed archive sidecar
  paths without refreshing the matching danmaku-format content keys. The core update path now refreshes
  affected entry keys and only refreshes aggregate record keys when every entry in that record reflects
  the selected danmaku formats.
- GitHub Codex review found that appending new comments after a self-closing empty root such as
  `<i/>` produced malformed XML. Empty existing XML, including self-closing roots with no comments,
  now uses the fetched XML as the merged output.
- GitHub Codex and offline frozen review found that `danmaku update --danmaku-format ass` wrote both
  canonical XML and ASS but refreshed archive content keys as ASS-only. The update path now computes
  effective archive formats as XML plus the requested formats, so archive duplicate/preflight matching
  reflects the actual sidecars refreshed by the update.
- `cargo test -p bbdown-core download::tests::danmaku_update --locked` passed: 2 related tests.
- `cargo test -p bbdown-core danmaku::tests::merge_xml_append_only --locked` passed: 5 related tests.
- `cargo test -p bbdown-cli danmaku_update --locked` passed: 9 CLI e2e tests plus filtered targets.
- `just ci` passed: format, clippy, Rust 1.95 workspace check, workspace tests, CLI e2e, and
  `cargo publish --dry-run -p bbdown-core --locked --allow-dirty`.

## Next Steps

- Open the PR and run the normal CI plus triple-review merge gate.
- After this lands, prepare the `0.4.0` release/versioning step before starting the next feature
  train.

---
id: 20260608-019e9eab-download-archive-dedup
title: Download Archive And Duplicate Decisions
status: completed
created: 2026-06-08
updated: 2026-06-08
branch: wip/download-archive-dedup
pr: https://github.com/Joey-Project/BBDown-rust/pull/13
supersedes: []
superseded_by:
---

# Download Archive And Duplicate Decisions

## Summary

- Thirteenth PR slice for the BBDown Rust rewrite continuation track.
- Scope is download archive and duplicate handling for both embedding callers and CLI users.
- MP4Box muxing and subtitle-to-SRT conversion remain outside this slice.

## Current State

- The `bbdown` crate exposes `DownloadArchive`, archive records, `DownloadPreflight`, output
  conflict reporting, and `DuplicateDecision`.
- Embedding callers can inspect archive hits, entry-level archive overlaps, and output conflicts
  before execution, then decide to replace, keep both, or cancel in their own UI.
- The CLI supports `--archive-file` plus `--on-duplicate replace|keep-both|cancel`.
- JSON/non-TTY download mode does not prompt when a duplicate decision is required; it fails with a
  clear instruction unless the caller passed `--on-duplicate`.
- Human TTY mode can prompt on stderr so stdout stays usable for normal output.
- Archive records keep content identity, output paths, entry ids, sidecar paths, mux output paths,
  and completion timestamps without storing media URLs or credentials.
- `replace` removes the existing planned output root before a fresh download, so stale sidecars or
  mux outputs from the previous duplicate run do not remain.
- User-facing docs, embedding docs, architecture docs, and top-level project state/TODO point to the
  archive and duplicate decision behavior.

## Evidence

- Targeted crate coverage:
  `cargo test --locked -p bbdown download_preflight_reports_archive_hit_and_output_conflict`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_preflight_reports_entry_overlap_from_archive`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown archive_decision_keep_both_uses_new_output_root`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown archive_decision_replace_forces_fresh_writes`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_archive_round_trips_without_urls`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_archive_save_replaces_existing_file`.
- Targeted CLI archive coverage:
  `cargo test --locked -p bbdown-cli download_archive_`.
- Workspace tests: `cargo test --workspace --locked`.
- Project journal validation passed with the project-journal helper.
- Full local gate: `just ci`.

## Next Steps

- Continue with the remaining roadmap slices after this PR lands; MP4Box muxing and subtitle-to-SRT
  conversion are intentionally deferred.

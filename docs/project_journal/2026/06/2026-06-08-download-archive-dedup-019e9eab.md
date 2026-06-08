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
  mux outputs from the previous duplicate run do not remain, and completed records replace stale
  archive records that pointed at the same normalized output path.
- `keep-both` reserves matching archive record output paths even when those paths are no longer on
  disk, so archive-only duplicate history is preserved across equivalent path spellings.
- Entry-level archive identities ignore display indexes, so reordered pages or episodes can still
  be detected as duplicates by stable aid/bvid/cid/epid content ids.
- Output-root occupancy uses symlink metadata, so broken symlink roots are treated as existing roots
  for preflight, keep-both candidate selection, and replace cleanup. Non-`NotFound` metadata
  errors are reported to callers instead of being retried as suffixed keep-both roots.
- The CLI rejects `--archive-file` and archive save sidecar paths when they overlap the chosen
  output root either lexically or through canonical targets, and `DownloadArchive::save` rejects
  directory targets before replacing archive files. `DownloadArchive::load` reports metadata errors
  instead of treating unreadable archive paths as empty archives.
- User-facing docs, embedding docs, architecture docs, and top-level project state/TODO point to the
  archive and duplicate decision behavior.

## Evidence

- Targeted crate coverage:
  `cargo test --locked -p bbdown download_preflight_reports_archive_hit_and_output_conflict`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_preflight_reports_entry_overlap_from_archive`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_preflight_reports_entry_overlap_after_index_change`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_entry_content_key_ignores_display_index`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_archive_load_reports_metadata_errors`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown archive_decision_keep_both_uses_new_output_root`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown archive_decision_keep_both_avoids_archive_only_output_root`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown archive_decision_replace_forces_fresh_writes`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown archive_decision_replace_removes_broken_symlink_output_root`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown archive_decision_keep_both_skips_broken_symlink_output_root`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_preflight_reports_broken_symlink_output_conflict`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown output_occupancy_reports_metadata_errors`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_preflight_`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown archive_decision_`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_archive_round_trips_without_urls`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_archive_save_replaces_existing_file`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_archive_save_rejects_directory_path`.
- Targeted CLI archive coverage:
  `cargo test --locked -p bbdown-cli download_archive_`.
- Targeted CLI unit coverage:
  `cargo test --locked -p bbdown-cli archive_file_guard_rejects_output_root_overlap`.
- Targeted CLI unit coverage:
  `cargo test --locked -p bbdown-cli archive_file_guard_rejects_sidecar_output_root_overlap`.
- Targeted CLI unit coverage:
  `cargo test --locked -p bbdown-cli archive_file_guard_rejects_lexical_symlink_inside_output_root`.
- Targeted CLI unit coverage:
  `cargo test --locked -p bbdown-cli duplicate_decision_prompt_state_tracks_displayed_preflight`.
- Workspace tests: `cargo test --workspace --locked`.
- Formatter and lint coverage:
  `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
  `git diff --check`.
- Project journal validation passed with the project-journal helper.
- Full local gate: `just ci`.

## Next Steps

- Continue with the remaining roadmap slices after this PR lands; MP4Box muxing and subtitle-to-SRT
  conversion are intentionally deferred.

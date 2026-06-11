---
id: 20260611-019eb2
title: Filename Templates
status: completed
created: 2026-06-11
updated: 2026-06-11
branch: wip/filename-templates
pr:
supersedes: []
superseded_by:
---

# Filename Templates

## Summary
- Added customizable output-root, entry-directory, and mux-file-stem templates for download
  execution.
- Exposed the same behavior through `bbdown download` CLI flags and the reusable
  `bbdown-core` API.
- Kept media, cover, subtitle, and danmaku sidecar names stable for resume behavior, duplicate
  track handling, and archive records.

## Current State
- `DownloadPathTemplates` supports plan placeholders for output directories and entry placeholders
  for entry and mux names.
- Template validation rejects duplicate rendered entry directories and excessive numeric padding
  widths before writing or deleting output paths.
- CLI users can pass `--output-template`, `--entry-template`, and `--mux-template`.
- Human-facing English and Simplified Chinese docs describe the supported placeholders and escaping
  rules.

## Next Steps
- Continue the remaining BBDown parity backlog from `docs/PROJECT_TODO.md`: app/TV playurl modes,
  richer selection syntax, API/server integration surfaces, aria2 or multi-thread download
  integration, MP4Box muxing, and subtitle-to-SRT conversion.

## Evidence
- Local targeted validation: `cargo test --locked -p bbdown-core path_templates`.
- Local targeted validation:
  `cargo test --locked -p bbdown-cli --test cli_e2e download_json_applies_path_templates`.

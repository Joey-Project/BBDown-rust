---
id: 20261116-pdar01
title: Public Danmaku ASS Renderer API
status: completed
created: 2026-11-16
updated: 2026-11-16
branch: codex/public-danmaku-renderer
pr:
supersedes: []
superseded_by:
---

# Public Danmaku ASS Renderer API

## Summary

- Expose the existing validated Bilibili XML-to-ASS renderer to embedding clients without
  changing normal download behavior.

## Current State

- `bbdown_core::render_danmaku_ass(&str) -> bbdown_core::Result<String>` uses the same strict XML
  parsing, `<i>` root validation, namespace handling, entity decoding, and renderer as downloads.
- The public path accepts at most 16 MiB of XML and 50,000 direct `<d p="...">` comments. It
  checks a conservative 48 MiB ASS output estimate before rendering. The parser has no separate
  node-count limit; its tree allocation is bounded by the input size.
- A focused external integration test covers multilingual old and appended comments, empty valid
  input, namespaced XML, entity handling, malformed XML, DTDs, foreign roots, and resource limits.
- CLI/download code continues using its existing validated conversion path and output-size checks.

## Validation

- Full workspace tests passed: 1,043 passed, zero failed, four existing opt-in
  tests ignored. The new external public-renderer integration suite passed all
  five cases, including old/local-only and newly appended multilingual comments.
- Workspace/all-targets Clippy passed with warnings denied. Formatting,
  whitespace checks and project-journal validation passed.
- Validation used local synthetic/mock inputs and Rust/Cargo 1.99.1. No live
  Bilibili account, media-library files or deployment was used.

## Integration boundary

- Embedding applications still own fetch policy, file identity/content checks,
  presentation styling and publication. This API performs no network or file I/O.
- The downloader bot can combine preserving XML merge with this renderer and
  publish sidecars through its own bound-filesystem transaction layer.

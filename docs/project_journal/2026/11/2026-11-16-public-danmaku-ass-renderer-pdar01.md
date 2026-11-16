---
id: 20261116-pdar01
title: Public Danmaku Decode and ASS Rendering APIs
status: completed
created: 2026-11-16
updated: 2026-11-16
branch: codex/public-danmaku-renderer
pr: "https://github.com/Joey-Project/BBDown-rust/pull/125"
supersedes: []
superseded_by:
---

# Public Danmaku Decode and ASS Rendering APIs

## Summary

- Expose the existing bounded decoder and validated XML-to-ASS renderer to embedding
  clients without coupling those operations to network or filesystem publication.

## Current State

- `bbdown_core::render_danmaku_ass(&str) -> bbdown_core::Result<String>` uses the same strict XML
  parsing, `<i>` root validation, namespace filtering, entity decoding, and renderer as downloads.
- `bbdown_core::decode_danmaku_xml(&[u8]) -> bbdown_core::Result<String>` exposes the existing
  bounded plaintext/gzip/zlib/raw-DEFLATE decoding and strict XML validation used by downloads.
  Raw and decompressed payloads are each limited to 16 MiB; the API performs no I/O.
- The public path accepts at most 16 MiB of XML and 50,000 direct `<d p="...">` comments. It
  checks a conservative 48 MiB ASS output estimate before rendering. The parser has no separate
  node-count limit; its tree allocation is bounded by the input size.
- ASS rendering extracts parameters and decoded text directly from parsed comment nodes; it does
  not build the XML-preservation representation or clone inherited namespace maps per comment.
- XML preservation rejects repeated per-comment namespace metadata over 8 MiB or 65,536 bindings,
  checked before copying those bindings into raw-comment records.
- A focused external integration test covers multilingual old and appended comments, empty valid
  input, namespaced XML, entity handling, malformed XML, DTDs, foreign roots, resource limits, and
  a large inherited namespace shared by many comments.
- A focused external decoder test covers plaintext and all three compressed formats, malformed and
  unsafe XML, truncated/trailing raw DEFLATE, and raw/decompressed size limits.
- CLI/download code continues using its existing validated conversion path and output-size checks.

## Validation

- Full workspace regression run passed: 1,045 passed, zero failed, four existing opt-in
  tests ignored. The new external public-renderer integration suite passed all
  six cases, including old/local-only and newly appended multilingual comments.
- The final core-only regression run passed 754 tests, with two existing opt-in
  tests ignored, including all four new public decoder tests.
- Workspace/all-targets Clippy passed with warnings denied. Formatting,
  whitespace checks and project-journal validation passed.
- Validation used local synthetic/mock inputs and Rust/Cargo 1.99.1. No live
  Bilibili account, media-library files or deployment was used.

## Integration boundary

- Embedding applications still own fetch policy, file identity/content checks,
  presentation styling and publication. This API performs no network or file I/O.
- The downloader bot can combine preserving XML merge with this renderer and
  publish sidecars through its own bound-filesystem transaction layer.

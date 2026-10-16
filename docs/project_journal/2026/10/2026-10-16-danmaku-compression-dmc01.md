---
id: 20261016-dmc01
title: Danmaku Transport Compression
status: completed
created: 2026-10-16
updated: 2026-10-16
branch: wip/danmaku-raw-deflate
pr:
supersedes: []
superseded_by:
---

# Danmaku Transport Compression

## Evidence

- A downstream bot stored raw DEFLATE bytes as XML and emitted a header-only
  ASS despite an XML pool containing 3,600 recoverable comments.
- Reqwest's optional deflate feature uses zlib framing and rejects raw
  DEFLATE, so enabling it alone is insufficient for the observed response.

## Changes

- Normalize bounded danmaku payloads before XML parsing and ASS generation,
  accepting plain UTF-8 XML, raw DEFLATE, zlib, and gzip.
- Reject malformed or oversized payloads rather than silently publishing an
  empty ASS generated from invalid XML. Leave media streaming unchanged.

## Validation

- Offline regression tests exercise compressed HTTP fixtures and malformed,
  truncated, and oversized payloads without production credentials or media.
- Rust 1.95.0 workspace tests passed: 862 passed, zero failed, and three
  existing opt-in tests ignored. The separate CLI e2e repeat passed all 159
  tests. HTTP compression fixtures exceed the decoder's per-chunk buffer.
- Formatting, workspace all-target Clippy with warnings denied, declared-MSRV
  check, and project journal validation passed.

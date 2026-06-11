---
id: 20260611-019eb2-danmaku-ass-format
title: Danmaku ASS Format
status: completed
created: 2026-06-11
updated: 2026-06-11
branch: wip/danmaku-ass-format
pr:
supersedes: []
superseded_by:
---

# Danmaku ASS Format

## Summary

- Adds opt-in ASS-format danmaku sidecar generation for the crate and CLI while preserving XML as
  the default format.
- Keeps `DownloadEntry.danmaku.xml_url` as the canonical planned endpoint and performs XML-to-ASS
  conversion in the download executor.
- Leaves UPOS host replacement and PCDN handling for the next PR slice.

## Current State

- `bbdown-core` exposes `DanmakuFormat::{Xml, Ass, Both}` and
  `DownloadOptions::with_danmaku_format(...)`.
- CLI `download` exposes `--danmaku-format xml|ass|both`; `xml` remains the default.
- XML output continues to report `DownloadFileKind::Danmaku` and write `danmaku.xml`.
- ASS output reports `DownloadFileKind::DanmakuAss` and writes `danmaku.ass`.
- ASS generation supports common scrolling, reverse-scrolling, top, and bottom comments. Advanced
  positioned danmaku comments are skipped instead of emitting unreliable coordinates.
- `--only danmaku --danmaku-format ass` writes only `danmaku.ass`; `both` writes both XML and ASS.
- Download archive matching distinguishes ASS/Both danmaku outputs from XML-only danmaku outputs
  while preserving legacy XML keys.

## Evidence

- Core unit tests cover XML-to-ASS conversion, XML entity decoding, ASS escaping, color conversion,
  common modes, and advanced-comment skipping.
- Core download tests cover ASS-only danmaku sidecar generation without media streams and `both`
  format reporting.
- Core archive tests cover that ASS-only danmaku records do not satisfy XML-only danmaku preflights.
- CLI e2e tests cover `--only danmaku --danmaku-format ass` and `both` JSON/report behavior.
- English and Simplified Chinese README, user guide, embedding guide, and architecture docs describe
  the new controls.
- Full local and PR validation evidence is recorded in the PR before merge.

## Next Steps

- Open the PR and run the normal CI plus triple-review merge gate.
- After this lands, start the UPOS host replacement and PCDN handling slice on a fresh branch.

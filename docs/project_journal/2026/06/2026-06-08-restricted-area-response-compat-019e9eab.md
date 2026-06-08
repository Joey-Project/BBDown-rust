---
id: 20260608-019e9eab-restricted-area-response-compat
title: Restricted-Area Proxy Response Compatibility
status: completed
created: 2026-06-08
updated: 2026-06-08
branch: wip/restricted-area-response-compat
pr: https://github.com/Joey-Project/BBDown-rust/pull/11
supersedes: []
superseded_by:
---

# Restricted-Area Proxy Response Compatibility

## Summary

- Eleventh PR slice for the BBDown Rust rewrite continuation track.
- Scope is restricted-area PGC playurl proxy response compatibility.
- The slice keeps proxy configuration explicit; no public proxy defaults are added.

## Current State

- `PlayUrlRoot` now accepts official `data` / `result` wrappers and helper-style top-level playurl
  payloads.
- Top-level helper payloads can expose DASH tracks, FLV `durl` segments, `timelength`, and quality
  metadata without an additional wrapper object.
- Existing nested `video_info` and intl `playurl` conversion behavior is preserved.
- Restricted-area proxy fallback still requires the official PGC playurl response to clearly report
  an area or region restriction.
- Proxy requests still omit Bilibili cookies and continue to redact access keys, proxy tokens,
  cookies, URL userinfo, paths, and query strings from diagnostics.

## Evidence

- Unit coverage for top-level BPplayurl-style FLV payloads.
- Unit coverage for top-level mobile/helper DASH payloads with quality metadata.
- CLI mock e2e coverage for restricted-area proxy fallback using a top-level DASH playurl response.
- Existing resolver ordering and diagnostic redaction coverage remains in place.

## Next Steps

- Continue with integration API and embedding documentation hardening.

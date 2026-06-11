---
id: 20260611-019eb2-upos-pcdn-controls
title: UPOS PCDN Controls
status: completed
created: 2026-06-11
updated: 2026-06-11
branch: wip/upos-pcdn-controls
pr: https://github.com/Joey-Project/BBDown-rust/pull/24
supersedes: []
superseded_by:
---

# UPOS PCDN Controls

## Summary

- Adds media host replacement controls for DASH and FLV media downloads.
- Keeps planned media URLs unchanged and applies `MediaHostOptions` only when building concrete
  download candidates.
- Documents the API/CLI split: crate defaults preserve URLs, while CLI defaults avoid PCDN-like
  non-local media candidates.

## Current State

- `bbdown-core` exposes `MediaHostOptions` and `DownloadOptions::with_media_hosts(...)`.
- `MediaHostOptions::default()` preserves upstream media URLs and allows PCDN candidates.
- `MediaHostOptions::bbdown_cli_default()` keeps BBDown-like PCDN avoidance for CLI callers.
- CLI `download` exposes `--upos-host <HOST>`, `--force-replace-host`, and `--allow-pcdn`.
- Host replacement applies only to DASH and FLV media candidates; cover, subtitle, and danmaku
  sidecar URLs are not rewritten.
- Localhost, private IPv4, and local/private IPv6 hosts are preserved by PCDN fallback handling so
  mock servers and private proxies keep working.

## Evidence

- Core unit tests cover default URL preservation, PCDN-like fallback, local host preservation,
  custom UPOS host rewriting, forced fallback rewriting, and candidate deduplication.
- Core download tests cover a custom UPOS host rewrite through the real media download path.
- CLI e2e tests cover `--upos-host` rewriting remote media candidates to a mock server and
  rejecting `--upos-host` values that include path/query/fragment data.
- English and Simplified Chinese README, user guide, embedding guide, crate README, and architecture
  docs describe the new controls and API defaults.

## Next Steps

- Open the PR and run the normal CI plus triple-review merge gate.
- After this lands, prepare the next release candidate for the new parity features.

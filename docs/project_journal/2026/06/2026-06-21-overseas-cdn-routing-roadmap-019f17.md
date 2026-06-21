---
id: 20260621-019f17-overseas-cdn-routing-roadmap
title: Overseas CDN Routing Roadmap
status: active
created: 2026-06-21
updated: 2026-06-21
branch: feature/overseas-cdn-routing-roadmap
pr:
supersedes: []
superseded_by:
---

# Overseas CDN Routing Roadmap

## Summary

- Overseas playback/download experience is a high-priority follow-up after the current credential
  lifecycle line.
- The current downloader already supports explicit `--upos-host`, `--force-replace-host`, and PCDN
  filtering controls, but it does not yet provide a first-class overseas routing preset or
  embeddable host-selection policy.
- CCB (`https://github.com/Kanda-Akihito-Kun/ccb`) is a useful research reference. Its README
  describes custom Bilibili playback-source switching for ordinary videos, live rooms, bangumi, and
  watch-later pages. It also documents strong replacement of `baseUrl` and `backupUrl`, PCDN
  avoidance effects, and overseas user reports that Hong Kong nodes can improve ordinary video
  playback.
- CCB's `data/cdn.json` and `data/region.json` expose a region-to-host catalog that currently
  includes regions such as Hong Kong and overseas hosts including Akamai and overseas Bilibili mirror
  candidates.

## Design Direction

- Treat CCB as a reference and data-source candidate, not as a hard runtime dependency.
- Keep `bbdown-core` deterministic and embeddable:
  - expose host-routing policy structs for CLI and API callers;
  - support explicit host lists and named presets;
  - preserve existing manual `upos_host` behavior;
  - keep region lock/proxy behavior separate from CDN acceleration;
  - avoid claiming that CDN switching bypasses restricted-area licensing.
- Prefer opt-in probing and clear fallback semantics:
  - optionally rank candidate hosts by lightweight HEAD/range probes against selected media URLs;
  - keep original Bilibili URLs and backup URLs available in reports;
  - record which host policy rewrote each media request;
  - fail back to original candidates when a selected host lacks content or returns incompatible
    responses.

## Candidate PR Slices

- PR A: document and expose `MediaHostPolicy`/`MediaHostPreset` as a stable API layer over the
  existing host rewrite and PCDN filtering controls.
- PR B: add CLI/API support for named overseas presets and explicit ordered host pools, while
  keeping existing `--upos-host` as the simplest manual override.
- PR C: add optional host probing/ranking for embedders and CLI dry-run diagnostics, bounded to
  small range requests and disabled by default.
- PR D: add live e2e fixture notes for overseas routing using public normal-video samples, with
  restricted-area behavior documented as orthogonal to CDN selection.

## Open Questions

- Whether to vendor a curated host catalog, let users provide catalogs, or periodically refresh a
  generated catalog in the release process.
- Whether overseas presets should default to Hong Kong-first, Akamai-first, or user-location-first.
- How much probing should be permitted by default without creating unnecessary traffic.
- Whether downloader archive/cache records should include the selected media host policy as
  diagnostic metadata without changing content identity.

## Evidence

- CCB repository: `https://github.com/Kanda-Akihito-Kun/ccb`.
- CCB README states that it supports custom Bilibili playback-source switching and covers ordinary
  videos, live rooms, bangumi, watch-later, and speed-test pages.
- CCB README describes strong replacement of ordinary video `baseUrl` and `backupUrl`, possible PCDN
  avoidance effects, and overseas user reports for Hong Kong nodes.
- CCB `data/region.json` currently lists Hong Kong and overseas regions.
- CCB `data/cdn.json` currently contains overseas host candidates such as
  `upos-hz-mirrorakam.akamaized.net`, `upos-sz-mirroraliov.bilivideo.com`, and
  `upos-sz-mirrorcosov.bilivideo.com`.

## Next Steps

- Keep the current `v0.6.0` credential lifecycle sequence as the active release line.
- Revisit this roadmap before the next downloader/playback polish release and decide whether it
  becomes `v0.7.0` or a dedicated overseas-experience release.

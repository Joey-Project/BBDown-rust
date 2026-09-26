---
id: 20260621-019f17-overseas-cdn-routing-roadmap
title: Overseas CDN Routing Roadmap
status: active
created: 2026-06-21
updated: 2026-09-26
branch: feature/overseas-cdn-routing-roadmap
pr: 62
supersedes: []
superseded_by:
---

# Overseas CDN Routing Roadmap

## Summary

- Overseas playback/download experience is a high-priority follow-up after the completed `v0.6.0`
  credential lifecycle release.
- The downloader now has an opt-in ordered CDN host pool, bounded range probing, and multi-CDN
  range downloads. These are host-selection and download-transfer controls; there is still no
  first-class overseas playback preset or persistent route-health policy.
- Existing `--upos-host`, `--force-replace-host`, and PCDN filtering controls remain available;
  the new candidate pool and transfer controls extend that foundation without providing a
  first-class overseas routing preset or embeddable host-selection policy.
- CCB (`https://github.com/Kanda-Akihito-Kun/ccb`) is a useful research reference. Its README
  describes custom Bilibili playback-source switching for ordinary videos, live rooms, bangumi, and
  watch-later pages. It also documents strong replacement of `baseUrl` and `backupUrl`, PCDN
  avoidance effects, and overseas user reports that Hong Kong nodes can improve ordinary video
  playback.
- CCB's `data/cdn.json` and `data/region.json` expose a region-to-host catalog that currently
  includes regions such as Hong Kong and overseas hosts including Akamai and overseas Bilibili mirror
  candidates.
- Bilibili-thread-ripper (BTR) is a second research reference for speed-aware CDN selection and
  concurrent byte-range fetching from an already resolved media representation. It does not resolve
  BiliRoaming playback addresses.
- BiliRoaming-style PGC playback address resolution remains a separate, opt-in upstream step. The
  existing `RestrictedAreaProxy::BilibiliApi` path targets the server's
  `/pgc/player/web/playurl` route after a qualifying official region error. Compatibility now has
  mock coverage and self-hosted endpoint guidance; there is no implicit public resolver.

## Current Implementation

- `--cdn-host <HOST>` can be repeated to form an ordered candidate pool for each resolved media
  URL; normal URL candidates and existing fallback policy remain available. This is also exposed
  through `MediaHostOptions::with_cdn_hosts`.
- `--cdn-probe` opts into bounded, validated range measurements to rank usable candidates for a
  single resolved representation. Probe failures preserve candidates as fallback routes.
- `--cdn-parallel 2..8` opts into bounded multi-CDN range transfer for fresh, known-size media.
  Responses are checked for exact range metadata, size, and body length. Candidate sources are
  initially grouped by a shared byte sample; every chunk returned by a secondary source is fetched
  again from the baseline source concurrently and compared byte-for-byte. Each lane can issue two
  simultaneous range requests for verification, up to 16 at parallelism 8. This adds duplicate
  transfer bytes and network load. A mismatch discards the temporary file and
  falls back to the regular candidate download. Partial-file resume remains sequential, and a
  speedup is not guaranteed.
- Mock tests cover candidate ordering/fallback, probe ranking and timeout behavior, compatible and
  incompatible range sources including a later-chunk mismatch with staging cleanup, transfer
  fallback, file progress, and CLI sidecars. A mock PGC test
  covers a BiliRoaming-compatible `/pgc/player/web/playurl` response; English and Chinese guides
  document self-hosted endpoint configuration.
- These changes cover resolved-media downloads and optional PGC address lookup. They do not provide
  a browser/player playback router, built-in region/CDN catalog, persistent throughput history,
  or live proof that a host-rewritten signed URL is accepted. Live overseas and restricted-area
  end-to-end validation remains outstanding.

## Design Direction

- Treat CCB and BTR as research references, not runtime dependencies. Do not assume that a CDN host
  accepts a signed path/query merely because another host served it.
- Keep `bbdown-core` deterministic and embeddable:
  - expose ordered, explicit host candidates for CLI and API callers before considering a curated
    named preset;
  - preserve existing manual `upos_host` behavior;
  - keep PGC region/proxy resolution separate from media CDN selection and transfer;
  - avoid claiming that CDN switching bypasses restricted-area licensing.
- Keep each selected representation's primary and backup URLs in one resource group. Never combine
  byte ranges from different qualities or from separately resolved playurl responses without an
  explicit same-content contract.
- Prefer bounded, opt-in probing and clear fallback semantics:
  - measure actual signed media URLs with small range requests or learn from real download chunks;
    cap time, bytes, and concurrent probes;
  - rank recent routes by observed latency, throughput, and failures; expire observations so a
    transient slow or failed node can be retried;
  - retain original Bilibili URLs and backups as fallbacks and expose redacted route diagnostics;
  - reject incompatible status, `Content-Range`, total length, or body length before committing a
    chunk; retry the same range elsewhere or use the existing sequential path.
- Introduce concurrent ranges only for a known-size, range-capable, fresh media file. Assemble into
  temporary storage and publish the completed file after all ranges validate. Keep existing
  contiguous-prefix resume behavior until a durable per-range resume format is designed.

## Current Support and Follow-ups

- The downloader supports the mock-covered BiliRoaming-compatible PGC API-path proxy,
  self-hosted endpoint configuration, explicit ordered CDN candidates, opt-in bounded probing,
  and opt-in concurrent range transfer.
- Validate actual overseas candidate compatibility and throughput with opt-in live fixtures; keep
  restricted-area resolver checks separate from CDN performance validation.
- Decide whether persistent route health and a curated region/host catalog are useful. Keep
  host-rewriting presets opt-in until live compatibility evidence supports a default.

## Open Questions

- Whether to vendor a curated host catalog, let users provide catalogs, or periodically refresh a
  generated catalog in the release process.
- Whether overseas presets should default to Hong Kong-first, Akamai-first, or user-location-first.
- Which signed media URL families safely accept host substitution; require a live compatibility
  check before enabling any built-in preset.
- Whether active probes should remain an explicit CLI/download option or whether real chunk
  measurements should eventually maintain route rankings without extra probe traffic.
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
- BTR `src/cdn-resolver.js` derives signed host candidates from one representation's playurl URLs,
  tracks recent route throughput and failures, and periodically explores stale candidates:
  `https://github.com/MrTangLuyao/Bilibili-thread-ripper/blob/main/src/cdn-resolver.js`.
- BTR `src/range-core.js` and `src/idm-downloader.js` validate exact range responses, assemble
  ordered chunks, and use observed transfer speed for scheduling and conditional rescue requests:
  `https://github.com/MrTangLuyao/Bilibili-thread-ripper/blob/main/src/range-core.js` and
  `https://github.com/MrTangLuyao/Bilibili-thread-ripper/blob/main/src/idm-downloader.js`.
- BiliRoaming-Rust-Server registers `/pgc/player/web/playurl` and handles area-aware upstream
  requests; its implementation is a compatibility reference, not a built-in service dependency:
  `https://github.com/pchpub/BiliRoaming-Rust-Server/blob/main/src/main.rs` and
  `https://github.com/pchpub/BiliRoaming-Rust-Server/blob/main/src/mods/upstream_res.rs`.

## Next Steps

- Decide the remaining roadmap scope and release placement; no version has been assigned.
- Resolve the open host-catalog ownership and preset questions without assuming a bundled,
  user-provided, or generated catalog.

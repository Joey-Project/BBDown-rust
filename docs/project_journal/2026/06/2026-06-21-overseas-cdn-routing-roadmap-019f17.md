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
  range downloads. The CLI bundles opt-in CDN and public resolver catalogs with explicit runtime
  probe commands. There is still no automatic overseas playback router or persistent route-health
  policy.
- Existing `--upos-host`, `--force-replace-host`, and PCDN filtering controls remain available;
  the new candidate pool and transfer controls extend that foundation.
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
  single resolved representation. Unknown-size probes include the one-byte discovery request in
  the 64 KiB per-candidate budget and exclude its RTT from sample throughput. Probe failures
  preserve candidates as fallback routes.
- `--cdn-parallel 2..8` opts into bounded multi-CDN range transfer for fresh, known-size media.
  Responses are checked for exact range metadata, size, and body length. Candidate sources are
  grouped by a shared prefix sample and identical URL scheme/path/query. Each chunk is fetched
  once from its selected CDN; a failed range is retried on another candidate. Parallelism 8 means
  at most 8 simultaneous Range requests. The baseline CDN no longer transfers the whole file for
  verification. Prefix and size checks cannot prove complete content identity across hosts, so a
  differing edge can silently splice another version into the completed file;
  partial-file resume remains sequential, and a speedup is not guaranteed. Symlink targets and
  targets with multiple hard links use the ordinary download path to preserve destination behavior.
  Non-Unix builds conservatively use that path for any existing regular target.
- `bbdown-core::probe_media_cdns` exposes explicit, bounded per-candidate results to embedding
  applications. The CLI bundles a snapshot of CCB's CDN host data and a historical public resolver
  list. Presets and runtime probes require an explicit user selection; no public resolver is a
  default. `resolver probe --server` isolates the selected server from configured CLI/environment
  proxy candidates. Catalog host availability must be checked at runtime. Preset downloads put
  the donor's normal media candidate after one configured CDN to bound failures before that route; it remains
  subject to the existing host-replacement policy. Manual `--cdn-host` pools retain their prior
  ordering. Probe ranking may reorder at most the first 8 candidates.
- Successful staged shards emit `CdnShardCompleted` progress events with the actual source host and
  byte count. No signed media URL path or query is included in the event. Recoverable sharding
  failures fall back to ordinary download without a terminal `FileFailed` event; cancellation and
  fatal target metadata errors still report failure.
- Mock tests cover candidate ordering/fallback, probe ranking and timeout behavior, compatible and
  incompatible range sources, transfer
  fallback, file progress, and CLI sidecars. A mock PGC test
  covers a BiliRoaming-compatible `/pgc/player/web/playurl` response; English and Chinese guides
  document self-hosted endpoint configuration.
- These changes cover resolved-media downloads and optional PGC address lookup. They do not provide
  a browser/player playback router or persistent throughput history. Live compatibility results
  should be recorded separately from mock coverage.

## Live Validation (2026-09-26)

- With public fixture `BV15hdwBKEMG`, `cdn probe --preset overseas` sampled 64 KiB per host twice.
  Two overseas hosts succeeded twice; one succeeded only on retry, and one failed twice. Observed
  sample throughput varied substantially between attempts, so these results are a runtime snapshot.
- An opt-in `--cdn-preset overseas --cdn-parallel 4` video-only transfer completed a 187,672,972
  byte media stream. Its 1 MiB progress deltas do not distinguish the sharded path from ordinary
  sequential writes. This run alone does not prove actual host-level byte distribution or a speedup
  over a single CDN.
- A separate one-shot live `BV1uW4y1s7zN` video-only download with `--cdn-preset overseas
  --cdn-parallel 4 --progress-json` completed 1,644,777 bytes. Two `cdn_shard_completed` events
  reported 1,048,576 bytes from `upos-sz-mirroraliov.bilivideo.com` and 596,201 bytes from
  `upos-hz-mirrorakam.akamaized.net`; their sum matched both `file_completed.total_bytes` and the
  on-disk media size. No `file_failed` event appeared. This verifies two CDN hosts supplied bytes
  in this run; it does not establish a speedup over a single-host control.
- The restricted-area `ep664928` probe against the explicitly selected `atri` public resolver
  returned one entry with `proxy_exercised=true`. This confirms that the PGC proxy path was used for
  that request, not that every listed resolver is available.
- After the resolver-isolation and public-probe budget fixes, an overseas rerun on `BV1uW4y1s7zN`
  found four hosts with at least one successful 65,536-byte Range response; another preset host
  failed. A fresh video-only transfer again emitted two successful shard events totaling 1,644,777
  bytes: 596,201 from `upos-hz-mirrorakam.akamaized.net` and 1,048,576 from
  `upos-sz-mirroraliov.bilivideo.com`. This matched `file_completed` and the on-disk size, with no
  `file_failed` event or observed fallback. No same-run single-host control was measured.
- In the final unauthenticated `ep664928` / HK resolver probes, `atri` returned HTTP 404, and
  `mahiron` also failed; `bstar` returned one entry with `proxy_exercised=true`. The earlier `atri`
  success and this later failure show why catalog entries need runtime checks.
- The older manifest-driven `just live-e2e` suite stopped before any fixture request because its
  ignored local manifest references an absent default credential file. The direct CDN and resolver
  runs above are the live evidence for this PR; the legacy suite has no pass result for this run.

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
  opt-in concurrent range transfer, and opt-in catalogs for CDN hosts and public resolvers.
- Validate actual overseas candidate compatibility and throughput with opt-in live fixtures; keep
  restricted-area resolver checks separate from CDN performance validation.
- Decide whether persistent route health is useful. Keep host-rewriting presets opt-in until live
  compatibility evidence supports a default.
- Compare measured wall time against a single-host control for representative media sizes and
  locations; the current progress events prove source distribution, not a speedup.

## Open Questions

- How to maintain, verify, and periodically refresh the bundled host and public resolver snapshots.
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
- Review live CDN and resolver probe results, then decide whether catalog maintenance or persistent
  route-health policy warrants another workstream.
- Add redacted per-host transfer counters if we need to quantify real multi-CDN byte distribution,
  and compare elapsed time against a single-CDN run on the same representation before claiming a
  live speedup.

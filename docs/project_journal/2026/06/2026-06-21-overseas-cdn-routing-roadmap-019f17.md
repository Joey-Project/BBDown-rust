---
id: 20260621-019f17-overseas-cdn-routing-roadmap
title: Overseas CDN Routing Roadmap
status: active
created: 2026-06-21
updated: 2026-09-28
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
  the 64 KiB per-candidate budget. Automatic ranking reuses that discovery byte in its prefix
  comparison; probe failures preserve candidates as fallback routes.
- `--cdn-parallel 2..8` opts into bounded multi-CDN range transfer for fresh, known-size media.
  Responses are checked for exact range metadata, size, and body length. Candidate sources are
  grouped by a shared prefix sample and identical URL scheme/path/query. Each chunk is fetched
  once from its selected CDN; a failed range is retried on another candidate. Parallelism 8 means
  at most 8 simultaneous Range requests. The baseline CDN no longer transfers the whole file for
  verification. Prefix and size checks cannot prove complete content identity across hosts, so a
  differing edge can silently splice another version into the completed file;
  non-empty partial-file resume remains sequential without probe reordering, and a speedup is not
  guaranteed. Symlink, multiply linked, and special-file targets use the ordinary download path
  to preserve destination behavior. On Unix, `--no-resume` can shard over an existing single-link
  regular target; non-Unix builds conservatively use the ordinary path for existing files.
- `bbdown-core::probe_media_cdns` exposes explicit, bounded per-candidate results to embedding
  applications. The CLI bundles a snapshot of CCB's CDN host data and a historical public resolver
  list. Presets and runtime probes require an explicit user selection; no public resolver is a
  default. `resolver probe --server` isolates the selected server from configured CLI/environment
  proxy candidates. Global `--resolver` tries the selected server before configured proxies and
  retains those proxies as fallbacks. Catalog host availability must be checked at runtime. Preset
  downloads put the donor's normal media candidate after one configured CDN to bound failures
  before that route; it remains
  subject to the existing host-replacement policy. Manual `--cdn-host` pools retain their prior
  ordering. Probe ranking may reorder at most the first 8 candidates.
- After complete sharded output is published, `CdnShardCompleted` events report each shard's actual
  source host and byte count; discarded staging bytes are not reported as progress. No signed media
  URL path or query is included in the event. Recoverable sharding
  failures fall back to ordinary download without a terminal `FileFailed` event; cancellation and
  fatal target metadata errors still report failure.
- Mock tests cover candidate ordering/fallback, probe ranking and timeout behavior, partial
  resume ordering, no-resume fresh sharding, special target preservation, compatible and
  incompatible range sources, transfer fallback, published-only shard progress, and CLI sidecars.
  A mock PGC test
  covers a BiliRoaming-compatible `/pgc/player/web/playurl` response; English and Chinese guides
  document self-hosted endpoint configuration.
- An unignored CLI e2e test uses local HTTP mocks for the complete restricted PGC fallback,
  BiliRoaming-compatible API-path response, unknown-size CDN probe, and two-host sharded download.
  It verifies exact downloaded bytes, preserved media query parameters, and per-host published
  shard events. The existing workspace test command includes this e2e in CI without public API
  or CDN dependencies. Both user guides include the three-feature download flow diagram.
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
- After deferring shard progress until publication, an authorized-network rerun of the same small
  video completed with two `cdn_shard_completed` events between `file_started` and
  `file_completed`: 1,048,576 bytes from `upos-sz-mirroraliov.bilivideo.com` and 596,201 from
  `upos-hz-mirrorakam.akamaized.net`. The 1,644,777-byte sum matched the completed event and
  on-disk file size; no `file_failed` or fallback was observed. A sandboxed attempt could not
  resolve the public video domain, while a narrowly authorized HTTPS preflight returned 200.
- After bounding the automatic unknown-size probe to 64 KiB per candidate, a final plan for
  `BV1uW4y1s7zN` reported 15 null size fields. The rebuilt CLI completed a video-only overseas
  transfer with two published shards: 1,048,576 bytes from
  `upos-hz-mirrorakam.akamaized.net` and 596,201 from
  `upos-sz-mirroraliov.bilivideo.com`. Their 1,644,777-byte sum matched the completed event and
  on-disk file size, with no `file_failed` event or observed fallback.
- The older manifest-driven `just live-e2e` suite stopped before any fixture request because its
  ignored local manifest references an absent default credential file. The direct CDN and resolver
  runs above are the live evidence for this PR; the legacy suite has no pass result for this run.

## Controlled Live A/B Benchmark (2026-09-28)

- The public `normal-playlist-video` fixture `BV1QtjA6BEB8` supplied one 106,436,100-byte
  quality-80 AVC video stream. A temporary API harness resolved one `DownloadPlan` and reused its
  exact signed representation for all eight video-only downloads. Each run used a fresh private
  output directory, no resume, and a single download attempt. The order was baseline, 2, 4, 8
  lanes, then 8, 4, 2, baseline to reduce simple time-order bias. A lane is one concurrent Range
  request, not a distinct CDN host.
- The single-lane baseline forced `upos-sz-mirror08h.bilivideo.com` for every donor URL. Parallel
  runs configured that host plus `upos-hz-mirrorakam.akamaized.net` and
  `upos-sz-mirroraliov.bilivideo.com`; the ordinary original donor remained eligible and appeared
  as `upos-sz-mirrorcoso1.bilivideo.com` in some published shards. The separate preflight took
  8.725 seconds, read 262,144 observed bytes, and found four usable candidates. Internal candidate
  checks are included in each download time but cannot be timed separately through the public API.

| Run | Mode | Download (s) | Published source bytes |
| --- | --- | ---: | --- |
| 1 | Baseline | 190.145 | Fixed mirror08h; 106,436,100 output bytes, no shard event |
| 2 | 2 lanes | 295.841 | aliov 105,387,524; akamai 1,048,576 |
| 3 | 4 lanes | 53.550 | aliov 93,853,188; akamai 12,582,912 |
| 4 | 8 lanes | 44.086 | aliov 76,027,396; akamai 15,728,640; original 13,631,488; mirror08h 1,048,576 |
| 5 | 8 lanes | 49.607 | aliov 61,865,984; akamai 22,020,096; original 18,355,716; mirror08h 4,194,304 |
| 6 | 4 lanes | 34.923 | aliov 82,318,852; akamai 15,728,640; original 6,291,456; mirror08h 2,097,152 |
| 7 | 2 lanes | 39.693 | aliov 91,756,036; akamai 13,631,488; mirror08h 1,048,576 |
| 8 | Baseline | 119.075 | Fixed mirror08h; 106,436,100 output bytes, no shard event |

- Every run produced exactly 106,436,100 bytes and the same SHA-256 digest,
  `a5e36c30dac68f70bdcb0c6a63040e11d7e49410f269ab464def09c19d6df404`. All six
  parallel runs emitted published shard events summing to the output size; no whole-file fallback
  was observed. The eight outputs totaled 851,488,800 bytes and were deleted after hashing.
- Mean elapsed times were 154.610 seconds for baseline, 167.767 for 2 lanes, 44.237 for 4 lanes,
  and 46.847 for 8 lanes. The 4- and 8-lane modes were about 3.49 and 3.30 times faster than the
  baseline mean on this host and fixture. The 2-lane runs varied from 295.841 to 39.693 seconds,
  while baseline varied from 190.145 to 119.075 seconds; two repeats are directional evidence,
  not a stable throughput distribution. The baseline is a single long transfer, whereas parallel
  modes change both concurrency and host set, so this run cannot isolate the benefit of CDN
  diversity from concurrent Range requests.
- Published shard events omit failed/retried transfers and probe traffic. The API does not expose
  internal probe duration, accepted/excluded host reasons, per-chunk retry counts, or total wire
  bytes. The exact network traffic and retry overhead therefore remain unknown; output bytes and
  the separate preflight sample are not a wire-byte total. A same-host concurrent Range control,
  more repetitions, and other media sizes/times are needed before choosing a default policy.

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
- Repeat the initial A/B benchmark across media sizes and times. Add a same-host concurrent Range
  control and route diagnostics before attributing observed 4-/8-lane speed gains to CDN diversity.

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
- Use the initial same-representation A/B result to choose the next validation: isolate the effect
  of concurrency from host diversity, then decide whether to tune the scheduler or route policy.

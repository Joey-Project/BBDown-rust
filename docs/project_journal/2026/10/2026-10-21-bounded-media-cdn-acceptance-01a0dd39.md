---
id: 20261021-bounded-media-cdn-acceptance-01a0dd39
title: Bounded Media CDN Transfer Acceptance
status: completed
created: 2026-10-21
updated: 2026-10-21
branch: codex/cdn-media-acceptance
pr:
supersedes: []
superseded_by:
---

# Bounded Media CDN Transfer Acceptance

## Scope and Method

- Compare the curated public `normal-playlist-video` fixture, entry 1, quality 16, AVC codec `avc1.640033`, across sequential baseline, fixed-host four-range, and multi-host four-range downloads. Fixture metadata did not include a representation size; the bounded shared donor size probe read 65,536 media-body bytes once outside the six group totals.
- Run two rotated repetitions in execution order: baseline / fixed-host-range / multi-host-range; then fixed-host-range / multi-host-range / baseline.
- The tested production code on source HEAD `273e9fc1be32f84bd753d5b36287f3dbdb708961` matched released `bbdown-core 0.11.0` source commit `0a3ccd624647017e10ec893ffd9f3d73bf507631` for this behavior.
- The run was anonymous and used no proxy, credential file, cookie, player, or injected media host. It did not change automatic or adaptive CDN policy.
- Guardrails: a 64 MiB representation-size cap, a 512 MiB aggregate application-consumed decoded media-body cancellation threshold, a 1,024 observed-request-ID threshold, 600 seconds of harness runtime plus bounded startup/cleanup grace, a nominal sampled 768 MiB media-disk watchdog, and a 1 MiB bounded log. Crossing chunks and in-flight reads can overshoot the body threshold; these are not wire-traffic limits.

## Results

All six records passed with `valid_three_way_sample=true`. Every output was 16,698,182 bytes with SHA-256 `5f2b3fcb64d2f74ea8160539aeb505fa8b3db40d686b3dd45fdcf6293ee52f55`.

| Repetition | Group | Elapsed | Preflight media-body bytes | Downloaded media-body bytes |
| --- | --- | ---: | ---: | ---: |
| 1 | Sequential baseline | 181.051 s | 0 | 16,698,182 |
| 1 | Fixed-host four-range | 1.327 s | 81,920 | 16,698,182 |
| 1 | Multi-host four-range | 2.910 s | 163,840 | 16,698,182 |
| 2 | Fixed-host four-range | 1.367 s | 81,920 | 16,698,182 |
| 2 | Multi-host four-range | 1.281 s | 163,840 | 16,698,182 |
| 2 | Sequential baseline | 9.811 s | 0 | 16,698,182 |

- Fixed-host shards all came from `upos-hz-mirrorakam.akamaized.net`. Each multi-host repetition successfully used both that Akamai host (12,503,878 media bytes) and `upos-sz-mirrorcosov.bilivideo.com` (4,194,304 media bytes). Both were upstream-provided HTTPS candidates; no host was injected.
- Group preflight-body bytes in execution order were `0, 81,920, 163,840, 81,920, 163,840, 0`. Each group transferred 16,698,182 media-body bytes. Range preflight overhead was about 0.49% of the representation for fixed-host and 0.98% for multi-host.
- Cumulative application-consumed decoded media-body bytes were 100,746,148 of the 536,870,912-byte budget, including the shared 65,536-byte size probe; budget overshoot was zero. Request-ID count was a lower bound of 80, below the 1,024 threshold. Sampled media-disk peak was 100,189,092 of the nominal 805,306,368-byte watchdog. The harness ran 205.61 seconds and exited successfully; the bounded log was 47,054 bytes.
- There were no failed or cancelled requests, the retry policy allowed one attempt, whole-file fallback was false, and cleanup did not time out. Shutdown/reaping completed and the media files were removed. Source HEAD and tracked-tree state remained unchanged.
- The first startup stopped before media transfer because its temporary selector required `codec_family`, which the `DashTrack` did not set. The selector was corrected to use the `avc1`/`avc3` codec strings; the completed six-record run above then passed. The aborted startup is not evidence of missing upstream hosts.

## Interpretation and Limits

- The sample proves successful, byte-identical outputs and genuine two-host shard use for this fixture and run. It does not establish a generalized speedup or that multi-host ranges consistently outperform fixed-host parallel ranges. The baseline varied from 181.051 to 9.811 seconds; multi-host was slower than fixed-host in repetition 1 and slightly faster in repetition 2.
- This was one public fixture, one time window, and two repetitions. API preflight JSON/body bytes and total wire traffic were not instrumented. Application-consumed response-body bytes exclude those unmeasured quantities and are not total network traffic; in-flight reads can overshoot configured bounds in general.
- This is media-transfer evidence only. It does not establish native decoding, sustained playback, full-fill, offline playback, global CDN performance, automatic routing, or adaptive policy.
- The downstream consumer V2/HLS smoke is a separate acceptance. See `2026-10-20-dynamic-playback-acceptance-01a0dd39.md` for the immutable consumer source head, core pin, and bounded smoke result.

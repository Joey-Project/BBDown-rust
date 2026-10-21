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
- The attempt-2 runner receipt records the outer validation envelope as `2026-10-21T13:45:18.470232+00:00` through `2026-10-21T13:49:02.147262+00:00` (UTC). The runner writes the start before its source check and anonymous plan preflight, and the finish after the benchmark child is reaped and its result log is parsed. This envelope includes preflight, Cargo startup, harness execution, reaping, and result capture; temporary output removal and receipt writing follow the recorded finish. The receipt has no separate UTC timestamps for the benchmark harness itself, so its measured 205.61-second runtime below is retained without deriving inner start/end times.
- The tested production code on source HEAD `273e9fc1be32f84bd753d5b36287f3dbdb708961` matched released `bbdown-core 0.11.0` source commit `0a3ccd624647017e10ec893ffd9f3d73bf507631` for this behavior.
- The run was anonymous and used no proxy, credential file, cookie, player, or injected media host. It did not change automatic or adaptive CDN policy.
- Guardrails: a 64 MiB representation-size cap, a 512 MiB aggregate application-consumed decoded media-body cancellation threshold, a 1,024 observed-request-ID threshold, 600 seconds of harness runtime plus bounded startup/cleanup grace, a nominal sampled 768 MiB media-disk watchdog, and a 1 MiB bounded log. Crossing chunks and in-flight reads can overshoot the body threshold; these are not wire-traffic limits.

## Reproduction

The repo-local fixture label `normal-playlist-video` maps to the public URL in the plan command
below. The one-shot
runner first made an anonymous plan request, then invoked the ignored `cdn_benchmark_harness`
test. `ABSENT_CREDENTIAL_FILE` stands for the runner's deliberately absent credential-file path;
no credential file was read. `PRIVATE_TEMP_MEDIA_DIR` stands for the private temporary media
directory created for this run and removed after the harness was reaped.

```sh
cargo run --locked -p bbdown-cli --quiet -- \
  --credential-file "$ABSENT_CREDENTIAL_FILE" \
  --credential-preflight off plan \
  https://www.bilibili.com/video/BV1QtjA6BEB8/ --json
```

The plan process had a 90-second deadline and an 8 MiB captured-stdout cap; stderr was discarded
to avoid retaining URL-bearing diagnostics. Before launch, the runner removed inherited variables
whose names start with `BBDOWN_` or `BILIBILI_LIVE_E2E_`, plus `HTTP_PROXY`, `HTTPS_PROXY`,
`ALL_PROXY`, `NO_PROXY` (and lowercase spellings), `RUST_LOG`, and `RUST_BACKTRACE`. The harness
received these explicit settings:

```sh
BBDOWN_CDN_BENCHMARK_SAMPLE=normal-playlist-video \
BBDOWN_CDN_BENCHMARK_ENTRY=1 \
BBDOWN_CDN_BENCHMARK_REPETITIONS=2 \
BBDOWN_CDN_BENCHMARK_ORDER_OFFSET=0 \
BBDOWN_CDN_BENCHMARK_QUALITY=16 \
BBDOWN_CDN_BENCHMARK_CODEC=avc1.640033 \
BBDOWN_CDN_BENCHMARK_MAX_MEDIA_BYTES=67108864 \
BBDOWN_CDN_BENCHMARK_BODY_BUDGET_BYTES=536870912 \
BBDOWN_CDN_BENCHMARK_REQUEST_BUDGET=1024 \
BBDOWN_CDN_BENCHMARK_RUN_TIMEOUT_SECS=600 \
BBDOWN_CDN_BENCHMARK_HOSTS=upos-sz-mirrorcosov.bilivideo.com,upos-hz-mirrorakam.akamaized.net \
BBDOWN_CDN_BENCHMARK_OUTPUT_DIR="$PRIVATE_TEMP_MEDIA_DIR" \
cargo test -p bbdown-core --test cdn_benchmark --locked \
  cdn_benchmark_harness -- --ignored --nocapture
```

`BBDOWN_CDN_BENCHMARK_FIXED_HOST` was unset, so the harness used the selected donor's Akamai
authority (`upos-hz-mirrorakam.akamaized.net`) for the fixed-host control. The host list contains
the two upstream HTTPS authorities discovered by the anonymous preflight. `BBDOWN_CDN_BENCHMARK_URL`
and `BBDOWN_CDN_BENCHMARK_KEEP_OUTPUT` were also unset. The outer runner enforced a 690-second
watchdog, 1 MiB log cap, and nominal 768 MiB sampled disk watchdog. The per-representation 64 MiB
cap, aggregate consumed-body threshold, request-ID threshold, and harness timeout are shown above.
These byte and request counters do not include API preflight JSON bodies or all wire traffic.

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
- This was one public fixture and two repetitions within the timestamped outer validation envelope above. API preflight JSON/body bytes and total wire traffic were not instrumented. Application-consumed response-body bytes exclude those unmeasured quantities and are not total network traffic; in-flight reads can overshoot configured bounds in general.
- This is media-transfer evidence only. It does not establish native decoding, sustained playback, full-fill, offline playback, global CDN performance, automatic routing, or adaptive policy.
- The consumer V2/HLS smoke is separate compatibility evidence from an isolated temporary consumer adaptation, not completed or accepted downstream integration. The downstream maintainer owns dependency upgrade, adaptation, acceptance, and merge. See `2026-10-20-dynamic-playback-acceptance-01a0dd39.md` for the immutable consumer source head, core pin, and bounded smoke result.

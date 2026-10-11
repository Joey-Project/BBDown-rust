---
id: 20261011-cdn-expanded-live-measurement-01a12c87
title: Expanded CDN Live Measurement and Routing Design
status: completed
created: 2026-10-11
updated: 2026-10-11
branch: wip/cdn-expanded-live-measurement
pr: 98
supersedes: []
superseded_by:
---

# Expanded CDN Live Measurement and Routing Design

## Summary

- Add an evening observation window with two anonymous, public-video benchmark invocations.
  Both invocations produced valid fixed-host sequential, fixed-host four-way Range, and multi-host
  four-way Range controls: six successful downloads in total.
- Add downward-only harness budget overrides and optional unique quality/codec selection, with
  deterministic regression tests. No production routing or default download behavior changes.
- Record a separate opt-in routing proposal in [English](../../../cdn-routing-plan.md) and
  [Simplified Chinese](../../../cdn-routing-plan.zh-CN.md). Implementation remains a later PR.
- Retain the complete sanitized group records in
  [the measurement artifact](../../../cdn-measurements/2026-10-11-evening.json).

## Environment and protocol

- Source baseline: `e971ee5f15826ce455fd4642e1b886060876af49`, after PRs #96 and #97.
  Measurement harness commit: `ad386e1ad7fb4ed99b1bfb189a5e778543e6823f`.
  The harness was built on macOS 26.3, Darwin arm64, Rust/Cargo 1.95.0.
  Task timezone is Europe/London; the physical egress location was not verified.
- Use `Credentials::default()` through the ignored library test, not the credential-loading CLI.
  No stored credentials or public resolver were used. Ambient HTTP/HTTPS/ALL proxy variables were
  removed for the library invocations. The preliminary Python probe also disabled proxies and
  redirects.
- Public fixtures: `BV1QtjA6BEB8` (normal playlist video), and `BV1uW4y1s7zN` (normal multipage
  video), entry 1, one repetition each. Representation selection was left unset, preserving the
  first-video behavior. The supplied candidate set was `upos-sz-mirroraliov.bilivideo.com`,
  `upos-hz-mirrorakam.akamaized.net`, and `upos-sz-mirror08h.bilivideo.com`; the first was the fixed
  host. Candidate configuration is not proof that every host supplied published media.
- Group-order offsets were 0 and 1 respectively. All groups used fresh temporary directories,
  no resume or mux, and no retained media. Downloads used the current fixed 1 MiB Range chunks
  and four configured lanes; a small representation has fewer chunks than lanes.
- Each invocation enforced a 128 MiB representation limit, a 512 MiB observed-body cancellation
  threshold, a 600-second harness deadline, and a 2,048 observed-request-ID threshold. Per-request
  timeout remained 300 seconds and active-download cancellation cleanup grace five seconds.
  A separate supervisor enforced a 615-second wall deadline, an 8 MiB log cap, a shared 16 MiB log
  allowance, and a 1.5 GiB free-disk reserve. These invocations ran serially.
- Two independent budgets nominally allow 1 GiB of consumed media body and 20 minutes of harness
  time, below the authorized task totals of 3 GiB and 60 minutes. Body cancellation observes a
  chunk after reading it; crossing and in-flight chunks can overshoot. The thresholds are not a
  hard wire-traffic cap. The actual counters, not the nominal allowance, determine final usage.

## Anonymous preflight and actual results

- The preliminary official metadata requests for both fixtures returned HTTP 412 at
  `2026-10-11T20:01:44Z`; two calls took 1.501 seconds. That Python path read no API or media body
  after the HTTP errors. It made no playurl or CDN request. Zero body bytes read is not zero wire
  traffic. Its user agent and request construction differ from the project library.
- The subsequently compiled project harness succeeded anonymously for both fixtures. The raw
  preflight failures therefore do not establish that the project path is blocked.
- Large output size: 106,436,100 bytes per group. Small output size: 1,644,777 bytes per group.
  Within each invocation all three output sizes and SHA-256 hashes matched; every record has
  `valid_three_way_sample: true` and no validity reasons. Hash equality establishes equality of
  completed sample outputs, not full-object identity at every CDN for all times.
- Final consumed decoded-media-body counters were 319,554,060 and 5,245,627 bytes respectively:
  **324,799,687 bytes total**. Neither invocation exhausted a budget, recorded byte overshoot,
  or timed out during cleanup. Outer process elapsed times were 33.062 and 5.499 seconds.
  Group elapsed times, probe/transfer phases, outcomes, and published shard sources are preserved
  independently in the artifact; the process duration is not a group duration.
- Media body counters exclude API bodies, HTTP headers, transport framing, and traffic not
  consumed by the application. Request IDs are observed from body/diagnostic events and provide
  only a lower bound; IDs restart by progress-reporter scope. Shared donor-size discovery is
  charged once to the invocation, even when repeated as metadata in all group records.
- Activity spans omit connection and response-header wait; they are not TTFB/full latency and
  cannot be added across concurrent hosts. Probe exclusions, retries, and fallback must be read
  alongside elapsed time and actually published shard bytes.

### Group observations

| Fixture | Group / order | Started at UTC | Download elapsed | Consumed body bytes |
| --- | --- | --- | ---: | ---: |
| Playlist | Sequential / 1 | 20:11:14.496 | 2,702 ms | 106,436,100 |
| Playlist | Fixed-host Range / 2 | 20:11:22.680 | 3,672 ms | 106,518,020 |
| Playlist | Multi-host Range / 3 | 20:11:33.058 | 5,867 ms | 106,534,404 |
| Multipage | Fixed-host Range / 1 | 20:13:00.890 | 204 ms | 1,726,697 |
| Multipage | Multi-host Range / 2 | 20:13:01.198 | 2,862 ms | 1,808,617 |
| Multipage | Sequential / 3 | 20:13:04.142 | 44 ms | 1,644,777 |

- The playlist representation was quality 80 / `avc1.640033`; the multipage representation was
  quality 80 / `avc1.640032`. All dates in the table are 2026-10-11. Download elapsed includes
  that group's probing and transfer but excludes the subsequent output hash calculation.
  Harness elapsed times were 32,977 and 5,464 ms, distinct from the outer process wall times.
- Each invocation also consumed a shared 65,536-byte donor-size probe (999 and 950 ms
  respectively). Add it once to the three group totals, not once per group record.
- Fixed-host Range published every output byte from `upos-sz-mirroraliov.bilivideo.com`.
  Playlist multi-host Range published 98,047,492 bytes from that host and 8,388,608 bytes from
  `upos-sz-mirror08h.bilivideo.com`. Multipage multi-host Range published 1,048,576 and 596,201
  bytes from those two hosts respectively. Four lanes are configured, but the small output has
  only two chunks. There is no evidence of three- or four-host published supply.
- Playlist groups observed 1, 104, and 108 distinct media request IDs. Their terminal outcome
  counts were respectively 1 success; 104 successes; and 105 successes, two invalid responses,
  and one timeout. Multipage groups, in executed order, observed 4, 8, and 1 IDs, with 4
  successes; 6 successes and two invalid responses; and 1 success. Shared size-probe IDs are
  separate. These remain media-event lower bounds, excluding API requests.
- Both multi-host groups recorded one candidate exclusion and one probe failure. No group
  recorded `retry_scheduled` or whole-file fallback. All successful output checks still passed;
  candidate failure is observable even when the group succeeds.
- Sequential was faster than either Range control in these individual observations. Probe
  costs, cache/order effects, and the short observation window limit interpretation; this is
  evidence against promising a universal speedup, not proof that parallelism is always slower.

## Harness and design follow-up

- `BBDOWN_CDN_BENCHMARK_BODY_BUDGET_BYTES`, `RUN_TIMEOUT_SECS`, `REQUEST_BUDGET`, and
  `MAX_MEDIA_BYTES` (each with the same full prefix) accept positive values no larger than their
  historical defaults. Each independent invocation resets its budget; operators must aggregate
  use across invocations. Records carry the configured limits.
- Optional `BBDOWN_CDN_BENCHMARK_QUALITY` and `BBDOWN_CDN_BENCHMARK_CODEC` must uniquely match a
  video representation when either is supplied. Codec strings match exactly. Unset selectors
  preserve first-video selection; zero or ambiguous matches fail setup.
- Later Stage 1 proposes an explicit bounded candidate-pool policy, retaining current defaults
  and the existing probe/parallel-triggered throughput ranking. It reuses the internal automatic
  sample pass (up to 64 KiB per candidate, two-second request timeout) once, with at most four
  candidate URLs/authorities including the donor and a proposed five-second overall ceiling.
  Public donor-size setup (64 KiB/five seconds) and internal shard compatibility probing
  (16 KiB/two seconds) remain separate operations.
- Stage 2 proposes bounded process-memory host health/TTL without URLs, identifiers, sampled
  bytes, or credentials. Persistence and adaptive chunk/concurrency tuning remain later opt-in
  work. API/resolver/PGC policy remains separate from CDN ranking.
- This adds a short evening window to the historical two-size, short-morning-window baseline.
  It adds neither new representation sizes nor independent locations. The limited comparisons
  do not demonstrate stable speedup or justify a global default host, TTL, or adaptive policy.

## Validation

- `cargo test --offline --locked -p bbdown-core --test cdn_benchmark`: 20 passed, one live test
  ignored after review corrections. The initial three new tests cover default/lowered limits,
  invalid/zero/over-cap limits, and unique quality/codec selection. Two further regressions cover
  low setup budgets, dropping a pending cancelled probe, and rejecting same-poll over-budget
  success. Two output-verification tests cover the known SHA-256 result and dropping a pending
  asynchronous read when the run deadline expires.
- `cargo clippy --offline --locked -p bbdown-core --test cdn_benchmark -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- The just-built live test binary was invoked twice with `--exact cdn_benchmark_harness --ignored
  --nocapture` and the limits above; both invocations passed at the recorded measurement commit,
  before the setup-cancellation and output-hashing deadline corrections. No additional live
  experiments were run. Neither
  measurement exhausted a budget, so it did not exercise the corrected cancellation path; that
  path is validated by the new local mock/controlled-future regressions; hashing uses local
  file/controlled-reader tests. Temporary media was not retained.
- Discussion and coordination used GPT-6.1 Sol Extra High; delegated harness, documentation, and
  measurement processing used GPT-6 Luna High. No local formal reviewer was started.

## Review corrections

- [Donor probe budgets](https://github.com/Joey-Project/BBDown-rust/pull/98#discussion_r4243095379):
  observe setup cancellation as well as deadlines, drop a pending probe future on budget
  exhaustion, and recheck the budget if the probe returns in the same poll as its crossing
  callback. Already-consumed crossing chunks/current requests remain observable overshoot;
  this does not add a request-start event or promise a wire cap.
- [Existing probe semantics](https://github.com/Joey-Project/BBDown-rust/pull/98#discussion_r4243095382):
  correct both design languages to preserve the existing probe/parallel-triggered throughput
  reordering and define how later opt-in candidate policy composes with it.
- [Output hashing deadline](https://github.com/Joey-Project/BBDown-rust/pull/98#discussion_r4243171390):
  apply the run deadline to asynchronous file open/read, hash in 64 KiB chunks with execution
  yields, and refresh the budget snapshot before declaring a group successful. Retain
  failed-group diagnostics and keep transfer elapsed time separate from output verification.
  Dropping a Tokio read does not guarantee OS-level cancellation: one pending chunk read may
  complete in the background; no unbounded whole-file blocking task is spawned.
- [Effective donor eligibility](https://github.com/Joey-Project/BBDown-rust/pull/98#discussion_r4243171394):
  reserve the donor only after the existing media-host rewrite and eligibility policy. Future
  auto mode preserves CLI defaults and `allow_pcdn`; it must not restore excluded raw hosts.

## Next steps

- Review the separate routing proposal and implement explicit Stage 1 selection in a later PR,
  including the documented mock acceptance conditions.
- Before selecting defaults or tuning host health/adaptation, repeat bounded opt-in controls
  across more sizes, independent windows, and verified egress locations. Preserve full output
  hashes, actual supplied hosts, body accounting, cancellation, and fallback evidence.

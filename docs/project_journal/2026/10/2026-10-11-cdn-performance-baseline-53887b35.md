---
id: 20261011-cdn-performance-baseline-53887b35
title: CDN Performance Baseline
status: completed
created: 2026-10-11
updated: 2026-10-11
branch: wip/cdn-performance-baseline
pr:
supersedes: []
superseded_by:
---

# CDN Performance Baseline

## Summary

- Document the opt-in three-way CDN comparison and deterministic local transfer scenarios.
- Keep this slice to measurement and test harness work; it changes no production API and does not
  establish a global automatic route.
- User documentation: [`cdn-benchmark.md`](../../../cdn-benchmark.md) and
  [`cdn-benchmark.zh-CN.md`](../../../cdn-benchmark.zh-CN.md).

## Measurement contract

- Compare one resolved representation using fixed-host sequential, fixed-host four-way Range, and
  multi-host four-way Range groups. Each run uses a fresh output directory, no resume or mux, one
  ordinary media download attempt, and rotated group order. Range chunk retries and whole-file
  fallback remain observable within that attempt.
- `transfer_bytes_by_host_phase` and `transfer_total_consumed_body_bytes` describe only the current
  group. Count application-consumed decoded response-body bytes from
  `TransferBytesReceived.bytes_delta`; `RequestFinished.bytes_received` is a final watermark, not an
  extra increment. `CdnShardCompleted.bytes` separately counts published shards.
- Shared donor size discovery is repeated as metadata in each record's `size_probe_transfer_*`
  fields, with `size_probe_included_in_group_totals: false`; do not sum those fields across group
  records. The shared probe is charged once to the 3 GiB run-wide body budget.
- Body/diagnostic events expose request IDs but there is no request-start event; the observed ID
  count is a lower bound. Completed, failed, and cancelled request outcomes are reported
  separately. API/auth traffic, headers, and transport framing are excluded; these are not wire
  byte measurements.
- Activity timestamps cover observed body and phase-tagged diagnostic events only. They omit
  connection and response-header wait, are not full request latency, and concurrent spans are not
  additive. Equal output size and SHA-256 prove equality of the completed sample outputs, while
  matching size and sampled prefixes do not establish whole-representation identity across hosts.

## Scope and future work

- Live runs remain explicitly opt-in. Limits are 256 MiB per representation, three repetitions,
  four candidate hosts, a 64 KiB donor size probe with a five-second deadline when needed, and
  300-second per-download-request timeouts. A 45-minute deadline covers resolution and
  repetitions; the cumulative body and observed-request-ID budgets are 3 GiB and 16,384. Cleanup
  has a five-second grace period. Crossing and in-flight chunks can overshoot the body budget, and
  cancellation/cleanup can extend elapsed time; there is no total wire-byte cap.
- Existing comparisons cover two representation sizes in two short morning windows and 18
  successful downloads. They do not establish stable speedup or justify global default tuning.
- Future automatic CDN selection should first gather broader distributions, then consider minimal
  host-health state without URLs or credentials and bounded adaptive chunk/concurrency settings.
  Preserve exact Range validation, cancellation, fallback, and destination safety. Public playurl
  resolvers and restricted-area PGC resolution are separate explicit choices.

## Validation

- Deterministic local mock scenarios cover small/large bodies, fast and delayed Range responses,
  candidate exclusion, partial-body retry on another host, whole-file fallback, and budget
  cancellation without external services.
- Local validation used Rust/Cargo 1.95.0 on macOS, with a task-specific external build cache,
  incremental compilation disabled, and dev/test debug symbols disabled after the local disk filled.
- `cargo fmt --all -- --check` and `git diff --check` passed.
- `cargo check --workspace --offline --locked` passed.
- `cargo clippy --workspace --all-targets --offline --locked -- -D warnings` passed.
- `cargo test --workspace --offline --locked` passed: 818 tests passed and three live tests remained
  ignored. The CDN integration target contributed ten passing tests and one ignored live benchmark.
- `cargo publish --dry-run -p bbdown-core --locked --allow-dirty --registry crates-io` passed.
  The offline attempt could not read registry HTTP metadata; the successful dry run did not upload.
- The mock server explicitly restores blocking mode after accept. Its byte comparison counts actual
  body writes rather than assuming that the whole requested response was sent.
- No new live CDN performance run or Windows runtime test was performed. Local mock correctness
  and concurrency coverage do not establish a live speedup.

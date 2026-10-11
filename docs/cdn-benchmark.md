# CDN benchmark

The opt-in live benchmark compares three downloads of one resolved video
representation: a sequential fixed-host baseline, four-way Range requests to
that host, and four-way Range requests across a bounded host set. Each group
uses a fresh output directory, disables resume and mux, and makes one ordinary
download attempt; Range chunk retries and whole-file fallback remain
observable within that attempt. Group order rotates between repetitions. The
ignored integration test
contacts public video/CDN services only when explicitly run; ordinary CI does
not use external services for these scenarios.

See the `cdn_benchmark` section in the [user guide](user-guide.md#re-running-the-opt-in-live-cdn-benchmark)
for the command and environment variables. Limits are 256 MiB per representation,
three repetitions, four candidate hosts, 64 KiB for a needed donor size probe,
five seconds for that probe, and 300 seconds per download request. A 45-minute
deadline covers resolution and every repetition. The cumulative consumed-body
budget is 3 GiB and the observed-request-ID budget is 16,384. Cleanup has a
five-second grace period. Exceeding a budget cancels the run; an already-read
crossing chunk and in-flight chunks may overshoot the byte limit, and cancellation
and cleanup may extend elapsed time. These limits are not a wire-traffic cap.

## Reading the measurements

The groups compare the same planned representation and each output is checked
by size and SHA-256. Equal output hashes establish that the completed sample
files match; they do not prove that different CDN hosts served one unchanged
representation. A matching size and sampled prefix are compatibility checks,
not whole-file identity evidence.

`transfer_bytes_by_host_phase` is a per-group array of `{host, phase, bytes}`.
It sums application-consumed, decoded media response-body chunks by safe host
authority (or `null`) and typed phase: `automatic_probe`, `shard_probe`,
`range_chunk`, or `whole_file`. `transfer_total_consumed_body_bytes` is the
per-group total; `preflight_consumed_body_bytes` covers the two probe phases and
`media_body_consumed_bytes` covers Range chunks and whole-file responses. API
and authentication response bodies are excluded, as are HTTP headers and
transport framing. None of these fields measures wire bytes.

If the selected plan has no size, the donor size probe is shared setup for the
three groups. Its `size_probe_transfer_*` fields are repeated as metadata in
each group record, with `size_probe_included_in_group_totals: false`; do not
sum those repeated fields across records. The shared probe is charged once to
the run-wide 3 GiB body budget. It has its own request-ID scope and uses the
`standalone_probe` phase.

Sum `TransferBytesReceived.bytes_delta` by host and phase. `RequestFinished.bytes_received`
is the final watermark for that request, not another increment to add. By
contrast, `CdnShardCompleted.bytes` counts bytes in successfully published
shards by source host. Failed, retried, probed, or cancelled response bodies
may be consumed without being published. Typed `transfer_diagnostics` report
candidate selection/exclusion, retries, whole-file fallback, and request
outcomes. `RequestFinished` outcome counts are reported separately from the
observed request IDs.

The unique IDs in `observed_request_ids` come from body or diagnostic events.
There is no request-start event, so `observed_request_count_lower_bound` cannot
count requests that emitted neither kind of event; it is a lower bound, not a
complete request count. Request IDs restart for each group. The shared donor
probe uses a separate ID scope and is reported in the separate `size_probe_*`
fields.

`transfer_activity_by_host_phase` and `size_probe_transfer_activity_by_host_phase`
report first and last observed event times plus their span for each host/phase.
`observed_at_elapsed_ms` on a diagnostic uses the same collector-relative
clock. These timestamps cover body and phase-tagged diagnostic events only;
they omit connection and response-header wait, so they are neither full request
latency nor durations that can be added across concurrent hosts.

Compare elapsed time and byte distributions across samples, hosts, sizes, and
run windows. Existing observations cover two representation sizes, two short
morning windows, and 18 successful downloads. They do not establish stable
speedup, all-day or multi-location performance, or a global default route.
Future automatic CDN selection should start with broader distributions, then
consider minimal host-health state without URLs or credentials and bounded
adaptive chunk size/concurrency. Exact Range validation, cancellation,
fallback, and destination safety must remain intact. Public playurl resolvers
are a separate explicit choice from media CDN host selection; restricted-area
PGC resolution is also separate and opt-in.

## Validation record

The deterministic local scenarios in
`crates/bbdown/tests/support/cdn_benchmark_scenarios.rs` cover small and large
representations, fast and delayed Range responses, candidate exclusion,
partial-body retry on another host, whole-file fallback, and budget
cancellation without public network access. The [workstream journal](project_journal/2026/10/2026-10-11-cdn-performance-baseline-53887b35.md)
records validation status and gate results. These mock results do not establish
live performance.

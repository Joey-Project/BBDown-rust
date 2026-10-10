---
id: 20261010-cdn-transfer-diagnostics
title: CDN Transfer Diagnostics
status: completed
created: 2026-10-10
updated: 2026-10-10
branch: wip/cdn-transfer-diagnostics
pr: https://github.com/Joey-Project/BBDown-rust/pull/92
supersedes: []
superseded_by:
---

# CDN Transfer Diagnostics

## Summary

- Add progress-sink diagnostics for CDN candidate selection/exclusion, retries, whole-file fallback,
  and response-body bytes consumed by the application. This is a post-`v0.8.0` source-tree addition;
  it is not part of the published `bbdown-core 0.8.0` package.
- Keep this diagnostics slice separate from adaptive chunk sizing, automatic routing, persistent
  route health, suffix resume, hedging, and performance claims.

## Public API

- `DownloadProgressEvent::TransferBytesReceived` reports a request ID, optional entry/file context,
  phase, host, `bytes_delta`, and request-local cumulative `bytes_received`.
- `DownloadProgressEvent::TransferDiagnostic` carries optional request ID, phase, file context, and
  host plus a typed `DownloadTransferDiagnostic`. Operation-level outcomes may have no request ID
  or phase. The progress types are non-exhaustive and re-exported from the crate root.
- `probe_media_cdns_with_progress(client, stream, media_hosts, progress)` adds a sink to standalone
  CDN probing. The existing `probe_media_cdns` signature remains available and runs with the no-op
  sink. Standalone events have no entry/file context, and request IDs restart at 1 per call.
- `DownloadProgressSink::wants_transfer_diagnostics` defaults to `true`; `NoopDownloadProgress`
  disables these events. The CLI emits them only when `--progress-json` is enabled. Existing
  `DownloadReport` and `CdnProbeResult` fields are unchanged.

## Byte And Diagnostic Semantics

- A byte delta is emitted synchronously for each response-body chunk yielded to the application by
  `reqwest::Response::bytes_stream()`, after status/header checks and before body-length validation
  or writing. A body rejected before its stream is consumed counts as zero; `Content-Length` is not
  used to estimate bytes. A whole-file HTTP error response such as 404 or 500 rejected before body
  consumption uses `InvalidResponse` and contributes zero consumed body bytes.
- Counts follow the client's transparent content-decoding configuration. They represent
  application-visible body chunks, not compressed entity octets or all wire traffic. See the
  [reqwest 0.12.28 gzip builder documentation](https://docs.rs/reqwest/0.12.28/reqwest/struct.ClientBuilder.html#method.gzip).
- Sum `bytes_delta` for consumed-body totals. `bytes_received` is a cumulative watermark for one
  request and must not be summed again. It is distinct from file bytes written or published.
- Byte request IDs start at 1 within one file-transfer operation and span its automatic probes,
  shard preflight/ranges, whole-file attempts, retries, and fallbacks. IDs are not unique across
  files or standalone probe calls. Group by the operation context and request ID. Standalone calls
  each start a separate sequence.
- Phases are `automatic_probe`, `shard_probe`, `range_chunk`, `whole_file`, and `standalone_probe`.
  Host fields contain only a parsed host and optional port. Request URL details, including its
  path, query, and credentials, plus raw upstream error text are excluded; the events may still
  include an optional output-file `path` field.
- Typed diagnostics cover candidate selected/excluded, retry scheduled, whole-file fallback, and
  request completion. Candidate selection means only that sample/size/path-query preflight placed a
  candidate in the winning compatibility group; it does not prove the complete representation is
  identical. Shard candidate selections/exclusions are emitted after all probes complete with no
  request ID and phase `ShardProbe`; they do not reuse probe IDs after `RequestFinished`. This phase
  is retained when no compatible group is selected and transfer falls back. Host labels contain
  only a parsed host and optional port, never URL paths, queries, or credentials. Probe timeouts use
  `TimedOut`, resume body lengths are classified against the remaining requested length, span/probe
  failures use `ProbeFailed`, and `SizeMismatch` is reserved for an explicit total-size mismatch.
  Scheme-only differences use `SchemeMismatch` (`scheme_mismatch`), while `PathQueryMismatch`
  (`path_query_mismatch`) means an actual path or query difference; this records the existing
  same-scheme compatibility policy without changing it.
- A successful whole-file `RequestFinished` precedes `FileCompleted` and is emitted only after body
  consumption and flush, length validation, and any required file replacement succeed. A resume
  already complete after HTTP 416 uses the same completion path. File operations that end in failure
  or cancellation do not emit `FileCompleted`; cancellation or dropping may still leave no terminal
  request diagnostic.
- Coverage includes instrumented media probes and range transfers, whole-file retries/fallbacks,
  and file responses such as media and sidecars. Metadata, playurl, and authentication API requests
  are outside this accounting.
- A cancelled or dropped request may lack a terminal `RequestFinished` event. Previously emitted
  byte deltas remain valid.

## Validation And Limits

- The workspace suite covers download and range-transfer diagnostics; the HTTP 500 classification
  regression extends the existing fixture. Parallel mock fixtures retain their server lease through
  the requests they serve, and CLI progress keeps the established one-based `entry_index` convention.
- `env RUSTUP_TOOLCHAIN=1.99.0 just ci` exited 0 in 60.44 seconds (79,839 bytes). Formatting, strict
  workspace Clippy, the Rust 1.95.0 MSRV check, and workspace tests passed (807 passed, 0 failed,
  3 ignored). A separate CLI repeat passed 154 tests (0 failed, 0 ignored). The publish dry run
  packaged 29 files (1.8 MiB, 272.8 KiB compressed) and performed no upload.
- No live-media comparison or measured performance improvement is claimed in this record.
- A later adaptive-transfer workstream can use these diagnostics while keeping consumed response-
  body bytes distinct from written bytes and total wire traffic.

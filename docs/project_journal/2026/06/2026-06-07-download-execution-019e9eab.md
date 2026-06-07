---
id: 20260607-019e9eab-download-execution
title: Download Execution
status: completed
created: 2026-06-07
updated: 2026-06-07
branch: wip/download-execution
pr: https://github.com/Joey-Project/BBDown-rust/pull/3
supersedes: []
superseded_by:
---

# Download Execution

## Summary

- Third PR slice for the BBDown Rust rewrite.
- Scope is file download execution, bounded retry/resume behavior, optional ffmpeg mux integration,
  and mock e2e download coverage.
- QR login, live-test opt-in samples, and restricted-area proxy candidate ordering remain follow-up
  slices.

## Current State

- `BiliClient::download_plan` executes a typed `DownloadPlan` and returns a typed
  `DownloadReport`.
- `BiliClient::download` and `BiliClient::download_input` are convenience wrappers that plan first,
  then execute.
- `DownloadOptions` controls output directory, retry policy, HTTP range resume, media read idle
  timeout, subtitle sidecars, danmaku sidecars, and muxing.
- Crate defaults keep muxing disabled so embedding projects do not spawn external processes unless
  requested.
- CLI `bbdown download` enables ffmpeg muxing by default and supports `--no-mux`, `--ffmpeg`,
  `--no-resume`, `--retry-attempts`, `--retry-backoff-ms`, `--download-idle-timeout-seconds`,
  `--no-subtitles`, `--no-danmaku`, and JSON reports.
- The executor downloads the first DASH video/audio pair for each entry, or FLV `durl` segments
  when DASH media is absent.
- Media and sidecar download requests use media headers without account cookies.
- DASH and FLV backup URLs are used as fallback candidates after the primary URL fails.
- Resume appends only when `Content-Range` starts at the local file length and completes at the
  advertised total length. Matching 416 responses are reported as already complete instead of failed.
- Media downloads validate stream or FLV segment sizes when the plan provides them and roll back
  failed write attempts to the pre-attempt file length.
- Media body reads use a separate idle timeout instead of the metadata request timeout.
- Subtitle and danmaku downloads are sidecars; muxing only combines media tracks.
- Resume coverage verifies HTTP `Range` requests and appending to a partial file.
- Safety coverage verifies media downloads do not send cookies, backup URLs are used, matching 416
  responses are treated as complete, mismatched `Content-Range` responses are rejected, range body
  lengths are checked, non-partial `Content-Range` responses are rejected, and expected media sizes
  are enforced.
- Retry coverage verifies a failed first request can be retried and then written successfully.
- Mux coverage verifies fake ffmpeg success reports and failed ffmpeg status propagation.
- FLV mux coverage verifies concat list paths are relative to the entry directory.
- CLI e2e coverage verifies a mock `download --no-mux --no-danmaku --json` run writes media and
  subtitle files to disk.
- CLI e2e coverage verifies the default mux path with a fake ffmpeg binary while preserving valid
  JSON stdout.

## Next Steps

- Add QR login state machine, local credential update flow, and live-test opt-in harness.
- Keep restricted-area proxy resolver ordering and diagnostics as a separate slice.

## Evidence

- Local type gate: `cargo check --workspace`.
- Local lint gate: `cargo clippy --workspace --all-targets -- -D warnings`.
- Local tests: `cargo test --workspace` with 42 library tests and 5 CLI e2e tests.

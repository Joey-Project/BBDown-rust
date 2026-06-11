---
id: 20260610-019eb1-single-download-modes
title: Single Download Modes
status: completed
created: 2026-06-10
updated: 2026-06-10
branch: wip/single-download-modes
pr: https://github.com/Joey-Project/BBDown-rust/pull/22
supersedes: []
superseded_by:
---

# Single Download Modes

## Summary

- Adds crate and CLI support for single-output downloads after the cover sidecar slice.
- Keeps default downloads unchanged while allowing callers to request only video, audio, subtitle,
  danmaku, or cover outputs.
- Leaves ASS danmaku generation and UPOS/PCDN policy controls for separate follow-up PRs.

## Current State

- `DownloadMode` is exported by `bbdown-core` and can be configured through
  `DownloadOptions::with_download_mode(...)`.
- `DownloadMode::All` preserves the existing DASH pair or FLV fallback behavior, default sidecars,
  and optional muxing.
- `VideoOnly` and `AudioOnly` download only the selected DASH stream, skip sidecars, and never mux.
- `SubtitleOnly`, `DanmakuOnly`, and `CoverOnly` skip media stream requirements, write only the
  requested sidecar family, and reject stream quality selection.
- Download archive records keep the legacy content key for full downloads and add a mode-qualified
  content key for single-output downloads, so sidecar-only runs do not masquerade as full downloads.
- CLI `download` exposes `--only video|audio|subtitle|danmaku|cover`, validates conflicting flags,
  and reports the single downloaded file kind in JSON output.
- English and Simplified Chinese README, crate README, user guide, embedding guide, and architecture
  docs describe the new mode.

## Evidence

- Targeted core tests cover media-only behavior, sidecar-only behavior without media streams, and
  invalid stream selection for sidecar-only modes.
- Targeted CLI e2e tests cover all five `--only` values and conflicting disable flags.
- CLI e2e covers that a `--only cover` archive record does not make a later full download require a
  duplicate decision when the full download uses a different output directory.
- CLI e2e covers that `--only cover` succeeds from metadata and cover endpoints without requiring
  playurl resolution, both with and without `--archive-file`.
- Internal review found the CLI archive preflight still used full planning; the branch now exposes
  mode-aware planning APIs and uses them in the archive path.
- Internal review found public archive lookups still used full-mode matching only; the branch now
  keeps broad `records_for_plan` lookup and adds `records_for_plan_with_mode` for mode-specific
  archive queries.
- `cargo test -p bbdown-core only --locked` passes: 5 filtered tests.
- `cargo test -p bbdown-core archive_records_can_be_queried_by_download_mode --locked` passes: 1
  filtered test.
- `cargo test -p bbdown-cli --test cli_e2e --locked download_only` passes: 5 filtered CLI e2e
  tests.
- `cargo test -p bbdown-cli --test cli_e2e --locked` passes: 26 CLI e2e tests.
- `just ci` passes, including formatter, clippy, MSRV check, workspace tests, CLI e2e, and crate
  publish dry-run.
- `just live-e2e` passes: 1 ignored live manifest test executed.
- Project journal validation passes.
- PR review evidence will be recorded in the PR once this branch is opened.

## Next Steps

- Open the PR, then run the normal CI and triple-review merge gate before moving to ASS danmaku and
  UPOS/PCDN slices.

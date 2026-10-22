---
id: 20261022-pgc-metadata-live-acceptance-01a0dd39
title: Anonymous PGC Metadata Live Acceptance
status: completed
created: 2026-10-22
updated: 2026-10-22
branch: codex/pgc-metadata-live-acceptance
pr:
supersedes: []
superseded_by:
---

# Anonymous PGC Metadata Live Acceptance

## Scope

- Add an opt-in live manifest for anonymous PGC `info` requests through a capability-declared
  metadata service. Cases cover `ep664928`, `ss42513`, and `md28338980`; they do not request
  playurl, playback, or media.
- Keep the tracked example separate from the ignored private `live-e2e.samples.json`. The example
  contains no credentials or proxy configuration and uses an `.invalid` service placeholder.
- Preserve the service contract: anonymous requests, no redirects or endpoint fallback, and an
  8 MiB default decoded-body limit per response. A successful observation does not guarantee later
  availability, region coverage, or media access.

## Upstream Sample Context

- The anonymous official season response for episode `664928` resolved to season `42513`, media
  `28338980`, 58 episodes, one main group (`84760`, title `正片`), and latest episode `664985`.
- The anonymous metadata service at `atri.ink` returned compatible season and media-review metadata
  during the acceptance below. This is a point-in-time response-shape observation, not a general
  availability or regional-access guarantee.

## Validation

- `just ci` passed on 2026-10-22 from 12:50:40.453 to 12:52:48.311 UTC (128 seconds): 994 unique
  workspace tests passed and 4 were ignored. The total comprises 67 + 199 + 15 + 684 + 21 + 2 + 0 +
  6 tests across the configured suites; the standalone CLI suite repeats tests included in that
  workspace total. Final formatter, Clippy, Rust 1.95 MSRV, and publish dry-run gates passed with
  Rust/Cargo 1.99.1. The publish operation was dry-run only; nothing was published. Two PGC metadata
  routing/redaction mock tests passed as part of normal CI.
- The exact ignored live test `live_manifest_cases` passed against both the official endpoint and
  `atri.ink`, once per destination (1 passed, 16 filtered each). Runs completed at 12:54:32.972009 UTC
  (official; started 12:54:30.444846 UTC) and 12:54:32.647303 UTC (`atri.ink`; started 12:54:29.273280
  UTC). All three info cases passed: `ep664928` selected `[664928]`, while `ss42513` and `md28338980`
  selected `[664985]`. Both returned season `42513` / media `28338980`, 58 ordered episodes
  (`664928`–`664985`), and the single expected main group `正片` (`84760`). No other sections were
  present in these live responses; the mock fixture separately covers extra-section behavior.
- Both private manifests contained no credentials or proxy settings. Runs used 10-second request
  timeouts, a 90-second outer deadline, a 2 MiB retained-log cap, stripped credential and proxy environment variables, and
  `NO_PROXY=*` / `no_proxy=*`. They did not read credentials or request playurl or media. The harness
  makes the selected metadata-service route anonymous, disables redirects and fallback, and applies
  the default 8 MiB cap per decoded response. No exact network request count is claimed.
- Earlier local gate attempts stopped at the sandboxed Cargo cache lock and then at three introduced
  Clippy warnings. The warnings were corrected before the successful complete `just ci` run above.
- The first direct test-binary launches failed before HTTP because the integration test expected
  Cargo's runtime `CARGO_BIN_EXE_bbdown` variable. The corrected direct invocation supplied that
  variable and both live runs passed; this required no production-code change.
- Local evidence artifacts are under `/private/tmp/bbdown-pgc-metadata-live-20261022-01a0dd39/`
  (machine-local): `live-*-attempt2.log` / `.json` and `ci-attempt2.log` / `.json`.

## Next Steps

- Re-run the bounded anonymous cases when service compatibility needs rechecking; each result remains
  time- and destination-specific and does not establish later availability, regional access, or media
  playback/download behavior.

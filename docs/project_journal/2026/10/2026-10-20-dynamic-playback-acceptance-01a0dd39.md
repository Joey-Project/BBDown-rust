---
id: 20261020-dynamic-playback-acceptance-01a0dd39
title: Dynamic Playback Candidate Acceptance
status: active
created: 2026-10-20
updated: 2026-10-20
branch: wip/dynamic-playback-acceptance
pr:
supersedes: []
superseded_by:
---

# Dynamic Playback Candidate Acceptance

## Scope

- Recheck the consumer fixture `https://space.bilibili.com/441167301/dynamic` against the
  `0.11.0` source line, starting at `98eadf00bf0cd8a7774c1043a07d22061983bfe4`.
- The earlier HTTP 412 report concerned `0.9.0`. PR #100 subsequently validated device-cookie
  initialization and bounded metadata continuation; neither establishes current playback acceptance.
  See the [existing follow-up](2026-10-11-space-dynamic-request-and-bounded-metadata-followup.md).
- This slice adds test coverage and a repeatable live entry point. It does not change production
  request behavior, credential lifecycle, resolver policy, or release versions.

## Deterministic Coverage

- `dynamic_device_cookie::legacy_space_dynamic_playback_plan_uses_only_first_eligible_page`
  calls the public `plan_playback_input(SpaceDynamic(441167301), Page(1))` API.
- The mock feed starts with a non-video record and provides a two-page archive. Assertions cover
  anonymous device initialization, signed navigation/feed requests, preservation of configured
  credentials, video detail lookup, and one NormalWeb playurl request for the first selected CID.
- A loopback media endpoint and the player subtitle endpoint each receive zero requests. The
  returned playback plan contains one selected entry and non-empty variants.
- Both tests in `dynamic_device_cookie` passed. The separate ignored live harness compiled.

## Opt-in Actual-Core Entry Point

`crates/bbdown/tests/dynamic_playback_live.rs` provides the default-ignored
`legacy_space_dynamic_playback_plan_live_acceptance` test:

```sh
BBDOWN_DYNAMIC_LIVE_CREDENTIAL_FILE=/path/to/credentials.json \
  cargo test -p bbdown-core --test dynamic_playback_live --locked \
  legacy_space_dynamic_playback_plan_live_acceptance -- --ignored --exact --nocapture
```

- The file must be explicitly selected. `BBDOWN_DYNAMIC_LIVE_PROFILE` optionally selects a stored
  profile; otherwise the selected file's default profile is used. There is no path discovery,
  login, renewal, or credential-store write.
- Only the selected Web cookie is copied into a fresh client configuration. The test uses default
  official API endpoints and disables restricted-area routes; access keys and TV keys are not
  configured on the client.
- One original legacy planning call uses a 10-second per-request timeout and a 45-second planning
  deadline. `Page(1)` limits selected output, not upstream requests: the legacy API exposes no hard
  aggregate request limit or configurable redirect policy. Production HTTP defaults remain active.
- Acceptance requires exactly one NormalWeb entry with positive aid/cid, a video variant with an
  HTTP(S) request spec, positive duration, and a known codec family. This checks candidate metadata;
  it does not fetch media bytes or prove playback.
- Output is limited to static failure stages, numeric HTTP/API error codes, and safe candidate
  counts/identifiers. Credentials, signed URLs, headers, raw responses, and complete plans are not
  printed. No separate account login-health assertion is made by this planning test.

## Current Evidence And Remaining Work

- Actual-core playback-candidate acceptance passed once on source revision
  `d3276044dd1f01d4a3d10b562d0cfa1e16a4364e` (`bbdown-core` `0.11.0`; the production source matched
  the scoped base `98eadf00bf0cd8a7774c1043a07d22061983bfe4`). The single legacy planning call used
  the existing TVD credential store's default profile and copied only its Web cookie into the test
  client. No credential path or secret is recorded here.
- The result was observed at `2026-10-20 20:04:33 UTC`: one entry, 36 variants, aid `117474083274912`,
  cid `42906226371`, duration 433 seconds, quality IDs `80,64,32,16`, and video codec family H264.
  The call returned a candidate and did not report HTTP 412. The harness had no API request observer,
  so this does not establish per-request counts or statuses, prove a root cause, or demonstrate a
  general fix. Login health was not checked separately.
- The run did not perform QR login, credential refresh or store writes, use access keys or a proxy,
  fetch media, or play it. BBDown's other local credential store received only static structure
  inspection and no authenticated request.
- The downstream consumer's V2 smoke remains a separate acceptance gate. The actual-core result
  does not run that consumer, HLS, long downloads, or native playback. The `/x/player/v2` subtitle
  API is unrelated to the consumer's V2 smoke terminology.

## Validation

- `cargo test -p bbdown-core --test dynamic_device_cookie --locked --offline`: 2 passed, 0 failed.
- `cargo test -p bbdown-core --test dynamic_playback_live --no-run --locked --offline`: compiled successfully;
  the live test was not executed.
- `cargo clippy -p bbdown-core --tests --locked --offline -- -D warnings`: passed after correcting
  test formatting/lint issues. Rust/Cargo `1.99.1` followed the repository's stable toolchain.
- `cargo fmt --all -- --check` and project-journal validation: passed.
- `git diff --check`: passed.
- The ignored `legacy_space_dynamic_playback_plan_live_acceptance` test then ran once against the
  explicitly selected existing store's default profile and passed in 2.97 seconds. This confirms
  actual-core candidate production for this run only; it is not a consumer smoke or media-playback
  result.
- On the same frozen source head, `just ci` passed with 979 workspace tests passed and 4 ignored;
  stable Rust/Cargo `1.99.1` and MSRV `1.95.0` checks passed. The standalone CLI E2E rerun passed
  197 tests and is separate from that workspace count. Core publish dry-run packaging validation
  passed without publishing. The existing `spin 0.9.8` yanked-dependency warning was retained; no
  dependency or version change was made.

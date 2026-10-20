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

- No authenticated live request has been made in this follow-up. The local sample manifest's
  credential-file target is absent; an existing file/profile must be selected before the live run.
- Actual-core live playback-candidate acceptance remains pending. Run the ignored test once with
  the selected existing Web credential, record the source revision and UTC time, and diagnose any
  failure at the reported stage without treating historical HTTP 412 as a current confirmed bug.
- The downstream consumer's V2 smoke remains a separate acceptance gate. This repository test
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

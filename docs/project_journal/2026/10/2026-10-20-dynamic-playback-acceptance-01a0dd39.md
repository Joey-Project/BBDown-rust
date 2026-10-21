---
id: 20261020-dynamic-playback-acceptance-01a0dd39
title: Dynamic Playback Candidate Acceptance
status: active
created: 2026-10-20
updated: 2026-10-21
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
- On 2026-10-21, one bounded V2/HLS compatibility smoke completed using an isolated temporary
  consumer adaptation at source head `de48b1abad3ca4104041c5596bd21348f7e0101a` (historical PR
  [#77](https://github.com/Joey-Project/tvOS-net-player/pull/77)), pinned to released
  `bbdown-core 0.11.0` source commit
  `0a3ccd624647017e10ec893ffd9f3d73bf507631`. From the temporary consumer checkout, the recorded
  invocation was:

  ```sh
  python3 "$BOUNDED_PTY_RUNNER" \
    --log "$PRIVATE_LOG" \
    --max-bytes 1048576 \
    --timeout-seconds 300 \
    -- env -u NO_PROXY -u RUST_LOG \
    RUSTUP_TOOLCHAIN=1.95.0 \
    BILIBILI_LIVE_E2E_BBDOWN_CREDENTIAL_PATH="$OPERATOR_SELECTED_CREDENTIAL_FILE" \
    BILIBILI_LIVE_E2E_BBDOWN_CREDENTIAL_PROFILE="$OPERATOR_SELECTED_PROFILE" \
    BILIBILI_LIVE_E2E_CASES=authenticated-space-dynamic \
    scripts/test-bilibili-live.sh
  ```

  `$BOUNDED_PTY_RUNNER` and `$PRIVATE_LOG` stand for the operator's temporary outer runner and
  private log path. This task-local wrapper is not shipped with the consumer repository; it
  enforced a 300-second wall-time limit and a 1 MiB captured-output limit. The authenticated
  fixture requires the credential file and profile variables shown in the command; they take
  operator-selected private values, whose local path and profile name are intentionally not
  recorded. The wrapper
  invoked `scripts/test-bilibili-live.sh`, which ran from the consumer checkout:

  ```sh
  cargo test \
    --manifest-path "$ROOT_DIR/Cargo.toml" \
    --package tvos-net-player-cache-server \
    --test bilibili_live_e2e \
    --locked \
    -- --ignored --nocapture
  ```

  The recorded command has no `BILIBILI_LIVE_E2E_FIXTURE` override. The script defaults to its
  checkout's `.agents/skills/bilibili-live-e2e/references/live-cases.json`; the observed selected
  fixture was `authenticated-space-dynamic`, with `minimum_candidates: 1`, first-candidate
  selection, 360p/H.264 preferences, and `prefer_tv_api: false`. The recorded command sets no
  `BILIBILI_LIVE_E2E_FULL_FILL`, `BILIBILI_LIVE_E2E_FORCE_MEDIA_REFRESH`,
  `BILIBILI_LIVE_E2E_OFFLINE_SECONDS`, or `BILIBILI_LIVE_E2E_SUSTAINED_SECONDS` override; the
  harness defaults full-fill off and offline/sustained durations to none. No full-fill, offline, or
  sustained phase is claimed for this smoke. The test used a temporary cache root and loopback
  gRPC/media services on ephemeral ports. The receipt/log record no absolute UTC wall-clock
  timestamp for this consumer run. A bounded search of available task history found only a
  retrospective pass summary, not its invocation or completion envelope; the scanner stopped at
  an oversized record before the current history suffix could be validated. The absolute time
  remains unknown. The historical route is also unknown: this command cleared `NO_PROXY` and
  `RUST_LOG` only, the outer runner inherited its environment, and no proxy environment values were
  recorded. The test client does not disable environment proxy discovery, so this run cannot be
  described as direct or proxied.

  One authenticated SpaceDynamic case passed in 48.135 seconds, covering the V2 task and playable
  HLS playlist/master/result flow; the inner Rust test reported 47.40 seconds and 1 passed, 0 failed,
  0 ignored, with 60 filtered out. Teardown
  completed and the designated source/credential-file integrity checks were unchanged. This is
  compatibility evidence only, not completed or accepted downstream integration. The downstream
  maintainer owns dependency upgrade, adaptation, acceptance, and merge; this run creates no BBDown
  requirement to merge PR #77. The smoke is limited to consumer HTTP/HLS behavior and does not
  establish native decoding, sustained playback, full-fill, offline playback, or deployment. See the separate
  [bounded media CDN acceptance](2026-10-21-bounded-media-cdn-acceptance-01a0dd39.md).
- On 2026-10-21, one new bounded consumer smoke passed on consumer source head
  `879d852cd2a9a45e5b2f806036c77191ec7bf233`, pinned to the same released `bbdown-core 0.11.0`
  source revision `0a3ccd624647017e10ec893ffd9f3d73bf507631`. Compared with the historical
  consumer head above, intervening changes were limited to README, a Swift login test,
  `PROJECT_STATE`, and the acceptance journal; Rust, Cargo, and the live harness were unchanged.
  The task-local supervisor verified this exact consumer head and a tracked-clean checkout before
  starting the test. It ran the existing consumer entry point:

  ```sh
  gtimeout --kill-after=5s 330s env \
    BILIBILI_LIVE_E2E_BBDOWN_CREDENTIAL_PATH="$OPERATOR_SELECTED_CREDENTIAL_FILE" \
    BILIBILI_LIVE_E2E_BBDOWN_CREDENTIAL_PROFILE="$OPERATOR_SELECTED_PROFILE" \
    python3 "$TASK_LOCAL_SUPERVISOR" --execute
  ```

  This temporary supervisor is not shipped with the consumer repository. It copied only `PATH`,
  `HOME`, `TMPDIR`, `CARGO_HOME`, and `RUSTUP_HOME` when present into a minimal child environment,
  then supplied these settings and operator-selected values, kept as placeholders here:

  ```sh
  RUSTUP_TOOLCHAIN=1.95.0
  CARGO_NET_OFFLINE=true
  BILIBILI_LIVE_E2E_BBDOWN_CREDENTIAL_PATH="$OPERATOR_SELECTED_CREDENTIAL_FILE"
  BILIBILI_LIVE_E2E_BBDOWN_CREDENTIAL_PROFILE="$OPERATOR_SELECTED_PROFILE"
  BILIBILI_LIVE_E2E_CASES=authenticated-space-dynamic
  ```

  All eight upper- and lowercase `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY`
  variables and all other inherited `BILIBILI_LIVE_E2E_*` overrides were omitted. A bounded
  `scutil --proxy` preflight reported every recognized HTTP, HTTPS, SOCKS, PAC, and related proxy
  enable flag false. The task-local supervisor recorded the run from `2026-10-21 22:17:13.725 UTC`
  through `2026-10-21 22:18:04.901 UTC` (51.176 seconds). The start is immediately before launching
  the bounded runner, after the proxy/source preflight; the finish follows runner exit and reaping.
  The window includes Cargo startup and the test, but excludes the earlier preflight and later
  receipt serialization. The result was exit 0, no timeout, and 427 captured
  output bytes under the 300-second/1-MiB limits. Cargo used the warm Rust 1.95 test build in 0.44
  seconds; the selected Rust test passed in 50.58 seconds (1 passed, 0 failed, 0 ignored, 60
  filtered out). The existing script ran `cargo test --manifest-path "$ROOT_DIR/Cargo.toml"
  --package tvos-net-player-cache-server --test bilibili_live_e2e --locked -- --ignored
  --nocapture`, and the log identifies `authenticated-space-dynamic` and
  `bilibili_live_cases_resolve_and_create_playable_hls`.

  The selected profile's Web cookie was used only for `api.bilibili.com`; media CDN requests carried
  no cookie, and no restricted-area reverse resolver or access-key route was selected. This is one
  bounded consumer compatibility run under an environment with no configured proxy
  variables or enabled system proxy settings; it does not independently establish network
  topology. It covers the selected V2 task and playable HLS playlist/master/result flow, not native
  decoding, sustained playback, full-fill, offline playback, deployment, or downstream integration
  acceptance. No credential refresh, store write, secret output, or credential-file integrity/hash
  check was performed for this run. It creates no requirement to merge PR #77. The earlier
  `de48b1a...` smoke remains historical evidence with unknown time and proxy route.
- The `/x/player/v2` subtitle API remains unrelated to the consumer's V2 smoke terminology.

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

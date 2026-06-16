---
id: 20260615-019ecf-v0-4-credential-danmaku-roadmap
title: v0.4.0 Credential And Danmaku Roadmap
status: active
created: 2026-06-15
updated: 2026-06-16
branch: wip/v0-4-roadmap-housekeeping
pr: https://github.com/Joey-Project/BBDown-rust/pull/42
supersedes: [20260614-019ec8-credential-lifecycle-roadmap]
superseded_by:
---

# v0.4.0 Credential And Danmaku Roadmap

## Summary
- `v0.3.0` shipped as a GitHub Release and crates.io `bbdown-core` package after the feed/list
  sequence landed.
- `master` moves to the `0.4.0` development line.
- The `0.4.0` work is split into eight sequential PRs, each starting from the latest `master` after
  the previous PR merges.

## Current State
- PR 1 records the shipped `v0.3.0` state, bumps the workspace to the `0.4.0` development line, and
  records this roadmap.
- PR 2 adds unified QR login ticket/output surfaces for existing WEB and TV login flows.
- PR 3 adds credential health-check diagnostics for WEB cookie, generic `access_key`, and TV
  `tv_access_key` credentials.
- PR 4 adds the credential profile storage model while preserving the current default profile.
- PR 5 makes CLI and embedding credential selection profile-aware.
- PR 6 adds the core generic `access_key` acquisition flow after validating the BiliPlus URL/QR
  authorization behavior observed in the prior `bilibili-helper` implementation.
- PR 7 adds CLI/docs integration for generic `access_key` acquisition.
- PR 8 will add append-only danmaku update support for already downloaded entries, including XML
  sidecar merging and regeneration of selected derived formats such as ASS.

## Next Steps
- Continue to PR 8 after the CLI/docs access-key acquisition slice lands, updating `master` after the
  merge before cutting the next branch.
- Keep automatic credential refresh and remaining BBDown parity items outside this eight-PR sequence
  unless explicitly reprioritized.

## Evidence
- Published GitHub Release `v0.3.0`:
  `https://github.com/Joey-Project/BBDown-rust/releases/tag/v0.3.0`.
- Published crate version: `bbdown-core` `0.3.0`.
- Promote workflow: `https://github.com/Joey-Project/BBDown-rust/actions/runs/27527684427`.
- BiliPlus access-key login preflight on 2026-06-15 showed `/login?balh_auth=1` still serves a page
  with QR-code rendering, an authorization URL, and callback data containing `access_key`,
  `refresh_token`, and expiration fields.
- PR 6: `https://github.com/Joey-Project/BBDown-rust/pull/47` on branch
  `wip/generic-access-key-auth`.
- PR 6 local validation before review: `cargo test -p bbdown-core login --locked` and
  `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- PR 6 review fixes accept both auth and callback sender origins for BALH `postMessage` imports,
  lazily parse fallback expiration fields, and preserve raw query text before trailing fragments.
- PR 7 branch: `wip/access-key-login-cli`.
- PR 7 local targeted validation: `cargo test -p bbdown-cli access_key --locked`.
- PR 7 local full validation: `just ci`.
- PR 7 independent review found that terminal access-key paste paths could echo callback tokens in
  scrollback; the CLI now requires `--file` or piped/redirected `--stdin` for pasted BALH data and
  documents the no-interactive-paste policy.
- PR 7 Codex review-gate found that implicit piped stdin could still be consumed without `--stdin`;
  the CLI now rejects missing input-source flags and requires callers to opt in to pipe consumption.

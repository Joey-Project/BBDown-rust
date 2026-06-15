---
id: 20260615-019ecf-v0-4-credential-danmaku-roadmap
title: v0.4.0 Credential And Danmaku Roadmap
status: active
created: 2026-06-15
updated: 2026-06-15
branch: wip/v0-4-roadmap-housekeeping
pr:
supersedes: []
superseded_by:
---

# v0.4.0 Credential And Danmaku Roadmap

## Summary
- `v0.3.0` shipped as a GitHub Release and crates.io `bbdown-core` package after the feed/list
  sequence landed.
- `master` moves to the `0.4.0` development line.
- The `0.4.0` work is split into four sequential PRs, each starting from the latest `master` after
  the previous PR merges.

## Current State
- PR 1 records the shipped `v0.3.0` state, bumps the workspace to the `0.4.0` development line, and
  records this roadmap.
- PR 2 will add unified login QR output for existing login flows and credential health-check
  diagnostics for WEB cookie, generic `access_key`, and TV `tv_access_key` credentials.
- PR 3 will add credential profiles and an in-project generic `access_key` acquisition flow after
  validating the BiliPlus URL/QR authorization behavior observed in the prior `bilibili-helper`
  implementation.
- PR 4 will add append-only danmaku update support for already downloaded entries, including XML
  sidecar merging and regeneration of selected derived formats such as ASS.

## Next Steps
- Land PR 1 with full validation and the normal three-review merge gate.
- After PR 1 merges, update `master`, cut the PR 2 branch, and implement unified login QR output and
  credential health diagnostics.
- Keep automatic credential refresh and remaining BBDown parity items outside this four-PR sequence
  unless explicitly reprioritized.

## Evidence
- Published GitHub Release `v0.3.0`:
  `https://github.com/Joey-Project/BBDown-rust/releases/tag/v0.3.0`.
- Published crate version: `bbdown-core` `0.3.0`.
- Promote workflow: `https://github.com/Joey-Project/BBDown-rust/actions/runs/27527684427`.
- BiliPlus access-key login preflight on 2026-06-15 showed `/login?balh_auth=1` still serves a page
  with QR-code rendering, an authorization URL, and callback data containing `access_key`,
  `refresh_token`, and expiration fields.

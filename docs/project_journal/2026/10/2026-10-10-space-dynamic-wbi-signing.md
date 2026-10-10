---
id: 20261010-space-dynamic-wbi-signing
title: Personal-Space Dynamic WBI Signing
status: completed
created: 2026-10-10
updated: 2026-10-10
branch: wip/space-dynamic-wbi-signing
pr:
supersedes: []
superseded_by:
---

# Personal-Space Dynamic WBI Signing

## Summary

- Sign personal-space dynamic feed page requests with the existing WBI mixin-key lookup and query
  signer. Sign each complete page query, including continuation `offset`, with one key lookup per
  collection.
- Preserve the existing Following feed request behavior; do not add WBI signing to that branch.

## Behavior And Boundaries

- WBI signing adds the existing `wts` and `w_rid` fields while retaining applicable Space query
  parameters, including `offset` on continuation pages.
- Keep nav lookup failures and malformed key data on credential-safe error paths. Do not log Web
  cookies, signature values, raw credential-backed response bodies, or private account data, and do
  not send Web cookies to public reverse proxies.
- This core workstream does not change the consumer application or claim playback acceptance.

## Supplied Consumer Evidence

- A consumer-supplied, bounded official API A/B report observed unsigned Space requests returning
  HTTP 412 / provider `-412`, then a signed request returning HTTP 200 / provider `0` with 13 dynamic
  items and `has_more=true`; a repeated unsigned request returned HTTP 412 / provider `-412` again.
  The signed query added only `wts` and `w_rid`.
- This is supplied consumer evidence, not a live run by this workstream. Thirteen dynamic items do
  not establish thirteen playable videos or playback acceptance.

## Validation

- `env RUSTUP_TOOLCHAIN=1.99.0 just ci` passed on attempt 3 (exit 0, 64.80 seconds): formatting,
  strict all-target Clippy, and the locked workspace check with Rust 1.95 passed; workspace tests
  reported 811 passed, 0 failed, and 3 ignored; the separate CLI repeat reported 154 passed, 0
  failed, and 0 ignored.
- The package dry run passed with 29 files (1.8 MiB; 274.3 KiB compressed). Upload was aborted;
  no package was uploaded. Existing-version and yanked `spin 0.9.8` warnings were nonblocking.
- The post-run receipt confirmed all 31 tracked source/config inputs were stable. No authenticated
  live calls or consumer end-to-end tests were run.

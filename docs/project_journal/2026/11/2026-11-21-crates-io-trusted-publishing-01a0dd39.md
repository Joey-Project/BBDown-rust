---
id: 20261121-crates-io-trusted-publishing-01a0dd39
title: crates.io Trusted Publishing
status: active
created: 2026-11-21
updated: 2026-11-21
branch: codex/crates-io-trusted-publishing
pr:
supersedes: []
superseded_by:
---

# crates.io Trusted Publishing

## Summary

- Migrate `bbdown-core` release promotion from a long-lived crates.io token to GitHub OIDC trusted
  publishing, with a pinned `rust-lang/crates-io-auth-action` and checksum-based recovery that skips
  authentication for an already matching crate version.
- Document the publisher mapping and old-RC compatibility in both release runbooks.

## Current State

- The workflow requires only `contents: read` and `id-token: write` for the crate-publish job. It
  accepts only the helper states `matching` and `absent`; authentication and publication run only
  for `absent`.
- The user confirmed the crates.io publisher mapping: crate `bbdown-core`, owner `Joey-Project`,
  repository `BBDown-rust`, workflow `promote-release-candidate.yml`, environment `crates-io`. This
  is user-reported configuration, not an independent site readback or OIDC exchange.
- Existing `CARGO_REGISTRY_TOKEN` configuration remains necessary for recovery from older RC tags,
  whose promotion runs use the workflow stored at each tag.
- No token exchange, crate publication, or live release validation has been performed.

## Next Steps

- On the next separately authorized release, create the first trusted-publishing RC from source
  containing this migration and validate the OIDC publish path.
- Retain the legacy token until that publish succeeds and recovery of pre-migration RC tags is no
  longer needed; then remove the obsolete secret.

## Local Validation

- `actionlint` passed for the promotion workflow; `git diff --check` passed.
- The extracted state-check step passed `bash -n`, ShellCheck, and offline mock cases for `matching`,
  `absent`, helper failure, and an unknown state. Structural checks confirmed job permissions,
  absent-only authentication/publication, the pinned action, and step-scoped token wiring.
- The offline harness is retained at `/private/tmp/bbdown-trusted-publishing-20261121-01a0dd39/`.
  These checks do not exercise crates.io configuration readback, OIDC exchange, or publication.

## Evidence

- Workflow: `.github/workflows/promote-release-candidate.yml`.
- Runbooks: `docs/release.md` and `docs/release.zh-CN.md`.

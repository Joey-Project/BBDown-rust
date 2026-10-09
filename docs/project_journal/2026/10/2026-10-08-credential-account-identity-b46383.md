---
id: 20261008-b46383
title: Credential Account Identity
status: completed
created: 2026-10-08
updated: 2026-10-09
branch: wip/credential-account-identity
pr:
supersedes: []
superseded_by:
---

# Credential Account Identity

## Scope
- Supply the narrow core prerequisite for the server-owned tvOS-net-player credential lifecycle.
- Add `BiliClient::credential_account_identity(CredentialKind)` and the typed `CredentialAccountIdentity` result, without changing existing health-check, login, storage, or CLI behavior.
- Reuse the official Web nav and signed generic/TV OAuth-info request paths. Require successful provider status and a positive account ID; bound the response body to 64 KiB.
- Keep cookies off OAuth probes and redact credentials and account IDs from debug/errors. The embedding server remains responsible for session authority, same-account checks, renewal policy, and private storage.

## Delivery
- Implement and test in a separate worktree from `master` commit `d28b2c7539b564c6e492bbdb5b103817ececd0e9`; preserve unrelated dirty source-checkout changes.
- Run focused mocked probes, complete workspace formatting/lint/tests, declared-toolchain checks, and the crate publish dry run.
- Use current-head GitHub Codex review and CI, with every PR conversation resolved before merge. No local formal review lane or release publication is included.

## Validation Boundaries
- Rust/Cargo `1.95.0`: formatting, workspace/all-target Clippy with warnings denied, declared-toolchain workspace check, complete workspace tests (732 passed, 3 opt-in live tests ignored), CLI e2e (146 passed), and core publish dry run passed. The identity-specific tests passed (4), as did unchanged health-check coverage (9). No crate was published.
- Implementation and validation were delegated to GPT-6 Luna Max; the lead integrated documentation and delivery. No local formal review lane was run.
- Deterministic provider fixtures establish parsing, signing, cookie separation, limits, and redaction contracts, not live account validity.
- Server integration and saved-profile validation belong to the consumer's credential-lifecycle slice after updating its exact dependency revision.
- This additive API is Git-only until a later crate release; the previously published `0.7.0` does not include it.

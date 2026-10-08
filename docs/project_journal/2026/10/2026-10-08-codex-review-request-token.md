---
id: 20261008-codex-review-request-token
title: Forward the Codex Review Request Token
status: completed
created: 2026-10-08
updated: 2026-10-08
branch:
pr:
supersedes: []
superseded_by:
---

# Forward the Codex Review Request Token

## Summary
- The controller passes the optional repository secret to the v2 action's `review_request_token` input.

## Current State
- The controller retains `github.token` as `github_token` and forwards `secrets.CODEX_REVIEW_GATE_REQUEST_TOKEN` separately for review-request authentication.
- This consumer change does not create or configure the repository secret and does not alter the verifier or other controller policy.

## Next Steps
- None within this controller-input change.

## Evidence
- `.github/workflows/codex-review-gate-controller.yml`.

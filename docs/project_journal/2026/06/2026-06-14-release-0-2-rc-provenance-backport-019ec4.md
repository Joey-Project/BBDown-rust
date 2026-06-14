---
id: 20260614-019ec4-release-0-2-rc-provenance-backport
title: Release 0.2.0 RC Provenance Backport
status: completed
created: 2026-06-14
updated: 2026-06-14
branch: wip/release-0.2-rc-provenance
pr:
supersedes: []
superseded_by:
---

# Release 0.2.0 RC Provenance Backport

## Summary
- Backported the RC workflow provenance fix from `master` to the `release/0.2.0` source branch.
- `Create Release Candidate` now accepts the repository default branch or `release/*` as the
  workflow ref, and the RC tag target is the workflow ref commit.
- Release runbooks and architecture docs on this branch now describe the source-branch RC flow.

## Current State
- `v0.2.0-rc.1` already exists and predates the later 0.2.0 release branch contents.
- The next 0.2.0 candidate should be `v0.2.0-rc.2`.
- After this backport merges, run `Create Release Candidate` from `release/0.2.0` with
  `version=0.2.0`.

## Next Steps
- Ensure the `release-candidate` environment allows deployments from `release/*`.
- Trigger `Create Release Candidate` from `release/0.2.0`.
- Verify `v0.2.0-rc.2` points at the `release/0.2.0` HEAD used by the workflow run.
- Promote `v0.2.0-rc.2` after verification and environment approvals.

## Evidence
- Incorrect default-branch provenance run was cancelled:
  `https://github.com/Joey-Project/BBDown-rust/actions/runs/27478465107`.
- Master-side workflow provenance fix landed in PR #34.

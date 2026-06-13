---
id: 20260613-019ec3-rc-from-release-branch
title: Release Candidate Source Branch Provenance
status: completed
created: 2026-06-13
updated: 2026-06-13
branch: wip/rc-from-release-branch
pr:
supersedes:
  - 20260613-019ec2-release-0-2-0-branch-rc2
superseded_by:
---

# Release Candidate Source Branch Provenance

## Summary
- Removed the `source_ref` RC workflow path so release candidate run provenance matches the
  released source branch.
- Allowed `Create Release Candidate` to run from either the repository default branch or a
  `release/*` branch.
- Kept RC tags protected by the release GitHub App and the `release-candidate` environment gate.

## Current State
- Current-development release candidates run from `master`.
- Maintenance release candidates run from their release branch, such as `release/0.2.0`.
- The next `0.2.0` RC should be created by running `Create Release Candidate` from
  `release/0.2.0`, producing `v0.2.0-rc.2` after the already existing `v0.2.0-rc.1`.

## Next Steps
- Merge the workflow and documentation fix.
- Backport the workflow fix to `release/0.2.0` so the release branch can run its own RC workflow.
- Ensure the `release-candidate` environment permits deployments from both `master` and
  `release/*`.
- Run `Create Release Candidate` from `release/0.2.0` with `version=0.2.0`.
- Verify the created `v0.2.0-rc.2` tag points at the `release/0.2.0` HEAD used by that workflow
  run.

## Evidence
- Cancelled incorrect provenance run:
  `https://github.com/Joey-Project/BBDown-rust/actions/runs/27478465107`.
- Release source branch:
  `origin/release/0.2.0` at `a1c6062561f3902ea31394151b50a57764f521e0`.

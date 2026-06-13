---
id: 20260613-019ec2-release-0-2-0-branch-rc2
title: Release 0.2.0 Branch And RC2 Source
status: completed
created: 2026-06-13
updated: 2026-06-13
branch: wip/release-source-ref
pr:
supersedes: []
superseded_by: 20260613-019ec3-rc-from-release-branch
---

# Release 0.2.0 Branch And RC2 Source

## Summary
- Pushed `release/0.2.0` at `a1c6062561f3902ea31394151b50a57764f521e0`, the commit immediately
  before the abandoned `0.3.0` version bump PR.
- Kept `v0.3.0-rc.1` as an abandoned release candidate because promotion was cancelled before any
  final tag, GitHub Release, or crates.io publication.
- Updated `Create Release Candidate` so it could release an explicit `source_ref` from the default
  branch. That approach was later superseded because the release candidate run provenance should
  itself be the released source branch.

## Current State
- Existing `v0.2.0-rc.1` targets `1faf022feb116c408ad4b457a574f1a78884099e` and predates the
  later playback and APP gRPC work.
- The intended `v0.2.0-rc.2` source is `release/0.2.0`, targeting
  `a1c6062561f3902ea31394151b50a57764f521e0`.
- `bbdown-core` and `bbdown-cli` are both `0.2.0` on that release branch.

## Next Steps
- Merge the workflow/runbook PR.
- Use the superseding RC provenance workflow so `Create Release Candidate` runs from
  `release/0.2.0` itself before creating `v0.2.0-rc.2`.
- Promote `v0.2.0-rc.2` after release verification and environment approvals.

## Evidence
- `git push -u origin release/0.2.0`.
- Cancelled `v0.3.0-rc.1` promotion run:
  `https://github.com/Joey-Project/BBDown-rust/actions/runs/27477750034`.

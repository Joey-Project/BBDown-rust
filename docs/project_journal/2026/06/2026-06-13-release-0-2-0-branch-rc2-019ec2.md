---
id: 20260613-019ec2-release-0-2-0-branch-rc2
title: Release 0.2.0 Branch And RC2 Source
status: completed
created: 2026-06-13
updated: 2026-06-14
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
- `v0.2.0-rc.2` was created from `release/0.2.0` after the release-candidate environment allowed
  deployments from `release/*`.
- `v0.2.0-rc.2` and final tag `v0.2.0` both peel to
  `49c023ffc40f48f64164fbee8ec0920a044ae845`.
- GitHub Release `v0.2.0` is published as a non-prerelease release with Linux, macOS, and Windows
  CLI archives plus checksum sidecars.
- crates.io lists `bbdown-core` `0.2.0` as the newest stable version.

## Next Steps
- No further `0.2.0` release action is pending. Continue feature work on the `0.3.0` development
  line from `master`.

## Evidence
- `git push -u origin release/0.2.0`.
- Cancelled `v0.3.0-rc.1` promotion run:
  `https://github.com/Joey-Project/BBDown-rust/actions/runs/27477750034`.
- `Create Release Candidate` run for `v0.2.0-rc.2`:
  `https://github.com/Joey-Project/BBDown-rust/actions/runs/27492844333`.
- `Promote Release Candidate` run for `v0.2.0`:
  `https://github.com/Joey-Project/BBDown-rust/actions/runs/27493414990`.
- GitHub Release: `https://github.com/Joey-Project/BBDown-rust/releases/tag/v0.2.0`.
- crates.io package: `https://crates.io/crates/bbdown-core`.

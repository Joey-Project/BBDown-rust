---
id: 20261004-codex-review-gate-v2-bbdown
title: Codex Review Gate V2 Migration
status: completed
created: 2026-10-04
updated: 2026-10-04
branch:
pr:
supersedes: []
superseded_by:
---

# Codex Review Gate V2 Migration

## Summary

- The repository's Codex review gate uses the canonical v2 verifier and protected controller, with `@JoeyTeng` owning the workflow control plane.

## Current State

- The read-only verifier uses `JoeyTeng/codex-review-gate-action@v2`, grants only the canonical read permissions including `actions: read`, and sets `CODEX_REVIEW_GATE_REQUEST_AUTHOR_PERMISSION=any`.
- The former v1 producer at the canonical gate path is replaced; the v2 controller is installed, and the previous release CODEOWNERS entries remain alongside the dedicated control-plane block.
- Unrelated release and CI workflows are unchanged.
- This consumer-file migration does not modify or assert the production ruleset's required-check state.

## Evidence

- Verifier and controller bytes match the canonical v2 templates from `Joey-Tools/codex-review-gate` source commit `fbc2ed10abbc4dd99717f2ff55fba390609ad6c4`.

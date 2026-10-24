---
id: 20261024-v2212-event
title: Codex Review Gate Event Upgrade
status: completed
created: 2026-10-24
updated: 2026-10-24
branch:
pr:
supersedes: []
superseded_by:
---

# Codex Review Gate Event Upgrade

## Summary
- Updated the canonical v2.2.12 verifier/controller pair for default-branch consumers handling edited and native stacked pull requests.

## Current State
- Non-base pull-request edits retain an ignored verifier job, distinct run title, and isolated concurrency group; base-retarget edits and stacked pull requests follow the normal verifier path.
- The controller refreshes only expected `codex-review-gate-verifier/` workflow runs and isolates unrelated workflow runs.
- The floating `@v2` action, configurable runner-label macro and fallback, review-request token mapping, auto-request setting, limits profile, and permission envelope remain canonical.
- Production rulesets and live repository state were not changed by this consumer-file update.

## Evidence
- Verifier and controller bytes match source commit `0746d53b8f5dc4851f6f3dfe63913d12cf53f807` (Git blobs `b7d5f846fb2af067f7a2bd03c060544a4ff4ac34` and `123137c518f53238183616695dae0246bb5402ca`).
- Exported canonical workflow validators accepted both files; `git diff --check` passed.
- The targeted canonical bootstrap contract test passed. Installed actionlint 1.7.12 rejects `stacked` as an unknown `pull_request` activity type on the canonical source as well as this consumer.

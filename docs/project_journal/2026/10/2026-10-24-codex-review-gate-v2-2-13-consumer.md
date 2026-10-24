---
id: 20261024-bbdown-v2213-consumer
title: Codex Review Gate v2.2.13 Consumer Workflow Update
status: completed
created: 2026-10-24
updated: 2026-10-24
branch: wip/non-skipping-review-gate
pr:
supersedes: []
superseded_by:
---

# Codex Review Gate v2.2.13 Consumer Workflow Update

## Summary

- Refresh the consumer verifier and controller from the prior v2.2.12 form to
  the canonical v2.2.13 pair. The earlier v2.2.12 consumer PR #123 is history,
  not this follow-up's PR.
- Remove the verifier's ignored-edit branch. All six configured pull-request
  event types now use the same `codex/github-review-gate` check and stable
  `codex-review-gate-verifier/<PR>/<SHA>` run title.
- Keep cancellation scoped to one verifier concurrency group per pull request;
  the controller continues to accept only expected verifier completion runs.

## Current State

- Both installed workflow files match the v2.2.13 canonical consumer pair.
- Floating `@v2`, runner labels and fallback, review-request token mapping,
  automatic-request and limits settings, and the permission envelope remain
  unchanged.
- No product code, production rulesets, or repository settings changed.

## Evidence

- Canonical v2.2.13 source is tracked by
  [implementation PR #146](https://github.com/Joey-Tools/codex-review-gate/pull/146);
  Action release and consumer installation remain separate workstreams.
- Verifier and controller match canonical source snapshot
  `325ba1a09f371f04b91d3bf624bd2e074dd0f349` byte-for-byte. Exported workflow
  validators, project-journal validation, and whitespace checks pass.
- Installed actionlint 1.7.12 reports only
  `invalid activity type "stacked" for "pull_request" Webhook event.`; this
  known schema diagnostic also applies to the canonical verifier.
- No product tests or remote CI were rerun; no live runtime execution is
  claimed.

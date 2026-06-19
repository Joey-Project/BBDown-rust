---
id: 20260619-019f10-live-e2e-fixture-skill
title: Live E2E Fixture Skill
status: completed
created: 2026-06-19
updated: 2026-06-19
branch: feature/repo-live-e2e-skill
pr:
supersedes: []
superseded_by:
---

# Live E2E Fixture Skill

## Summary

- Added the repo-local `$bbdown-live-e2e-fixtures` skill for selecting real Bilibili URLs during
  opt-in live e2e validation.
- Recorded canonical public, multi-page, restricted bangumi series, and restricted bangumi episode
  fixtures in `.agents/skills/bbdown-live-e2e-fixtures/references/live-fixtures.md`.
- Aligned `live-e2e.samples.example.json` with the same fixture set so operators copying the sample
  manifest start from the current URLs.
- Added a short `AGENTS.md` pointer and updated project state/TODO entrypoints.

## Evidence

- Fixture reference includes:
  - `https://www.bilibili.com/video/BV1QtjA6BEB8/`
  - `https://www.bilibili.com/video/BV1uW4y1s7zN/`
  - `https://www.bilibili.com/bangumi/media/md28338980`
  - `https://www.bilibili.com/bangumi/play/ep664928`

## Next Steps

- Use the skill for future feature PR live validation reports, especially restricted-area resolver
  and download execution changes.

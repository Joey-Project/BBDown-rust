---
id: 20261006-danmaku-preserving-refresh
title: Strictly Preserving Danmaku Refresh
status: active
created: 2026-10-06
updated: 2026-10-06
branch:
pr:
supersedes: []
superseded_by:
---

# Strictly Preserving Danmaku Refresh

## Summary

- Track a later-release enhancement for refreshing downloaded danmaku while preserving every
  existing XML and ASS detail. This is additional backlog, not a claim that the completed 0.4.0
  append-only XML and ASS regeneration workflow already meets these stricter requirements.
- This work is deferred from `v0.7.0` and does not change its release scope.

## Current State

- The existing workflow is documented in
  `docs/project_journal/2026/06/2026-06-18-danmaku-append-update-019f0a.md`. The downstream request
  reports preservation boundaries for unknown existing XML nodes and whole-file ASS regeneration
  in its current integration. That integration does not meet the strict preservation requirement
  below.
- The request was transferred from the Telegram-Video-Downloader task. Its bot-side consumer pinned
  BBDown-rust revision `0a94b071bbc1897ec1d1fec9dfcf7883c5754a15`; that bot should update its
  dependency after the core capability is implemented.
- Responsibility boundary: the bot owns historical-task buttons, whole-directory batch selection,
  queueing, progress, recovery, macOS File Provider coordination, and safe publication. BBDown core
  owns content parsing and merging, with CLI options exposing the reusable capability.

## Next Steps

- Add a reusable core API and CLI options for strict append-only refresh while keeping existing
  public APIs compatible.
- For XML, retain every existing node, attribute, text body, and duplicate; append only newly fetched
  danmaku. For ASS, retain styles, every event, ASS-only or custom existing content, and append only
  new events without rebuilding the whole file.
- For non-empty old files that are damaged, use an unknown format, or cannot be safely merged,
  return an explicit skip/error and preserve the original. Network, parse, and write failures must
  also leave old content intact; expose a staged result or safe publication interface for callers
  coordinating final replacement.
- Cover XML node/attribute preservation and existing duplicates, ASS styles/old events/custom
  content, and old-content retention on failures with core unit tests, mock-download tests, and CLI
  end-to-end tests.
- After implementation and validation, update the pinned bot dependency and integrate the bot-side
  workflow.

## Evidence

- Existing implementation record: `docs/project_journal/2026/06/2026-06-18-danmaku-append-update-019f0a.md`.
- Downstream bot dependency pin: `0a94b071bbc1897ec1d1fec9dfcf7883c5754a15`.

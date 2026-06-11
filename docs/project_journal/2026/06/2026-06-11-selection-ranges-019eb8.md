---
id: 20260611-019eb8
title: Selection Ranges
status: completed
created: 2026-06-11
updated: 2026-06-11
branch: wip/selection-ranges
pr:
supersedes: []
superseded_by:
---

# Selection Ranges

## Summary
- Added structured numeric index selection for normal video pages, season episode indexes, and
  batch collection item indexes.
- Kept existing selectors compatible: `current`, `latest`, `all`, `episode:<epid>`, and
  `page:<index>` still parse and behave as before.
- Exposed list and range selection through both the reusable `bbdown-core` API and the CLI parser.

## Current State
- `Selection::Indices(IndexSelection)` supports ordered, deduplicated selectors such as `1,3-5`.
- The CLI accepts equivalent strings including `1`, `page:1`, `1,3-5`, and `page:2-4,7`.
- Batch planning can stop fetching once it has covered the maximum requested index instead of
  fetching the whole collection for narrow range selections.
- English and Simplified Chinese user-facing docs describe the selector syntax, and embedding docs
  show how to construct the structured API.

## Next Steps
- Continue the remaining BBDown parity backlog from `docs/PROJECT_TODO.md`: additional app/TV
  playurl modes, API/server integration surfaces, aria2 or multi-thread download integration,
  MP4Box muxing, and subtitle-to-SRT conversion.

## Evidence
- Local targeted validation: `cargo test --locked -p bbdown-core selection`.
- Local targeted validation:
  `cargo test --locked -p bbdown-cli --test cli_e2e plan_json_applies_index_range_selection`.
- Local full validation: `just ci`.

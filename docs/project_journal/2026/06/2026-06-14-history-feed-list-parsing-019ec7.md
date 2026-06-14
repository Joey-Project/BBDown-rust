---
id: 20260614-019ec7-history-feed-list-parsing
title: History Feed/List Parsing
status: completed
created: 2026-06-14
updated: 2026-06-14
branch: wip/history-feed-list-parsing
pr:
supersedes: []
superseded_by:
---

# History Feed/List Parsing

## Summary
- Added `history` and `https://www.bilibili.com/account/history` as authenticated watch-history
  inputs.
- Mapped normal-video `archive` history records into `ResolvedContent::Collection` so `info`,
  `plan`, `playback`, and `download` can reuse existing batch selection and normal-video planning
  behavior.
- Documented the current scope boundary: history parsing requires a web cookie and skips non-archive
  history business types such as PGC, live, or article records until those item shapes have
  dedicated collection planning support.

## Current State
- `Input::History` resolves through the shared feed/list selection behavior introduced by the
  previous slice.
- The history fetcher calls `/x/web-interface/history/cursor` with `type=archive`, keeps `business`
  as the returned pagination cursor value, follows the cursor until the requested fetch mode is
  satisfied, deduplicates by `aid/cid`, and renumbers the resulting collection items.
- CLI mock e2e coverage verifies `bbdown info history --json` against a local history cursor
  response.
- English and Simplified Chinese user-facing docs describe history input examples, credential
  requirements, and archive-only scope.

## Next Steps
- Add following/UP page parsing as the next sequential `0.3.0` feed/list PR after this slice lands.
- Keep recommendation and watch-later parsing as later sequential PRs.

## Evidence
- Targeted validation: `cargo test -p bbdown-core history --locked`.
- Targeted validation: `cargo test -p bbdown-core input::tests::parses_common_inputs --locked`.
- Targeted validation: `cargo test -p bbdown-cli --test cli_e2e history --locked`.
- Project journal validation:
  `python3 /Users/joey/.codex/personal-sync/overlays/private/releases/29f61f3e579e2a4166436b963eab301ac5d80d94/personal_codex/skills/project-journal/scripts/project_journal.py validate --repo /Users/joey/Program/Codex-workspace/BBDown-rust`.
- Full local gate: `just ci`, including formatter, clippy, declared MSRV check, workspace tests,
  CLI mock e2e, live manifest unit tests, and crates.io dry-run packaging.
- Local readonly review: helper-backed `codex-readonly` on the history slice returned `LGTM`.
- Independent PR review found that `/account/history/` with a trailing slash should parse as
  history; the parser now filters empty path segments and the input parsing test covers that form.
- GitHub Codex review found that history filtering should use `type=archive` while keeping
  `business` as the cursor value, and that multi-page history records should preserve first-page
  part titles. The resolver and mock tests now cover both behaviors.

---
id: 20260614-019ec8-following-dynamic-feed-parsing
title: Following Dynamic Feed Parsing
status: completed
created: 2026-06-14
updated: 2026-06-14
branch: wip/following-up-feed-list-parsing
pr:
supersedes: []
superseded_by:
---

# Following Dynamic Feed Parsing

## Summary
- Added authenticated following video feed parsing on top of the shared `feed_list` resolver layer.
- Added authenticated space dynamic video feed parsing for `https://space.bilibili.com/<mid>/dynamic`.
- Kept existing `SpaceVideos` behavior for uploader archive pages such as `mid...` and
  `https://space.bilibili.com/<mid>`.

## Current State
- New inputs:
  - `following`
  - `https://t.bilibili.com/`
  - `https://www.bilibili.com/account/dynamic`
  - `https://space.bilibili.com/<mid>/dynamic`
- Dynamic feed requests use the web dynamic feed endpoints and include the stored WEB cookie when
  present.
- The parser currently emits normal-video archive cards and skips non-video dynamic cards.
- Dynamic feed collection items map back to normal video planning before stream, subtitle, danmaku,
  cover, playback, and download handling.

## Next Steps
- Continue with recommendation page parsing as the next `0.3.0` feed/list feature slice.
- Keep broader credential lifecycle work, including health checks, renewal, profiles, and generic
  `access_key` acquisition, in the credential lifecycle roadmap note rather than this feed/list PR.

## Evidence
- Targeted following resolver test:
  `cargo test -p bbdown-core resolves_following_feed_archive_items --locked`.
- Targeted space dynamic planning test:
  `cargo test -p bbdown-core plans_space_dynamic_latest_as_normal_video_entry --locked`.
- Targeted CLI e2e:
  `cargo test -p bbdown-cli --test cli_e2e info_json_resolves_mock_following_collection --locked`.
- Full local gate: `just ci`.

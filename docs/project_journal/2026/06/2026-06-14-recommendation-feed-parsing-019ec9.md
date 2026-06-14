---
id: 20260614-019ec9-recommendation-feed-parsing
title: Recommendation Feed Parsing
status: completed
created: 2026-06-14
updated: 2026-06-14
branch: wip/recommendation-feed-list-parsing
pr:
supersedes: []
superseded_by:
---

# Recommendation Feed Parsing

## Summary
- Added homepage recommendation feed parsing on top of the shared `feed_list` resolver layer.
- Added `RecommendationFeed` input parsing for `recommendations`, `recommendation`, `recommend`,
  and the Bilibili homepage URL.
- Recommendation items map directly to normal-video collection items when the upstream card is an
  `av` recommendation with `aid`/`cid` metadata.

## Current State
- New inputs:
  - `recommendations`
  - `recommendation`
  - `recommend`
  - `https://www.bilibili.com/`
  - `https://bilibili.com/`
- Recommendation fetch uses `/x/web-interface/index/top/feed/rcmd`.
- The resolver skips non-video recommendation cards and requests up to the endpoint-supported
  batch size when explicit index selection needs enough filtered normal-video cards.
- Selected recommendation items plan through the normal video stream, subtitle, danmaku, cover,
  playback, and download surfaces.

## Next Steps
- Continue with watch-later parsing as the final planned `0.3.0` feed/list feature slice.
- Keep single-video related recommendations as a separate future capability if Joey wants parity
  with Bilibili's per-video related list.

## Evidence
- Live endpoint shape check:
  `curl -sS 'https://api.bilibili.com/x/web-interface/index/top/feed/rcmd?ps=3'`.
- Targeted input test: `cargo test -p bbdown-core input::tests::parses_feed_inputs --locked`.
- Targeted recommendation resolver test:
  `cargo test -p bbdown-core resolves_recommendation_feed_items --locked`.
- Targeted recommendation planning test:
  `cargo test -p bbdown-core plans_recommendation_latest_as_normal_video_entry --locked`.
- Targeted CLI e2e:
  `cargo test -p bbdown-cli --test cli_e2e info_json_resolves_mock_recommendation_collection --locked`.

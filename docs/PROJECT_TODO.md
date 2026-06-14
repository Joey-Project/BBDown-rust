# Project TODO

- [completed] Land the Rust workspace, CI, architecture docs, metadata resolver, credential store, and CLI `info/auth` foundation.
- [completed] Add stream resolver chain, download planning, subtitle discovery, and danmaku discovery.
- [completed] Add real file downloads, retry/resume policy, ffmpeg mux integration, and mock e2e download coverage.
- [completed] Add QR login flow and live-test opt-in harness.
- [completed] Add restricted-area proxy resolver ordering and diagnostics.
- [completed] Strengthen local live e2e coverage with a manifest-driven sample matrix.
- [completed] Add GitHub release binary packaging for tagged and manual builds.
- [completed] Add crate publish readiness and dry-run validation.
- [completed] Add clearer stream quality selection and listing support.
- [completed] Expand restricted-area proxy response compatibility.
- [completed] Harden integration APIs and embedding documentation.
- [completed] Add download archive and duplicate decision handling.
- [completed] Add more input parsing and batch collection parsing for B23 short links, cheese
  courses, favorite lists, space videos, collections, and series.
- [completed] Add bilingual human-facing docs and rerun real live e2e samples.
- [completed] Rename the publishable library package to `bbdown-core` before the 0.1.0 release.
- [completed] Add protected release candidate creation and RC promotion workflows for GitHub
  Release and crates.io publication.
- [completed] Add cover download support to the crate and CLI.
- [completed] Keep `master` on the next `0.3.0` development line while publishing `0.2.0` from the
  `release/0.2.0` branch.
- [completed] Add single-download modes for video-only, audio-only, subtitle-only, danmaku-only, and
  cover-only workflows.
- [completed] Add ASS-format danmaku sidecar generation.
- [completed] Add UPOS host replacement controls and PCDN filtering/handling.
- [completed] Add richer output, entry, and mux filename templates for CLI and API downloads.
- [completed] Add richer selection syntax for numeric lists and ranges across video pages, season
  episode indexes, and batch collection items.
- [completed] Add a playback ladder and serializable media request spec for downstream player/cache
  integrations without implementing a player or HLS cache server in this repository.
- [completed] Add AVPlayer-oriented codec/device compatibility profiles and codec-preference
  helpers that expose exact codec strings for downstream validation.
- [completed] Add ABR policy metadata and cache identity helpers so downstream cache servers can
  retain already fetched variants and segments while switching bitrate levels.
- [completed] Add BBDown-compatible TV HTTP playurl mode for normal videos and PGC episodes.
- [completed] Add APP/gRPC playurl mode after the TV HTTP mode and request-spec surface is stable.
- [completed] Add a feed/list resolver abstraction for richer Bilibili page families as the first
  `0.3.0` feature slice.
- [completed] Add history record parsing on top of the feed/list resolver abstraction as the second
  `0.3.0` feature slice.
- [completed] Add following/UP page parsing on top of the feed/list resolver abstraction as the third
  `0.3.0` feature slice.
- [completed] Add recommendation page parsing on top of the feed/list resolver abstraction as the
  fourth `0.3.0` feature slice.
- [pending] Add watch-later parsing after the other feed/list inputs are in place as the final
  `0.3.0` feed/list slice.
- [pending] Add credential health checks, renewal/re-login guidance, and multi-account profile
  management for WEB cookie, generic `access_key`, and TV `tv_access_key` credentials.
- [pending] Investigate in-project `access_key` acquisition by studying the prior
  `JoeyTeng/bilibili-helper` approach, including the historical biliplus.com-based flow, before
  adding any automated login or token retrieval path.
- [pending] Continue the remaining BBDown parity backlog after the current playback/list slices:
  aria2 or multi-thread download integration, MP4Box muxing, and subtitle-to-SRT conversion.

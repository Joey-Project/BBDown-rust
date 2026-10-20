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
- [completed] Add watch-later parsing after the other feed/list inputs are in place as the final
  `0.3.0` feed/list slice.
- [completed] Publish `v0.3.0` from the completed feed/list line as a GitHub Release and
  `bbdown-core` crate.
- [completed] Add unified QR login ticket/output surfaces for existing WEB and TV login flows.
- [completed] Add credential health-check diagnostics for WEB cookie, generic `access_key`, and TV
  `tv_access_key` credentials; generic `access_key` health currently covers the intl/Bstar probe
  scope.
- [completed] Add multi-account credential profile storage while preserving default-profile
  behavior.
- [completed] Make CLI and embedding credential selection profile-aware.
- [completed] Add the core generic `access_key` acquisition flow after validating the historical
  BiliPlus QR/URL authorization approach.
- [completed] Add CLI/docs integration for generic `access_key` acquisition.
- [completed] Add append-only danmaku update workflows that can update XML sidecars and regenerate
  selected formats such as ASS for already downloaded archive records.
- [completed] Publish `v0.4.0` through the protected release candidate and promotion workflow after
  the eight-PR credential and danmaku sequence landed.
- [completed] Add the `v0.5.0` progress callback slice with a stable `DownloadProgressEvent` model,
  core `*_with_progress` download APIs, and CLI `--progress-json` output.
- [completed] Add the next `v0.5.0` progress polish slice: terminal failure/cancelled progress events,
  clearer CLI `--progress-json` schema/sample docs, embedding guidance for progress plus
  cancellation, and `DownloadReport` summary helpers for downstream UI state.
- [completed] Add cancellation-aware download execution with `DownloadCancellationToken`, graceful
  CLI `Ctrl-C`, terminal `plan_cancelled` progress events, and partial-file rollback semantics.
- [completed] Add a repo-local `$bbdown-live-e2e-fixtures` skill and aligned sample manifest entries
  with canonical real Bilibili URLs for opt-in normal, multi-page, and restricted-area live e2e
  validation.
- [completed] Add chapter metadata mux support: plan entries expose discovered chapters, ffmpeg mux
  maps them through temporary ffmetadata, and mux reports include `chapter_count`.
- [completed] Add `v0.5.0` audio language selection: expose optional audio language metadata on
  plan/playback streams, support CLI/API language selection, and keep explicit stream choices
  distinct in archive keys.
- [completed] Add `v0.5.0` AI subtitle filtering: preserve subtitle AI metadata in plans, support
  CLI/API policies for including, preferring, excluding, or selecting only AI-generated subtitles,
  and keep non-default subtitle policies distinct in archive keys.
- [completed] Finish `v0.5.0` release prep with bilingual release notes for `v0.1.0` through
  `v0.5.0`, release-archive release-note packaging, public API/rustdoc validation, deterministic
  `just ci`, and documented live-e2e restricted-proxy caveat.
- [completed] Publish `v0.5.0` through the protected release candidate and promotion workflow as a
  GitHub Release and crates.io `bbdown-core` package.
- [completed] Plan `v0.6.0` as the credential lifecycle line: renewal/reauthorization
  orchestration, health policy/reporting, profile-level status, and multi-account lifecycle UX.
- [completed] Implement the `v0.6.0` credential lifecycle sequence from
  `docs/project_journal/2026/06/2026-06-20-v0-6-credential-lifecycle-roadmap-019f16.md`, including
  persistent account switching and automatic refresh for WEB cookie and TV `tv_access_key`
  credentials.
- [completed] Add explicit normal-BV UGC collection and series membership resolution through the
  public API while preserving single-video input and download planning semantics.
- [completed] Publish `v0.6.0` through the protected release candidate and promotion workflow as a
  GitHub Release and crates.io `bbdown-core` package; both workflows succeeded and the release tag
  targets `0a94b071`. See
  `docs/project_journal/2026/06/2026-06-20-v0-6-credential-lifecycle-roadmap-019f16.md`.
- [completed] Implement opt-in overseas CDN host selection, probing, multi-CDN range downloads,
  and bundled CDN/public resolver catalogs. See
  `docs/project_journal/2026/06/2026-06-21-overseas-cdn-routing-roadmap-019f17.md`.
- [completed] Add the independent restricted-area PGC web-route API and validate its opt-in live
  proxy path. See
  `docs/project_journal/2026/09/2026-09-30-pgc-web-playurl-routes.md`.
- [completed] Run an initial same-representation single-CDN versus 2-, 4-, and 8-lane live A/B
  measurement with two repeats, recording elapsed time, preflight cost, published per-CDN bytes,
  and whole-file fallback observations. See
  `docs/project_journal/2026/06/2026-06-21-overseas-cdn-routing-roadmap-019f17.md`.
- [completed] Repeat the controlled CDN benchmark on two media sizes in two short same-morning
  windows, including a fixed single-host concurrent Range control and multi-host transfer. The
  results do not characterize all-day, multi-location, or stable performance. See
  `docs/project_journal/2026/06/2026-06-21-overseas-cdn-routing-roadmap-019f17.md`.
- [completed] Prepare the `v0.7.0` release documentation for existing opt-in CDN selection,
  probing, parallel transfer, bundled catalogs, and the independent PGC web-route API. See
  `docs/project_journal/2026/10/2026-10-05-v0-7-release-prep.md`.
- [completed] Publish `v0.7.0` through the protected release candidate and promotion workflow as a
  GitHub Release and crates.io `bbdown-core` package. See
  `docs/project_journal/2026/10/2026-10-05-v0-7-release-prep.md`.
- [completed] Add typed CDN candidate-exclusion, retry, and whole-file fallback diagnostics plus
  counters for application-consumed HTTP response-body chunks. These counts do not represent all
  wire traffic. Keep automatic routing, persistent route health, and adaptive policy as a separate
  follow-up. See
  `docs/project_journal/2026/10/2026-10-10-cdn-transfer-diagnostics.md`.
- [completed] Sign personal-space dynamic feed pages with the existing WBI key lookup and signer,
  including continuation offsets, while preserving following-feed behavior. See
  `docs/project_journal/2026/10/2026-10-10-space-dynamic-wbi-signing.md`.
- [completed] Add single-video related recommendations as one finite batch, with CLI, Rust API,
  and selection support. See
  `docs/project_journal/2026/10/2026-10-11-related-recommendations-01a12ad8.md`.
- [completed] Preserve domestic PGC season section titles and grouping while retaining flat
  episode selection and main-episode-only `Latest` behavior. See
  `docs/project_journal/2026/10/2026-10-18-pgc-season-grouping-01a0dd39.md`.
- [completed] Add bounded ordinary video keyword search with ordering and explicit selection before
  selected results enter normal metadata resolution, planning, or download. Search uses its
  configured API base only for compatible navigation/search endpoints; PGC playurl routing and
  credentials are not reused for search. See
  `docs/project_journal/2026/10/2026-10-18-video-search-01a0dd39.md`.
- [completed] Add the bounded anonymous anime/Chinese-animation PGC timetable metadata command and
  public Rust API. This slice does not add listing inputs or downloads. See
  `docs/project_journal/2026/10/2026-10-19-pgc-timetable-01a0dd39.md`.
- [completed] Add bounded anonymous PGC movie and TV category-index metadata with separate list
  and filter-values commands/API. This does not add listing inputs or downloads. See
  `docs/project_journal/2026/10/2026-10-19-pgc-season-index-01a0dd39.md`.
- [completed] Add one bounded page of anonymous popular-video metadata. See
  `docs/project_journal/2026/10/2026-10-19-popular-videos-01a0dd39.md`.
- [completed] Add bounded anonymous global ranking metadata through a separate API and CLI command.
  Category/channel lists and topic pages remain future page-family work. See
  `docs/project_journal/2026/10/2026-10-20-ranking-videos-01a0dd39.md`.
- [pending] Add category/channel lists, then topic pages at lower priority. See
  `docs/project_journal/2026/10/2026-10-11-page-family-roadmap.md`.
- [completed] Add the `0.8.0` preserving danmaku refresh API and CLI: deduplicate XML comments by
  positive IDs (falling back to complete `p` values and decoded text for invalid IDs), retain XML
  history, regenerate selected ASS from merged XML, and publish staged sidecars with the archive as
  a group, with failure preservation and core/CLI regression coverage. See
  `docs/project_journal/2026/10/2026-10-06-danmaku-preserving-refresh.md`.
- [completed] Publish `v0.8.0` through the protected release-candidate and promotion workflows as a
  GitHub Release and crates.io `bbdown-core` package. See
  `docs/project_journal/2026/10/2026-10-06-v0-8-release-prep.md`.
- [completed] Publish `v0.9.0` as a GitHub Release and crates.io `bbdown-core` package through the
  protected release-candidate and promotion workflows. See
  `docs/project_journal/2026/10/2026-10-10-v0-9-release-prep.md`.
- [completed] Record the opt-in CDN three-way comparison and deterministic local transfer baseline
  for the `0.10.0` source line, without adding a global automatic route. See
  `docs/project_journal/2026/10/2026-10-11-cdn-performance-baseline-53887b35.md`.
- [completed] Extend the opt-in live CDN sample with six valid downloads across two invocations;
  sequential was faster than either Range control in both observations. This is a small sample, not
  an automatic or adaptive routing implementation. See
  `docs/project_journal/2026/10/2026-10-11-cdn-expanded-live-measurement-01a12c87.md`.
- [pending] Consider automatic CDN routing only after broader measurements; persistent host-health
  state and adaptive chunk/concurrency settings also remain future work. See
  `docs/project_journal/2026/10/2026-10-11-cdn-performance-baseline-53887b35.md`.
- [completed] Add the explicit bounded metadata-only API for `FollowingFeed` and `SpaceDynamic`,
  preserving full `resolve`/`info` defaults and accounting for metadata requests. This does not
  validate V2 playback-candidate planning or playback. See
  `docs/project_journal/2026/10/2026-10-11-dynamic-request-and-bounded-metadata.md`.
- [completed] Initialize missing or empty device-cookie values for authenticated SpaceDynamic
  feed requests using server-issued values, a temporary request header, and clone-shared in-memory
  cache; preserve configured credentials and keep Following behavior unchanged. See
  `docs/project_journal/2026/10/2026-10-11-dynamic-request-and-bounded-metadata.md`.
- [pending] Diagnose the consumer-reported initial Space dynamic HTTP 412 and complete bounded real
  Rust playback-candidate/V2 smoke validation. The consumer's V2 resolution failed before producing
  a candidate; the focused smoke has not passed. See
  `docs/project_journal/2026/10/2026-10-11-space-dynamic-request-and-bounded-metadata-followup.md`.
- [pending] Add aria2 backend integration, MP4Box muxing, and subtitle-to-SRT conversion. BBDown
  Rust's custom concurrent HTTP Range downloader is already implemented.

---
id: 20261011-space-dynamic-request-and-bounded-metadata-followup
title: Space Dynamic Request And Bounded Metadata Follow-up
status: completed
created: 2026-10-11
updated: 2026-10-21
branch:
pr:
supersedes: []
superseded_by:
---

# Space Dynamic Request And Bounded Metadata Follow-up

## Scope And Evidence Boundary

- Keep the completed [personal-space WBI signing workstream](2026-10-10-space-dynamic-wbi-signing.md)
  and PR #93 completed. The explicit bounded API and device-cookie initialization have now landed
  in PRs #99 and #100. The BBDown-owned bounded consumer compatibility smoke is now recorded in
  the [playback acceptance journal](2026-10-20-dynamic-playback-acceptance-01a0dd39.md); extended
  consumer integration acceptance remains downstream-owned.
- The downstream consumer reported this evidence against `bbdown-core 0.9.0`, pinned to
  `e422a159cdf70c5bec72b4120e2a70e5bd749088`, using the public fixture
  `https://space.bilibili.com/441167301/dynamic`. No credentials, raw responses, private data, or
  signed URLs were supplied for this record. The publication-preparation thread made no new live
  requests. The repair thread's subsequent implementation and live evidence are recorded below.
- The historical consumer report below is evidence for its `0.9.0` source and time only. A later
  actual-core candidate check passed once on `0.11.0` source, followed by one bounded consumer
  SpaceDynamic V2/HLS compatibility smoke on the recorded consumer source with core `0.11.0`. These checks
  do not establish a general result for every dynamic input or complete downstream integration
  acceptance. These observations do not alter the deterministic workspace/release-validation gate.

## A. Initial Space Dynamic HTTP 412

**Status:** device initialization and bounded metadata API complete; actual-core candidate check and one bounded consumer V2/HLS compatibility smoke passed. Historical HTTP 412 cause remains undetermined.

### Implemented Resolution

- PR [#100](https://github.com/Joey-Project/BBDown-rust/pull/100) landed at
  `58038b3032067b82e074b7cf813f87d569be8fa5`. Authenticated SpaceDynamic requests initialize
  missing or empty device cookies from the configured API in memory, preserve nonempty values,
  and reuse a clone-shared context-bound cache. They do not update the stored credential.
- Bounded request accounting includes initialization, nav, feed, and view attempts. The repair
  thread's actual-core metadata/continuation check returned two video metadata items in five
  requests: initializer/nav/feed followed by two continuation view requests. This establishes
  metadata initialization and accounting, not successful consumer V2 planning or playback.
- See the implementation, mock regressions, and precise live-evidence boundaries in the
  [repair journal](2026-10-11-dynamic-request-and-bounded-metadata.md). This release-preparation
  thread has not reused credentials or run a new live test.

### Consumer-Supplied Observations

- At that time, against `bbdown-core 0.9.0`, a real Rust `plan_playback_input(SpaceDynamic(441167301), Page(1))` attempt received HTTP 412
  at the initial `space_feed` request. Navigation/WBI-key discovery succeeded; the result was
  `Error::Http`, not a connection, decoding, or timeout failure. No playable candidates were
  produced. Real V2 resolution also failed before candidate production. These are historical
  observations only; they do not establish the cause of the HTTP 412 or describe the later
  `0.11.0` compatibility smoke.
- A Python signed baseline at a different time reportedly returned HTTP 200, API code 0, 13 items
  including 12 archive-shaped entries, and `has_more=true`. That observation does not prove Rust
  request equivalence, a root cause, or playback acceptance.
- A bounded reqwest A/B used one fresh signed URL. Adding JSON `Accept` still produced HTTP 412;
  the B response reported code `-412`. Query-byte ordering was not proved identical to core, and
  HTTP/2 was not tested. These observations cannot isolate TLS, Referer, Cookie, or signing behavior.
- An anonymous browser request returning HTTP 200 with an empty list does not replace the
  consumer's authenticated acceptance criteria.

### BBDown Completion And Downstream Ownership

- The `0.11.0` [playback acceptance follow-up](2026-10-20-dynamic-playback-acceptance-01a0dd39.md)
  adds a direct public-API mock regression and an explicitly configured ignored live entry point.
  One actual-core legacy planning call then produced a candidate on `0.11.0` source. The call did
  not report HTTP 412, but no API request observer recorded individual request counts or statuses;
  this does not identify the historical cause or prove a general fix. Login health was not checked.
- The bounded consumer smoke is complete and recorded in the
  [playback acceptance journal](2026-10-20-dynamic-playback-acceptance-01a0dd39.md): on consumer
  source `879d852cd2a9a45e5b2f806036c77191ec7bf233`, the authenticated SpaceDynamic case passed
  (`1 passed`, Rust test duration `50.58s`) and resolved HLS playlists. This is bounded HTTP
  compatibility evidence; it does not prove native decoding or complete consumer integration.
- Extended long-download, disconnection/recovery, sustained playback, native decoding, full-fill,
  offline, deployment, and broader downstream acceptance remain the consumer maintainer's scope.
  They are not BBDown release blockers. BBDown delivery does not require reopening or merging
  consumer PR #77; its maintainer sets the consumer's acceptance and merge gates.
- Do not substitute unbounded retries, risk-control bypasses, guessed headers, or weaker candidate
  standards for acceptance. Do not request a new QR login without explicit credential-rejection
  evidence.

## B. Bounded Metadata-Only Resolution API

**Status:** completed for Following and SpaceDynamic; independent of consumer playback acceptance.

- PR [#99](https://github.com/Joey-Project/BBDown-rust/pull/99) landed at
  `4648427e81cc822a091e7402671f20dec64fcc36`. `resolve_metadata_page` provides explicit item,
  request, and source-record budgets and opaque in-memory continuation. Unsupported collection
  types are rejected; full `resolve`/`info` defaults remain unchanged.
- The cursor buffers pending records/pages, binds its input/API/cookie context, and carries no
  credentials or signing keys. Upstream lists lack a stable snapshot token, so continuation is
  best effort as the list changes. Metadata-only resolution makes no playurl or media requests.
- The [repair journal](2026-10-11-dynamic-request-and-bounded-metadata.md) records deterministic
  request-count, continuation, hard-limit, unsupported-input, and full-resolution regressions.

### Original Source Inventory

- At `e971ee5f15826ce455fd4642e1b886060876af49`, dynamic resolution uses
  `collection_info_fetch_mode` and `feed_list_info_fetch_mode`. The latter maps a valid
  `feed_list_fetch_mode(selection)` to `FeedListFetchMode::All`, so finite selectors still use the
  full information-fetch mode. This inventory is not a new live test or the HTTP 412 cause.
- The implemented API preserves full `resolve`/`info` default behavior. It is explicit and
  metadata-only; a planning API that calls playurl is not a replacement for this contract.

### Implemented Acceptance Scope

- Test upstream request counts and early stopping, including how empty, invalid, and non-video
  pages consume budgets.
- Define an opaque continuation cursor plus snapshot/list-change semantics.
- Prove that metadata-only resolution makes no playurl or media requests, and retain regression
  coverage for existing full-resolution defaults.
- Inspect the shared information-mode paths for space, favorites, history, watch-later, and
  medialists; extract only the necessary common contract without unrelated refactoring.

---
id: 20261011-space-dynamic-request-and-bounded-metadata-followup
title: Space Dynamic Request And Bounded Metadata Follow-up
status: active
created: 2026-10-11
updated: 2026-10-20
branch:
pr:
supersedes: []
superseded_by:
---

# Space Dynamic Request And Bounded Metadata Follow-up

## Scope And Evidence Boundary

- Keep the completed [personal-space WBI signing workstream](2026-10-10-space-dynamic-wbi-signing.md)
  and PR #93 completed. The explicit bounded API and device-cookie initialization have now landed
  in PRs #99 and #100. Consumer playback acceptance remains a separate pending follow-up.
- The downstream consumer reported this evidence against `bbdown-core 0.9.0`, pinned to
  `e422a159cdf70c5bec72b4120e2a70e5bd749088`, using the public fixture
  `https://space.bilibili.com/441167301/dynamic`. No credentials, raw responses, private data, or
  signed URLs were supplied for this record. The publication-preparation thread made no new live
  requests. The repair thread's subsequent implementation and live evidence are recorded below.
- The consumer's focused dynamic playback gate has no successful acceptance evidence. This does not make the deterministic
  workspace/release-validation gate fail, nor establish a general failure for every dynamic input.

## A. Initial Space Dynamic HTTP 412

**Status:** device initialization implemented; consumer V2 playback acceptance pending.

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

- A real Rust `plan_playback_input(SpaceDynamic(441167301), Page(1))` attempt received HTTP 412
  at the initial `space_feed` request. Navigation/WBI-key discovery succeeded; the result was
  `Error::Http`, not a connection, decoding, or timeout failure. No playable candidates were
  produced. Real V2 resolution also failed before candidate production, and the focused smoke has
  not passed; long-download, HLS, and native-playback tests have not started.
- A Python signed baseline at a different time reportedly returned HTTP 200, API code 0, 13 items
  including 12 archive-shaped entries, and `has_more=true`. That observation does not prove Rust
  request equivalence, a root cause, or playback acceptance.
- A bounded reqwest A/B used one fresh signed URL. Adding JSON `Accept` still produced HTTP 412;
  the B response reported code `-412`. Query-byte ordering was not proved identical to core, and
  HTTP/2 was not tested. These observations cannot isolate TLS, Referer, Cookie, or signing behavior.
- An anonymous browser request returning HTTP 200 with an empty list does not replace the
  consumer's authenticated acceptance criteria.

### Remaining Consumer Acceptance

- The `0.11.0` [playback acceptance follow-up](2026-10-20-dynamic-playback-acceptance-01a0dd39.md)
  adds a direct public-API mock regression and an explicitly configured ignored live entry point.
  Its current evidence does not establish authenticated live candidate or consumer V2 acceptance.
- Obtain playable candidates meeting the consumer's existing requirements through bounded real
  Rust metadata/playback resolution, then pass the real V2 focused smoke before resuming long
  download, disconnection/recovery, HLS, or native-playback validation.
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

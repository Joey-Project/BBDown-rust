---
id: 20261011-space-dynamic-request-and-bounded-metadata-followup
title: Space Dynamic Request And Bounded Metadata Follow-up
status: active
created: 2026-10-11
updated: 2026-10-11
branch:
pr:
supersedes: []
superseded_by:
---

# Space Dynamic Request And Bounded Metadata Follow-up

## Scope And Evidence Boundary

- Keep the completed [personal-space WBI signing workstream](2026-10-10-space-dynamic-wbi-signing.md)
  and PR #93 completed. The two items below are separate pending follow-ups, not implemented
  `v0.10.0` features.
- The downstream consumer reported this evidence against `bbdown-core 0.9.0`, pinned to
  `e422a159cdf70c5bec72b4120e2a70e5bd749088`, using the public fixture
  `https://space.bilibili.com/441167301/dynamic`. No credentials, raw responses, private data, or
  signed URLs were supplied for this record. The publication-preparation thread made no new live
  requests and has not established the cause.
- The consumer's focused dynamic live gate is blocked. This does not make the deterministic
  workspace/release-validation gate fail, nor establish a general failure for every dynamic input.

## A. Initial Space Dynamic HTTP 412

**Status:** pending diagnosis; consumer dynamic live gate blocked.

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

### Acceptance Conditions

- Locate and fix the failure under legitimate official request conditions, with focused mock or
  request-contract regression coverage for the established cause.
- Obtain playable candidates meeting the consumer's existing requirements through bounded real
  Rust metadata/playback resolution, then pass the real V2 focused smoke before resuming long
  download, disconnection/recovery, HLS, or native-playback validation.
- Do not substitute unbounded retries, risk-control bypasses, guessed headers, or weaker candidate
  standards for acceptance. Do not request a new QR login without explicit credential-rejection
  evidence.

## B. Bounded Metadata-Only Resolution API

**Status:** pending API design and implementation; independent of the initial HTTP 412.

### Current Source Boundary

- At `e971ee5f15826ce455fd4642e1b886060876af49`, dynamic resolution uses
  `collection_info_fetch_mode` and `feed_list_info_fetch_mode`. The latter maps a valid
  `feed_list_fetch_mode(selection)` to `FeedListFetchMode::All`, so finite selectors still use the
  full information-fetch mode. This inventory is not a new live test or the HTTP 412 cause.
- Preserve the current full `resolve`/`info` default behavior. Add an explicit metadata-only API
  for bounded consumer candidate windows or list pages; a planning API that calls playurl is not
  a replacement for that contract.

### Acceptance Conditions

- Test upstream request counts and early stopping, including how empty, invalid, and non-video
  pages consume budgets.
- Define an opaque continuation cursor plus snapshot/list-change semantics.
- Prove that metadata-only resolution makes no playurl or media requests, and retain regression
  coverage for existing full-resolution defaults.
- Inspect the shared information-mode paths for space, favorites, history, watch-later, and
  medialists; extract only the necessary common contract without unrelated refactoring.

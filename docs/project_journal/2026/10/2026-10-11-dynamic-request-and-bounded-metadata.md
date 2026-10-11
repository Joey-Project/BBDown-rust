---
id: 20261011-dynamic-request-and-bounded-metadata
title: Dynamic Request Diagnosis And Bounded Metadata
status: active
created: 2026-10-11
updated: 2026-10-11
branch: wip/dynamic-bounded-metadata
pr:
supersedes: []
superseded_by:
---

# Dynamic Request Diagnosis And Bounded Metadata

## Scope

- Follow up on the completed [space dynamic signing workstream](2026-10-10-space-dynamic-wbi-signing.md)
  without changing its completed status.
- Base: `e971ee5f15826ce455fd4642e1b886060876af49`. Published `0.9.0` already contains
  WBI signing; missing signing is not an established explanation for the reported HTTP 412.
- Release preparation is paused in a separate worktree. This repair does not change versions,
  create release candidates, publish packages, or update release-wide documentation.

## A. Initial Dynamic Request

**Status:** diagnosis pending; authenticated consumer acceptance remains blocked.

- This repair session has not sent authenticated requests. A scoped request to reuse the
  consumer's `family-room-new` Web-cookie profile against official Bilibili APIs is awaiting
  Joey's authorization; the earlier episode/proxy authorization does not cover this fixture.

- Consumer fixture: `https://space.bilibili.com/441167301/dynamic`, against `0.9.0`
  commit `e422a159cdf70c5bec72b4120e2a70e5bd749088`.
- The consumer reported successful nav/key discovery followed by HTTP 412 at the initial
  dynamic feed request. Actual V2 resolution failed before candidate production; focused smoke
  has not passed. Long download, HLS, and native playback tests have not started.
- Its earlier Python success was at a different time. Adding JSON Accept to a separate reqwest
  control did not resolve 412. Neither result isolates a cause or proves playable acceptance.
- Source inspection confirms signing on initial and continuation space requests. Nav/key
  success alone does not prove login: the current key lookup does not inspect `isLogin`.
- Another session reported anonymous public-video HTTP 412 on this machine. That environmental
  observation does not establish the same cause for authenticated dynamic requests.
- Acceptance requires bounded actual Rust requests yielding playable candidates and the
  consumer's actual V2 focused smoke. Mock success or an empty anonymous feed is insufficient.

## B. Explicit Bounded Metadata API

**Status:** completed for the explicit bounded metadata API; independent of A.

- Preserve full `resolve`/`info` collection semantics. `Selection::Page(n)` remains a result
  item index, not an upstream page number.
- Add a separate metadata-only operation for following and personal-space dynamic feeds.
  Output limits and upstream request/record budgets are explicit; nav, feed, and view calls
  all count. No tags, playurl, or media requests belong to this operation.
- Continuation must retain unconsumed source records and video pages, bind to the input and
  API endpoint, and fail on malformed or cyclic upstream pagination. It stores no credentials,
  signing keys, or signed request URLs.
- The upstream has no stable snapshot token. Continuation is best effort when the source list
  changes; it must not promise a frozen snapshot.
- `resolve_metadata_page` returns bounded items, a collection summary, request/source counts,
  a stop reason, and an opaque in-memory cursor. Cursor state is bound to input, API endpoint,
  and cookie context by hashes, with a redacted `Debug` implementation.
- Requests disable redirects and enforce a 1 MiB streamed JSON-body cap. Cursor source records,
  expanded video pages, and remembered identities are capped at 4096; offset history is capped
  at 128. Returned item string payloads have a 4 MiB aggregate cap.
- Buffered records and unconsumed video pages survive continuation without another feed request.
  Space feeds obtain a WBI key lazily; pending video details can resume without another nav call.
- Shared-path audit: space videos, favorites, collection/series, history, watch-later, and
  medialists have differing pagination and nested-detail costs. This slice does not silently
  route unsupported inputs through full resolution.

## Validation

- Passed: `cargo test --locked --offline -p bbdown-core` with Rust/Cargo 1.95.0: 594 unit
  tests, 13 integration tests, and one public API integration test; one integration test is
  ignored and there are no crate doctests. Loopback mock servers ran outside the sandbox;
  this test run did not use real credentials or Bilibili endpoints.
- New mock cases cover exact nav/feed/view counts, pending and buffered continuation,
  invalid/non-video/duplicate filtering, empty pages, malformed/cyclic offsets, cursor binding,
  unsupported inputs, hard limits, oversized responses, redirect denial, and absent media calls.
- The existing full-resolution/selected-plan regression passed.
- English and Simplified Chinese embedding guides are synchronized, including the candidate
  loading diagram and the distinction between source-only API additions and published 0.9.0.
- Passed: `cargo clippy --locked --offline -p bbdown-core --all-targets -- -D warnings`
  and `cargo fmt --all -- --check` on the final source.
- Project journal validation and `git diff --check` passed.
- No live dynamic acceptance is claimed by these mock results. PR checks and current-head
  review evidence belong to the PR rather than changing A's pending acceptance state.
- Delivery uses GitHub Codex PR review only; no local reviewer.

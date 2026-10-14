---
id: 20261011-dynamic-request-and-bounded-metadata
title: Dynamic Request Diagnosis And Bounded Metadata
status: active
created: 2026-10-11
updated: 2026-10-12
branch: wip/dynamic-request-diagnostics
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
- The bounded API landed in [PR #99](https://github.com/Joey-Project/BBDown-rust/pull/99),
  squash commit `4648427e81cc822a091e7402671f20dec64fcc36`. Follow-up diagnosis starts
  from that merged master state.
- Release preparation is paused in a separate worktree. This repair does not change versions,
  create release candidates, publish packages, or update release-wide documentation.

## A. Initial Dynamic Request

**Status:** library initialization implemented; authenticated consumer acceptance remains pending.

The automatic-production live metadata and continuation check passed after correcting the new
Cookie pair separator to `; `. The original profile was loaded read-only; no external initializer
or credential-store update was used. Full consumer playback acceptance is still separate.

- Joey explicitly authorized real-cookie requests for this diagnosis. The selected existing
  profile was loaded read-only; credential values were neither printed nor retained. Requests
  used only official `api.bilibili.com`, without proxies, playurl requests, or media downloads.
- On 2026-10-12, the real bounded Rust core operation returned HTTP 412 without a completed
  metadata page. Its error output reports a six-request budget, not the actual number used.
- A separate three-request protocol control returned nav HTTP/2 200, provider code 0,
  and `isLogin=true`. Both its HTTP/2 and HTTP/1.1 feed requests returned HTTP 412 with
  non-JSON responses. This batch was bounded by nine API attempts in total.
- The protocol control did not establish a source-code fix. Source comparison found no proven
  semantic WBI difference; previous Python success was not contemporaneous.
- A second three-request comparison ran within `2026-10-12 08:30:34` to `08:30:57 UTC`.
  Its HTTP helpers used the same signed URL and explicit headers. Python independently
  recomputed matching signature and query bytes, while both reqwest and urllib feed calls
  returned HTTP 412 / provider code -412. Nav again reported `isLogin=true`.
- This helper comparison is not actual-core acceptance or proof of server-side signature
  validity. It did not isolate a source-code cause; no cause is assigned to IP, TLS, or
  credential expiry.
- A third three-request comparison ran within `2026-10-12 10:26:56` to `10:27:09 UTC`.
  Nav reported `isLogin=true`. Both the current fields and the public webpage's observed
  static fields returned HTTP 412 / provider code -412. The second case changed Referer,
  features, empty offset, timezone offset, and web location together; it excluded device,
  fingerprint, locale, CAPTCHA, and anti-risk values. It does not isolate one field.
- Joey confirmed that the normal browser, using the same account on the current network,
  displays all dynamics. This manual observation has not supplied a successful API request
  or its cookie-name set for comparison. A separate anonymous browser observation returned
  HTTP 200 / code 0 with zero items, which does not establish playable acceptance.
- Source inspection found that saved Web cookies are sent as provided. The client has no
  cookie jar or independent Web initialization step, and Web QR generation/interim polling
  cookies are not carried into the final credential. A read-only, zero-request diagnostic
  confirmed that the selected profile has `SESSDATA`, `bili_jct`, `DedeUserID`, and
  `DedeUserID__ckMd5`, but lacks `buvid3`, `buvid4`, `b_nut`, `b_lsid`, and `buvid_fp`.
  This establishes saved-cookie presence only, not the successful browser's cookie set or
  a cause for 412. No media tests have run.
- A four-request cookie-only control ran within `2026-10-12 10:44:25` to `10:44:39 UTC`.
  Its baseline feed returned HTTP 412 / code -412. An anonymous official
  `/x/frontend/finger/spi` request returned valid `data.b_3` and `data.b_4`; appending only
  the missing `buvid3` and `buvid4` in memory changed the feed result to HTTP 200 / code 0,
  with 13 dynamic records including 12 video records. Existing cookie values were preserved.
  The public RSSHub source uses this server-issued contract; this does not prove the
  browser's complete initialization sequence.
- The actual `BiliClient::resolve_metadata_page` also succeeded after the same in-memory
  preparation, using unchanged production headers and query fields: two video metadata
  items, three source records examined, four core requests plus one anonymous initializer.
  It stopped at the item limit. This validates the metadata diagnosis; the production
  initialization path and the consumer's V2 smoke have separate acceptance evidence.
- Authenticated SpaceDynamic now prepares missing or empty device cookies before fetching a new feed.
  It requests the configured API's initializer anonymously, validates both server values,
  and preserves existing nonempty cookie values. Empty or whitespace-only device pairs are removed
  from the temporary feed header before missing values are added, avoiding duplicate-name shadowing.
  When both device names already have nonempty values, empty duplicates are removed without SPI.
  Only the feed's temporary header is prepared;
  credentials and configuration are unchanged. Following and other collections keep their
  existing behavior.
- A clone-shared, context-bound in-memory cache avoids repeated initialization. Failed
  initialization is not cached; values and context are redacted from Debug output. Legacy
  resolution uses the same preparation without changing full collection or page selection.
- The bounded operation counts initializer, nav, feed, and view attempts. Reservation occurs
  while holding the initialization cache lock. Each call starts without a WBI key: its first
  new Space feed needs nav plus feed, and a cold initializer adds a third request. Lower budgets
  stop that group before requesting initializer, nav, or feed. Only later feeds within the same
  call reuse its local key. The cursor stores no signing keys; buffered output does not force
  initialization.
- The initializer disables redirects and caps JSON at 1 MiB on both bounded and legacy paths.
  No fingerprint values are fabricated, no CAPTCHA or anti-risk values are replayed, and no
  credential store writes or media requests are part of this fix.
- During implementation, the automatic path initially returned HTTP 412 at the Space feed.
  Its appended pairs lacked the standard separator space used by the successful control.
  Correcting only that delimiter restored the automatic path; nav and view need no new override.
- The final live check used two actual-core calls: the first exhausted its three-request budget
  after initializer, nav, and feed; continuation used two view requests and returned two video
  metadata items. Total usage was five requests and three examined source records. This validates
  production initialization, accurate accounting, and continuation; it does not validate consumer
  V2 planning, downloads, HLS, or native playback.

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

- Passed: `cargo test --locked --offline -p bbdown-core` with Rust/Cargo 1.95.0: 600 unit
  tests and 15 integration tests (13 CDN, one legacy dynamic, one public API); one integration test is
  ignored and there are no crate doctests. Loopback mock servers ran outside the sandbox;
  this test run did not use real credentials or Bilibili endpoints.
- New mock cases cover exact nav/feed/view counts, pending and buffered continuation,
  invalid/non-video/duplicate filtering, empty pages, malformed/cyclic offsets, cursor binding,
  unsupported inputs, hard limits, oversized responses, redirect denial, and absent media calls.
- The existing full-resolution/selected-plan regression passed.
- Initialization regressions cover precise cold/warm request budgets, clone cache sharing,
  existing-name precedence, invalid values and uncached failure retry, redirects, and oversized
  JSON. A public-API mock verifies legacy SpaceDynamic multi-page selection and unchanged
  configured credentials. These ordinary Cargo tests run in CI without live credentials.
- English and Simplified Chinese embedding guides are synchronized, including the candidate
  loading diagram and the distinction between source-only API additions and published 0.9.0.
- Passed: `cargo clippy --locked --offline -p bbdown-core --all-targets -- -D warnings`
  and `cargo fmt --all -- --check` on the API implementation.
- Project journal validation and `git diff --check` passed.
- No live dynamic acceptance is claimed by these mock results. PR checks and current-head
  review evidence belong to the PR rather than changing A's pending acceptance state.
- Delivery uses GitHub Codex PR review only; no local reviewer.

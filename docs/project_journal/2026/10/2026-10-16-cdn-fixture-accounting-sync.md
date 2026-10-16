---
id: 20261016-cdn-fixture-accounting-sync
title: CDN Fixture Accounting Synchronization
status: completed
created: 2026-10-16
updated: 2026-10-16
branch: wip/cdn-fixture-accounting-sync
pr:
supersedes: []
superseded_by:
---

# CDN Fixture Accounting Synchronization

## Scope

- Repair the deterministic CDN fixture's observation boundary after the first `0.10.0` RC verification exposed a race between socket writes and server-side byte accounting.
- Keep strict observer/server byte equality and cumulative budget assertions. The repair belongs to the local test fixture, not production download or budget policy.

## Evidence

- Source: `addbb224072cfd6bdde60b23d66910ff07d11007`, including release preparation PR #101 and compressed danmaku repair PR #103.
- [RC run 38971931266](https://github.com/Joey-Project/BBDown-rust/actions/runs/38971931266) failed at `Unit and e2e tests`; artifact construction and RC tag creation were skipped.
- `scenarios::cumulative_budget_counts_restarted_request_ids_across_groups` observed a completed 65,536-byte `AutomaticProbe` on the client while the fixture had recorded its request but still reported zero served bytes.
- The fixture writes response bytes to the socket before adding them to its observation counters. The client can finish consuming those bytes before the handler thread commits its counter update; an immediate snapshot or reset can therefore race with that bookkeeping.

## Implementation

- Track accepted request handlers through an RAII completion guard. Fixture snapshots and resets wait for active handlers to finish, with an eight-second deadline and explicit timeout diagnostics.
- Use a condition variable with independently running server threads; a current-thread Tokio test does not have to schedule a server task to release the barrier. Existing socket timeouts still bound cancelled handler I/O.
- Add a deterministic regression that pauses a handler after its socket write and before accounting, then proves a snapshot waits at that incomplete accounting boundary before checking strict client/server byte equality.
- Change only the test fixture and its regression; production code and cumulative budget assertions are unchanged.

## Validation

- With the repository-selected installed stable Rust/Cargo 1.95.0, the original cumulative-budget regression and the new forced accounting-window regression each passed individually.
- `cargo test --locked -p bbdown-core --test cdn_benchmark`: 21 passed, zero failed, one ignored live test.
- `cargo fmt --all -- --check` and the changed-file `git diff --check` passed.
- Final `just ci` passed with exit zero in 99.7 seconds: formatting, strict all-target Clippy, the Rust 1.95.0 MSRV check, workspace tests (872 passed, zero failed, three ignored), the separate CLI e2e suite (159 passed), and the core package dry run.
- The dry run did not upload a crate. Cargo reported the existing locked `spin 0.9.8` yanked warning.
- Earlier attempts stopped at a sandbox build-cache lock restriction and then three Clippy findings in the fixture repair. The lint findings were corrected before the successful full rerun; those attempts are not counted as passed gates.
- The final bounded runner used a ten-minute deadline and a 16 MiB log ceiling; neither limit was reached. The failed RC run is not counted as successful repair validation.

## Publication Follow-up

- After the repair passes local checks and GitHub Codex review and merges, create a fresh RC from the repaired master with version input `0.10.0`.
- Verify its source and artifacts before promotion, and honor the configured release environment approval gates.

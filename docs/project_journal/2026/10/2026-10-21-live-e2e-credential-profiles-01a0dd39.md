---
id: 2026-10-21-live-e2e-credential-profiles-01a0dd39
title: Live E2E Credential Profile Support
status: completed
created: 2026-10-21
updated: 2026-10-21
branch: wip/live-e2e-credential-profiles
base: 66be2e1365bf4d48549032fa7d7686f0449f442d
pr:
supersedes: []
superseded_by:
---

# Live E2E Credential Profile Support

## Goal

Allow the opt-in live E2E manifest to select credentials from an existing versioned
BBDown CredentialStore while retaining compatibility with legacy flat BBDown and
Telegram Video Downloader credential files. This closes a test-harness compatibility
gap identified after the earlier playback acceptance work; it does not change
production credential behavior.

## Contract

- `credential_file` and `credential_profile` may be set on the manifest and on each
  case. A case value overrides the corresponding manifest value independently.
- An omitted profile uses the source store's actual default profile. A profile without a
  credential file, an empty or unknown profile, and an empty or malformed explicit
  store must fail. Cases with no explicit credential source or profile may remain
  anonymous.
- Optional `access_key_file` remains supported; a case-level file overrides the
  manifest-level file, and its non-empty trimmed value overrides the selected
  profile's generic access key.
- The PGC test's credential-file environment override replaces only the source file;
  profile selection still follows case, then manifest, then store default.
- The source credential store is read-only. The harness writes the selected cookie,
  generic access key, and TV access key into a temporary per-case credential file;
  other profiles and lifecycle metadata are not copied.

No real credentials or live requests are used for this change. The selected profile
is materialized only for the existing local test harness; this is not a guarantee
that the source store receives no writes from unrelated software.

## Validation

`cargo test -p bbdown-cli --test live_e2e --locked --offline` passed (13 passed,
0 failed, 2 ignored), and `cargo fmt --all -- --check` passed. The first `just ci` attempt stopped at Clippy
on five test-only lint findings. After those test-only fixes, the second full CI
passed in 49.386 seconds: 983 workspace tests passed, 0 failed, and 4 were ignored.
The 45 tracked Rust inputs remained hash-stable across the successful run.
The separate CLI E2E rerun passed 197 tests and is not included in that workspace
count. The MSRV 1.95 check and `bbdown-core` publish dry run passed; no package was
published. Existing warnings remain for the already-published `bbdown-core 0.11.0`
and yanked `spin 0.9.8`; dependencies and versions were unchanged. The tracked
example JSON parses; the project-journal validator and scoped whitespace check pass.
No live request or real credential was used for this change.

## Boundaries

Live E2E remains opt-in and depends on network, account, token, and regional access.
This journal records harness credential selection only; it does not claim live
playback, credential refresh, download, media transfer, or downstream V2 validation.

## Attribution

Implementation and docs: GPT-6 Luna High. Coordination: GPT-6.1 Sol.

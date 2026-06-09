---
id: 20260608-019ea7-release-automation
title: Release Automation
status: completed
created: 2026-06-08
updated: 2026-06-08
branch: wip/release-automation
pr:
supersedes:
  - 20260607-019e9eab-release-packaging
  - 20260607-019e9eab-crate-publish-readiness
superseded_by:
---

# Release Automation

## Summary

- Scope is the 0.1.0 release governance flow after CODEOWNERS landed.
- The release path is now RC-first: validate the repository default branch, create a protected RC
  tag, then promote the latest RC tag for a version to the final GitHub Release and `bbdown-core`
  crates.io publication.

## Current State

- `.github/workflows/release.yml` is now `Release Artifacts`, a reusable/manual artifact builder. It
  no longer publishes on arbitrary `v*` tag pushes.
- `.github/workflows/release-verify.yml` is a reusable verification workflow shared by RC creation
  and RC promotion, so formatter, clippy, declared MSRV check, tests, and crates.io dry-run coverage
  stay in one place.
- `.github/workflows/create-release-candidate.yml` validates the repository default branch, runs
  release verification, builds release archives, auto-selects the next `vX.Y.Z-rc.N` tag, and
  creates that annotated tag through the release GitHub App inside the `release-candidate`
  environment.
- `.github/workflows/promote-release-candidate.yml` must be dispatched from an RC tag. It reruns the
  shared release verification, rebuilds final artifacts, verifies the selected RC is the latest for
  that version, creates the final annotated `vX.Y.Z` tag, publishes the GitHub Release inside the
  `production-release` environment, and publishes `bbdown-core` inside the `crates-io` environment.
- Shared release scripts live under `scripts/release/`: `common.sh` centralizes GitHub tag/release
  queries, latest RC calculation, and Cargo workspace version extraction outside YAML, while
  `package-release.sh` and `package-release.ps1` package Unix and Windows release archives.
- RC and promotion validation both require `bbdown-core` and `bbdown-cli` Cargo versions to match
  the requested release version so GitHub Release archive names, CLI package metadata, and the
  crates.io package do not drift.
- RC creation is serialized per requested release version, and promotion is serialized per final
  release version with the same concurrency group, so concurrent manual runs cannot race the
  automatic RC number, create a later RC while an older RC is being promoted, or duplicate the same
  final release.
- RC creation rejects versions that already have a final tag or GitHub Release, so maintainers cannot
  create dead-end RC tags after a version has already shipped. The create-tag job repeats that check
  immediately before writing the RC tag because artifact builds and environment approval can add a
  race window after validation.
- Promotion workflow runs are now serialized per final release version through the manual `version`
  input, and the selected RC is rechecked as latest immediately before final tag and GitHub Release
  creation.
- Release archives now include the English and Simplified Chinese release runbooks alongside the
  existing README, user, embedding, architecture, and license files.
- CI and release workflows now use runner-provided `rustup` with the floating stable channel from
  `rust-toolchain.toml`; third-party Rust setup and cache actions are intentionally not used. They
  also run `cargo +1.95.0 check --workspace --locked` to protect the declared crate `rust-version`.
- `docs/release.md` and `docs/release.zh-CN.md` document required environments, secrets, tag
  rulesets, RC creation, promotion, and failure recovery.
- Offline frozen review found that promotion recovery would fail if final tag creation succeeded but
  GitHub Release creation failed. The promotion workflow now reuses an existing final tag when it
  points at the same RC target commit and the release is still missing.
- Follow-up offline review found that interrupted `gh release create` runs can leave a draft GitHub
  Release behind. Promotion now deletes draft releases before retrying, and it reuses already
  published releases only when all expected assets are present.
- Follow-up independent review found that generated final release notes could compare against the
  just-created RC tag. Promotion now passes the previous non-RC final release tag as the generated
  notes start tag when one exists.
- Follow-up independent review found a cross-workflow race between RC creation and promotion. Both
  workflows now share the same version-scoped concurrency group.
- Follow-up independent review found a crates.io publish recovery gap after an upload is accepted but
  the job is marked failed. The publish step now checks the exact `bbdown-core` version first and
  treats an already-published matching version as recovered success.
- Follow-up independent review found draft GitHub Releases were not reliably discovered through the
  published-release tag endpoint. Promotion now lists releases by `tag_name` with the release GitHub
  App token before deciding whether to delete a draft or reuse a published release.
- Follow-up independent review found published release reuse needed stronger asset validation.
  Promotion now requires matching asset names, `uploaded` states, byte sizes, and SHA-256 digests
  and rejects unexpected extra assets before it treats an existing GitHub Release as reusable.
- Final frozen review found the stricter digest check required reproducible package archives for
  full promotion reruns. Unix tarballs and Windows zip files now normalize archive metadata so the
  same target commit rebuilds to stable package checksums.
- Follow-up review noted that published-release reuse must not accept stale assets. Reuse now
  validates the already-published assets and checksum sidecars, validates the rebuilt `dist`
  sidecars, and requires the published archive checksums to match the rebuilt archive checksums.
- Final independent review found checksum sidecars must be tied to the archive currently being
  checked. Promotion now rejects sidecars whose recorded filename does not match the archive asset.

## Validation

- Workflow lint: `actionlint .github/workflows/*.yml`.
- Shell syntax: `bash -n scripts/release/package-release.sh`.
- Shell syntax: `bash -n scripts/release/common.sh`.
- Shell lint: `shellcheck scripts/release/package-release.sh scripts/release/common.sh`.
- Release build: `cargo build -p bbdown-cli --bin bbdown --release --locked`.
- Local release package smoke:
  `scripts/release/package-release.sh target/release/bbdown bbdown-local-smoke .codex-tmp/release-package-smoke`.
- Local package content check: `tar -tzf .codex-tmp/release-package-smoke/bbdown-local-smoke.tar.gz`.
- Local package checksum check:
  `shasum -a 256 -c bbdown-local-smoke.tar.gz.sha256` from the package output directory.
- Local default gate: `just ci`.
- Offline frozen review:
  `isolated_review stateful start --entrypoint codex-readonly --base-ref master --head-ref HEAD`.
- Independent Codex PR review found the automatic RC numbering concurrency window; the workflow now
  uses GitHub Actions concurrency groups to serialize runs by release version.
- GitHub Codex review flagged a hard-coded release branch check; the workflow now checks the
  repository default branch, which is currently `master`.
- Independent Codex PR review found stale RC promotion and missing MSRV coverage risks; promotion now
  rejects non-latest RC tags before publication and CI/release gates check the declared MSRV.
- Offline frozen review found that RC creation still allowed already-shipped versions; RC validation
  now rejects existing final tags and GitHub Releases before creating another candidate.
- Follow-up reviews found fail-open process substitutions around `gh api` tag enumeration; the
  workflows now capture those API results before looping so query failures stop the run.
- Offline frozen review then confirmed GitHub `matching-refs` returns 404 for an empty first-RC tag
  set; the workflows now treat that specific 404 as an empty tag list while keeping other API errors
  fail-closed.
- Follow-up offline frozen review found interrupted GitHub Release creation was not fully retryable;
  the workflow now deletes draft releases and verifies complete assets before reusing a published
  release.
- Follow-up independent Codex PR review found generated release notes could use the RC tag as their
  comparison base; the workflow now uses the previous non-RC final release tag when available.
- Follow-up independent Codex PR review found RC creation and promotion needed a shared cross-workflow
  lock; both workflows now use the same version-scoped concurrency group.
- Follow-up independent Codex PR review found crates.io publication was not idempotent after an
  accepted upload with a failed job result; the publish step now has an idempotent path for existing
  exact `bbdown-core` versions.
- Follow-up independent Codex PR review found draft release detection and published asset reuse were
  too weak; the workflow now discovers drafts via release listing and checks asset state, size, and
  digest, and rejects unexpected extra assets, before reuse.
- Final frozen review found non-reproducible archives would break full promotion reruns after a
  successful GitHub Release and failed crates.io publish; package scripts now create deterministic
  archives before digest-level release reuse.
- Follow-up independent review found the Windows ZIP timestamp needed to use the ZIP-supported
  minimum date and that floating stable Rust limits full-rerun byte identity guarantees; the workflow
  now verifies published release assets through their sidecar checksums before continuing to crates.io.
- Final independent review found sidecar checksum success alone could validate the wrong filename;
  the workflow now requires each sidecar to name its paired archive before running `sha256sum -c`.
- Follow-up frozen review found crates.io version-only recovery could accept a package published
  from a different commit; the crate publish job now repackages the selected RC source and requires
  the local `.crate` SHA256 to match the crates.io checksum before treating an existing version as
  recovered success.
- GitHub Codex review found release asset reuse could accept stale but self-consistent archives; the
  promotion workflow now compares each published archive checksum against the rebuilt `dist`
  checksum before continuing to crates.io.
- Follow-up refactor split repeated validation into `.github/workflows/release-verify.yml` and
  repeated shell helpers into `scripts/release/common.sh`, with CODEOWNERS coverage for both new
  surfaces.
- Follow-up independent review found non-locked lint/test steps could update `Cargo.lock` before the
  later locked checks. CI, release verification, local tests, CLI e2e, and live e2e now run Cargo
  validation with `--locked`.
- Follow-up independent review found GitHub Release reuse could accept an existing final release
  marked as a prerelease. Promotion now rejects prerelease final releases instead of treating them as
  recovered success.
- Follow-up independent review found the shared Cargo version metadata helper could still resolve
  dependencies before the reusable locked verification job. The helper now calls
  `cargo metadata --locked`.
- Follow-up frozen review found crates.io checksum mismatch recovery would fail only after creating
  the final tag and GitHub Release. Promotion now preflights the crates.io package state before
  publishing the GitHub Release and repeats the same helper check in the crate publish job.
- Follow-up independent review found crates.io recovery could accept a yanked version with a matching
  checksum. The shared crates.io helper now rejects yanked exact-version matches.
- Follow-up independent review found the architecture docs still documented the old non-locked
  formatter/linter/test commands. The English and Simplified Chinese architecture docs now list the
  locked CI and local e2e commands used by the workflows.
- Follow-up independent review found the release runbook only told maintainers to update the
  `bbdown-core` package version even though RC validation checks both workspace package versions.
  The English and Simplified Chinese release runbooks now call out both `bbdown-core` and
  `bbdown-cli`.
- Follow-up independent review found the release write jobs granted the default `GITHUB_TOKEN`
  `contents: write` even though tag and release writes use the release GitHub App token. Those jobs
  now keep the default token read-only so the App remains the only intended write actor.
- Follow-up maintainability cleanup moved both release packaging scripts into `scripts/release/`, so
  all release shell and PowerShell helpers live under the same CODEOWNERS-protected subtree.
- Local package smoke after the move found both packaging scripts still derived the repository root
  from the old `scripts/` location. `package-release.sh` and `package-release.ps1` now resolve the
  repository root from `scripts/release/`.
- Follow-up independent review found the validation journal used a multi-file `bash -n` command even
  though Bash parses only the first script argument. The journal now records the two shell syntax
  checks as separate commands.

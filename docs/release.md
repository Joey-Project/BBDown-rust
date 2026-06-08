[ English | [简体中文](release.zh-CN.md) ]

# Release Runbook

This project releases in two phases:

1. A manually approved GitHub Actions run validates the repository default branch, builds release
   artifacts, and creates a release candidate tag such as `v0.1.0-rc.1`.
2. A second manually approved run is started from that RC tag. It rebuilds final artifacts, creates
   the final tag such as `v0.1.0`, publishes the GitHub Release, and publishes the `bbdown-core`
   crate to crates.io.

The reusable release artifact workflow can still be run manually for archive previews, but it does
not publish tags, GitHub Releases, or crates.
RC creation and RC promotion share the same `Release Verification` reusable workflow for formatter,
lint, declared MSRV, tests, and crates.io dry-run validation.

## GitHub Setup

Configure these environments:

- `release-candidate`: allow deployment from the repository default branch only, currently
  `master`. Store
  `RELEASE_GITHUB_APP_ID` and `RELEASE_GITHUB_APP_PRIVATE_KEY` here, or make equivalent repository
  secrets available to this environment.
- `production-release`: allow deployment from tags matching `v*-rc.*` only. Store the same release
  GitHub App secrets here.
- `crates-io`: allow deployment from tags matching `v*-rc.*` only. Store
  `CARGO_REGISTRY_TOKEN` here.

Configure the release GitHub App with repository metadata read and contents write permissions. The
App should be the only non-human actor allowed by the release tag rulesets.

Recommended rulesets:

- Repository default branch, currently `master`: require pull requests, require status checks `Rust`
  and `codex/review-gate`, require Code Owner review, and keep the general required approval count at
  `0` if ordinary PRs should not need human approval.
- RC tags: target `v*-rc.*`; restrict creation, updates, and deletion; allow creation only through
  the release GitHub App.
- Final release tags: target `v*` and exclude `v*-rc.*` when the UI supports excludes; otherwise use
  a separate final-tag pattern that does not match RC tags. Restrict creation, updates, and deletion;
  allow creation only through the release GitHub App.

## Create An RC Tag

1. Ensure `crates/bbdown/Cargo.toml` has the final crate version, for example `0.1.0`.
2. Ensure the intended branch is merged to the repository default branch, currently `master`.
3. In GitHub Actions, run `Create Release Candidate` from the repository default branch.
4. Enter `version` without a leading `v`, for example `0.1.0`. The workflow chooses the next
   available RC number automatically.
5. Approve the `release-candidate` environment deployment.

The workflow checks that it is running from the repository default branch, validates the
`bbdown-core` and `bbdown-cli` Cargo versions, serializes all RC creation and promotion runs for the
same release version, computes the next RC number, calls the shared release verification workflow,
rejects versions that already have a final tag or GitHub Release, builds all release archives, then
rechecks the final tag and GitHub Release state immediately before writing, and creates the
annotated RC tag.

## Promote An RC

1. In GitHub Actions, open `Promote Release Candidate`.
2. Use the branch/tag selector to select the latest RC tag for that version, for example
   `v0.1.0-rc.1`.
3. Enter the final SemVer `version` without a leading `v`, for example `0.1.0`. This must match the
   selected RC tag and is used to serialize promotion for the final release version.
4. Start the workflow.
5. Approve the `production-release` environment deployment.
6. Approve the `crates-io` environment deployment.

The workflow validates that the selected ref is the latest RC tag for the requested version,
serializes all RC creation and promotion runs for the same release version, confirms the
`bbdown-core` and `bbdown-cli` Cargo versions match the final tag, calls the shared release
verification workflow, rebuilds final release archives, rechecks that the selected RC is still latest
immediately before publication, creates the final annotated tag, publishes the GitHub Release, and
then publishes `bbdown-core`. Generated GitHub Release notes start from the
previous non-RC release tag when one exists, so the final release notes do not use the just-created
RC tag as their comparison base. If crates.io already contains the exact `bbdown-core` version, the
publish step repackages the selected RC source and treats the run as recovered success only when the
local `.crate` SHA256 matches the crates.io checksum.

## Failure Recovery

- If RC creation fails before the tag is created, fix the problem and rerun the RC workflow with the
  same version.
- If RC creation fails after a tag already exists, rerun the workflow with the same version; it will
  choose the next RC number unless the existing tag is intentionally deleted by a maintainer.
- If the final tag or GitHub Release already exists, do not create another RC for that version;
  create a new version instead.
- If promotion fails before `Publish GitHub Release`, fix the issue and rerun from the same RC tag.
  If the final tag was already created and still points at the RC target commit, the workflow reuses
  that tag.
- If promotion left behind a draft GitHub Release, rerun from the same RC tag. The workflow deletes
  the draft and recreates the release with the rebuilt assets.
- If GitHub Release publication succeeds but crates.io publication fails, prefer GitHub Actions
  `Re-run failed jobs` so only the failed crate job is retried without rebuilding artifacts. A full
  rerun from the same RC tag is fail-closed: the workflow reuses an existing published GitHub Release
  only when it is not marked as a prerelease, the release asset names exactly match the expected
  names, each asset is `uploaded` and non-empty, and the downloaded archives are named by and verify
  against their published `.sha256` sidecars, while the rebuilt `dist` archives also verify against
  their own sidecars and have the same archive checksums as the published assets. It then continues
  to crates.io publication. Release archives normalize entry ordering, timestamps, owners, groups,
  and archive container metadata so identical compiled inputs have stable package checksums. If the
  exact crate version was already accepted by crates.io, the crate publish step exits successfully
  only after the current RC package checksum matches the crates.io checksum.
- Final releases are intentionally non-overwriting. Final tags are reused only when they already
  point at the same RC target commit, and published GitHub Releases are reused only when they are
  non-prerelease releases with a complete checksum-verified asset set. To replace a bad final
  release, create a new version.

After publication, verify:

```bash
gh release view v0.1.0 --repo Joey-Project/BBDown-rust
cargo search bbdown-core --limit 5
```

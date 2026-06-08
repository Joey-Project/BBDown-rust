[ English | [简体中文](release.zh-CN.md) ]

# Release Runbook

This project releases in two phases:

1. A manually approved GitHub Actions run validates `master`, builds release artifacts, and creates
   a release candidate tag such as `v0.1.0-rc.1`.
2. A second manually approved run is started from that RC tag. It rebuilds final artifacts, creates
   the final tag such as `v0.1.0`, publishes the GitHub Release, and publishes the `bbdown-core`
   crate to crates.io.

The reusable release artifact workflow can still be run manually for archive previews, but it does
not publish tags, GitHub Releases, or crates.

## GitHub Setup

Configure these environments:

- `release-candidate`: allow deployment from the `master` branch only. Store
  `RELEASE_GITHUB_APP_ID` and `RELEASE_GITHUB_APP_PRIVATE_KEY` here, or make equivalent repository
  secrets available to this environment.
- `production-release`: allow deployment from tags matching `v*-rc.*` only. Store the same release
  GitHub App secrets here.
- `crates-io`: allow deployment from tags matching `v*-rc.*` only. Store
  `CARGO_REGISTRY_TOKEN` here.

Configure the release GitHub App with repository metadata read and contents write permissions. The
App should be the only non-human actor allowed by the release tag rulesets.

Recommended rulesets:

- `master` branch: require pull requests, require status checks `Rust` and `codex/review-gate`,
  require Code Owner review, and keep the general required approval count at `0` if ordinary PRs
  should not need human approval.
- RC tags: target `v*-rc.*`; restrict creation, updates, and deletion; allow creation only through
  the release GitHub App.
- Final release tags: target `v*` and exclude `v*-rc.*` when the UI supports excludes; otherwise use
  a separate final-tag pattern that does not match RC tags. Restrict creation, updates, and deletion;
  allow creation only through the release GitHub App.

## Create An RC Tag

1. Ensure `crates/bbdown/Cargo.toml` has the final crate version, for example `0.1.0`.
2. Ensure the intended branch is merged to `master`.
3. In GitHub Actions, run `Create Release Candidate` from the `master` branch.
4. Enter `version` without a leading `v`, for example `0.1.0`, and an RC number such as `1`.
5. Approve the `release-candidate` environment deployment.

The workflow checks that it is running from `master`, validates the `bbdown-core` Cargo version,
runs formatter, clippy, tests, and a crates.io dry run, builds all release archives, then creates
the annotated RC tag.

## Promote An RC

1. In GitHub Actions, open `Promote Release Candidate`.
2. Use the branch/tag selector to select the RC tag, for example `v0.1.0-rc.1`.
3. Start the workflow.
4. Approve the `production-release` environment deployment.
5. Approve the `crates-io` environment deployment.

The workflow validates that the selected ref is an RC tag, confirms the Cargo version matches the
final tag, reruns formatter, clippy, tests, and crates.io dry run, rebuilds final release archives,
creates the final annotated tag, publishes the GitHub Release, and then publishes `bbdown-core`.

## Failure Recovery

- If RC creation fails before the tag is created, fix the problem and rerun the RC workflow with the
  same version and RC number.
- If RC creation fails after a tag already exists, use a new RC number unless the existing tag is
  intentionally deleted by a maintainer.
- If promotion fails before `Publish GitHub Release`, fix the issue and rerun from the same RC tag.
  If the final tag was already created and still points at the RC target commit, the workflow reuses
  that tag and continues creating the missing release.
- If GitHub Release publication succeeds but crates.io publication fails, use GitHub Actions
  `Re-run failed jobs` so only the failed crate job is retried. Rerunning the entire workflow will
  fail because the final tag and GitHub Release already exist.
- Final releases are intentionally non-overwriting. Final tags are reused only when they already
  point at the same RC target commit and the GitHub Release is still missing. To replace a bad final
  release, create a new version.

After publication, verify:

```bash
gh release view v0.1.0 --repo Joey-Project/BBDown-rust
cargo search bbdown-core --limit 5
```

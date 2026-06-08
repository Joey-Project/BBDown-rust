---
id: 20260608-019e9eab-integration-api-docs
title: Integration API And Embedding Documentation
status: completed
created: 2026-06-08
updated: 2026-06-08
branch: wip/integration-api-docs
pr:
supersedes: []
superseded_by:
---

# Integration API And Embedding Documentation

## Summary

- Twelfth PR slice for the BBDown Rust rewrite continuation track.
- Scope is crate integration ergonomics and embedding documentation.
- The slice keeps behavior compatible and focuses on constructor paths that reduce downstream
  struct-literal coupling before the first crates.io release.

## Current State

- `EndpointConfig`, `ClientConfig`, and `RestrictedAreaConfig` expose builder-style APIs for
  endpoint overrides, credentials, restricted-area hints, and proxy candidates.
- `Credentials` exposes builder-style secret injection helpers for embedding projects that manage
  their own credential storage.
- `DownloadOptions`, `RetryPolicy`, `StreamSelection`, and `MuxOptions` expose constructor paths for
  common download execution configuration.
- The CLI now constructs download options, endpoint config, and restricted-area config through the
  same public crate APIs that embedders should use.
- `docs/embedding.md` documents planning, credentials, restricted-area PGC planning, downloads,
  endpoint overrides, and compatibility guidance.
- `README.md`, `crates/bbdown/README.md`, `docs/user-guide.md`, and the architecture document point
  to the embedding path and describe the builder-first API surface.
- Release archive packaging includes `docs/embedding.md` so README links remain valid in downloaded
  CLI archives.

## Evidence

- Targeted crate coverage:
  `cargo test --locked -p bbdown endpoint_client_and_restricted_area_builders_configure_embedding_inputs`.
- Targeted crate coverage:
  `cargo test --locked -p bbdown download_options_builders_configure_embedding_controls`.
- Targeted CLI coverage for endpoint and restricted-area construction paths:
  `cargo test --locked -p bbdown-cli passport_base_does_not_override_default_tv_poll_base`,
  `cargo test --locked -p bbdown-cli tv_passport_base_controls_tv_poll_when_poll_base_is_implicit`,
  `cargo test --locked -p bbdown-cli explicit_tv_passport_poll_base_wins`, and
  `cargo test --locked -p bbdown-cli restricted_area_cli_builds_proxy_chain`.
- Shell validation for release packaging: `bash -n scripts/package-release.sh` and
  `shellcheck scripts/package-release.sh`.
- Release packaging smoke:
  `scripts/package-release.sh target/debug/bbdown bbdown-package-smoke /tmp/bbdown-package-smoke`;
  the generated archive contained `docs/embedding.md`.
- Project journal validation passed with the project-journal helper.
- Full local gate: `just ci`.

## Next Steps

- No additional rewrite continuation slice is currently planned from the agreed roadmap.

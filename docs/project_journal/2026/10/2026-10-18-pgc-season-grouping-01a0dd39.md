---
id: 20261018-pgc-season-grouping-01a0dd39
title: Domestic PGC Season Episode Groups
status: completed
created: 2026-10-18
updated: 2026-10-18
branch: wip/pgc-season-grouping
pr:
supersedes: []
superseded_by:
---

# Domestic PGC Season Episode Groups

## Summary

- Preserve domestic PGC main and supplementary episode groups through the public Rust API and
  CLI metadata output, before the ordinary-video-search workstream.
- Keep the existing flat episode list, global selection indexes, exact EP selection, download
  planning, and main-episode-only `Latest` semantics.

## Model and Output

- `SeasonMetadata.episode_groups` contains ordered `SeasonEpisodeGroup` values. Each group
  exposes `kind`, optional upstream `source_id` and `title`, and `episode_positions`.
- Positions are zero-based offsets into `SeasonMetadata.episodes`, calculated after invalid
  upstream episodes are filtered. They are neither EP IDs nor the one-based display indexes.
- The root episode list forms the main group; its available upstream ID and title come from
  `positive`. Every supplementary section retains its own
  position and membership, including empty groups, repeated titles, missing or empty titles,
  and repeated episode IDs. Upstream numeric or string group IDs are represented as strings;
  absent or unsupported IDs are not synthesized.
- JSON preserves raw group metadata for the full season even when selection returns a subset.
  Human-readable `info` displays group headings and the existing global episode indexes;
  supplementary section labels count only sections, not the main group.
- Older JSON without `episode_groups` deserializes to an empty list. Rust callers constructing
  `SeasonMetadata` literals must supply the added field. Intl modules and PUGV courses retain
  their existing flat behavior and do not acquire inferred groups in this slice.

## Configured Metadata Gateways

- Existing `EndpointConfig::with_pgc_base` and CLI `--pgc-base` select the metadata base for
  domestic PGC requests. Grouping is preserved when a caller-selected gateway supports the
  requested metadata paths and compatible response shapes.
- `PgcWebPlayurlRoute` continues to control Web playurl only. This change does not add metadata
  fallback to playurl-only services, change credential forwarding, or claim live regional access.

## Validation

- `just ci` passed with stable Cargo/rustc 1.99.1: formatting, strict workspace Clippy, Rust 1.95.0
  MSRV check, workspace tests (877 passed, 3 ignored), the separate CLI e2e command (160 passed),
  and crate publication dry run. No crate was uploaded. The existing published `0.10.0` version
  and locked yanked `spin 0.9.8` produced warnings, not gate failures.
- Current stable's `assert_is_empty` lint is satisfied with typed empty-collection equality
  assertions. Validation covers both stable 1.99.1 and the separate 1.95.0 MSRV gate.
- Mock coverage checks main-group `positive` metadata, filtering before member-position assignment, repeated episode IDs,
  optional numeric/string source IDs, empty or repeated titles and groups, old JSON defaults,
  public reexports, configured metadata endpoints, full CLI JSON metadata, human-readable
  output, numeric and exact-EP selection, and main-only `Latest` behavior.
- The focused CLI grouping e2e also passed independently (1 passed, 159 filtered). External
  Bilibili APIs were mocked; these tests do not establish live gateway regional access.
- Project-journal frontmatter validation and `git diff --check` passed.

## Next Steps

- Ordinary video search remains the next page-family feature. Timetable and film/TV categories
  follow it; broader metadata/list reverse-resolver support remains capability-dependent.

## Evidence

- PR: `https://github.com/Joey-Project/BBDown-rust/pull/105`.
- Priority and shared resolver requirements: `2026-10-11-page-family-roadmap.md`.
- Implementation: `crates/bbdown/src/models.rs`, `client.rs`, and `lib.rs`.
- CLI and public-surface coverage: `crates/bbdown-cli/tests/cli_e2e.rs` and
  `crates/bbdown/tests/public_api.rs`.

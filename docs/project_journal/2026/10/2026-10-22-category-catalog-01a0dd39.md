---
id: 20261022-category-catalog-01a0dd39
title: Offline Category Catalog Snapshot
status: completed
created: 2026-10-22
updated: 2026-10-22
branch: codex/category-catalog
pr:
supersedes: []
superseded_by:
---

# Offline Category Catalog Snapshot

## Summary

- Expose an offline `category_catalog()` Rust API and `bbdown category catalog` CLI command for a bundled snapshot of Bilibili's frontend channel-menu fallback.
- Keep remote menu fetching, full category taxonomy, automatic ID translation or route classification, generic URL parsing, content enumeration, and video listing outside this slice.
- Synchronize bilingual README, user guide, embedding guide, the catalog-source note, TODO, and page-family roadmap.

## Public Contract

- `bbdown_core::category_catalog() -> Result<CategoryCatalog>` reads bundled JSON without constructing or using `BiliClient`, endpoint settings, credentials, resolvers, network, or request timeout. `bbdown category catalog [--json]` takes the same offline path before timeout validation and credential/resolver initialization.
- The catalog carries `scope`, `captured_at`, `source_url`, `source_asset_url`, `source_asset_sha256`, and `entries`. The snapshot scope is `bundled_channel_menu_fallback`. `captured_at` is acquisition time, not an upstream revision.
- Each recursive entry contains `name`, `url`, optional `channel_id`, optional `sub_channel_id`, optional `tid`, and `children`. `channel_id` and `sub_channel_id` are navigation identifiers independent of `tid`; no automatic conversion is provided.
- The included `channelList` fallback snapshot has 28 roots and 114 nested entries in source order. It preserves 12 entries without `tid` and duplicate `tid` values: 17 and 138 are duplicated menu entries, while 246 occurs on two distinct car subentries. It includes UGC-looking, PGC/program, and non-video destinations and has no UGC/PGC discriminator. The separate `sideChannelList` (6 roots and 20 nested entries) is excluded.
- Menu URLs normalize to HTTPS and remove tracking-only `visit_id` query parameters; menu paths, functional query parameters, and functional URL fragments (including the VLOG `#/530003` fragment) are preserved. JSON returns all fields and provenance; human output groups children and distinguishes navigation IDs from `tid`.
- The frontend can replace this fallback using a runtime remote key-value menu. The bundled snapshot does not claim to be the current effective menu or a complete category taxonomy. It does not prove that every `tid` maps to legacy `newlist/rid` or that every menu route lists videos. Callers may explicitly test a known chosen `tid` with the separate `category list` API; no inference or translation is performed.

## Source Evidence

- Two anonymous GETs were made with redirects disabled, no credentials/cookies, proxy environment cleared, 10-second per-request timeout, and a 90-second aggregate bound. `https://www.bilibili.com/c/douga/` returned HTTP 200 (116,545 bytes, SHA-256 `8d7a7aa5b91d826516198115b69e2fe3ec4fb1d44707d27093039e9aac42c713`) at `2026-10-22T22:32:22.332079Z`; the source asset returned HTTP 200 (960,224 bytes, SHA-256 `e9a6380a51acb054d8728db716a10a13818371a7834ba5f4f53a4b7872cfcbcb`) at `2026-10-22T22:32:44.646206Z`.
- The HTML response was capped at 1 MiB and the JavaScript asset at 4 MiB. The asset was statically inspected with a bounded tokenizer and was not executed for this extraction. Evidence and source context are in `/private/tmp/bbdown-category-catalog-20261022-01a0dd39/evidence.json` and `source-context.txt`.
- Source context shows the remote `channel_list` key-value group may override the embedded menu; absent/failed remote ordering uses `channelList`. It also distinguishes `channelId` and `tid` in frontend routes and analytics. This supports exposing the fallback as captured navigation data, not asserting live taxonomy semantics.

## Validation

- Full local CI passed (exit 0, 77.67 seconds, under the 600-second / 2 MiB runner bounds): formatting check, strict Clippy for workspace/all targets, Rust 1.95 compatibility check, 1,038 workspace tests passed with 4 ignored, separate CLI e2e passed 213/213, and the `bbdown-core` package dry run succeeded for 42 files including the bundled JSON. The standalone CLI test group is reported separately and is not added again to the workspace total. No package was published. Receipt and bounded log: `/private/tmp/bbdown-category-catalog-20261022-01a0dd39/ci-attempt3.log.json` and `ci-attempt3.log`.
- Offline CLI acceptance passed for both `bbdown category catalog --json` and human-readable `bbdown category catalog`: exit 0 and empty stderr. JSON's recursive entries exactly matched an independently parsed 142-entry source snapshot (28 roots and 114 children), including 12 missing TIDs and duplicate TIDs 17, 138, and 246 each appearing twice. Human output preserved provenance, source order, representative nesting, and a missing-TID entry. Failure-path checks with an invalid API base, resolver, credential profile, and timeout 0 also passed; the absent credential sentinel remained absent. Runtime was 0.064 seconds.
- The tested binary SHA-256 was `e98062f5f0981840e0971067d0dda13d9c0118f15b0865b3e3948c6e08c119fa`; the six feature source hashes remained stable through validation. The binary was a working-tree build based on `10e7021`, not a clean-base build. Safe acceptance summary: `/private/tmp/bbdown-category-catalog-20261022-01a0dd39/accept-catalog-cli-summary.json`.
- These checks validate the bundled snapshot and offline command only. No authenticated request, live menu override fetch, playback, media request, or content-listing validation was performed; the snapshot is not the current effective menu or a complete taxonomy.

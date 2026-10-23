---
id: 20261023-remote-channel-menu-01a0dd39
title: Opt-In Remote Channel Menu Configuration
status: completed
created: 2026-10-23
updated: 2026-10-23
branch: codex/remote-channel-menu
pr:
supersedes: []
superseded_by:
---

# Opt-In Remote Channel Menu Configuration

## Summary

- Add an explicitly requested live mode for reading the site's remote channel-menu configuration while preserving the existing offline bundled fallback as the default.
- Expose configuration metadata only. The returned structure does not claim to be the website's current effective or visible menu and does not provide content listing, taxonomy completion, feeds, pagination, hydration, playurl, or media.

## Current State

- The implementation and local validation gates passed. This entry records completed target-branch state after squash; transient PR review/merge state is not recorded here.
- CLI: `bbdown category catalog --live [--max-response-bytes N] [--json]`. Omitting `--live` preserves the offline command's behavior and makes no request. Live mode uses the configured API base and request timeout, sends one anonymous request, and fails explicitly without an offline fallback. It does not initialize credential, PGC, resolver, or restricted-area proxy configuration.
- Rust: `BiliClient::channel_menu(&ChannelMenuOptions) -> Result<ChannelMenu>`, with a 1 MiB default decoded-response cap and a configurable cap. Zero is rejected before HTTP. There is no row-count limit.
- Route: `/x/kv-frontend/namespace/data?appKey=333.1339&nscode=10`. The response exposes the ordered channel configuration selected by its `sort` array. Flattened `channel_list.*` values are JSON strings in `data.data`.
- `ChannelMenu` reports `scope=remote_channel_menu_config`, optional opaque `version_id` and `app_version_id`, and ordered `entries`. Each recursive entry keeps optional root `key`, `name`, `url`, `route`, optional channel/navigation/category identifiers and `season_type`, raw numeric `enable_sub`, and `children`.
- Preserve root configuration keys independently from routes and numeric identifiers; child entries have no root key. Keep explicit zero TIDs, duplicate records, empty URLs, and configured children whose `enable_sub` is zero. Normalize protocol-relative URLs to HTTPS and retain functional query parameters/fragments. Empty sorted configuration is a successful empty result. Required sort/reference errors remain explicit.
- The API does not resolve runtime menu selection, side-menu groups, or unsorted records, and does not infer TID/RID or UGC/PGC relationships. Existing `category_catalog()` remains an offline snapshot; `category_videos()` remains a separate fixed first-batch endpoint.

## Source Research

- Anonymous source page: `https://www.bilibili.com/c/douga/`; frontend asset: `https://s1.hdslb.com/bfs/static/shanks/channel/assets/index-bedd2c00.js`, SHA-256 `e9a6380a51acb054d8728db716a10a13818371a7834ba5f4f53a4b7872cfcbcb`. The source asset was inspected statically and not executed.
- The static frontend trace derives the default namespace host as `api.bilibili.com` from its source and constructs the namespace GET with `appKey=333.1339` and `nscode=10`; an optional runtime host override means this does not prove every browser session uses that host. The observation does not require authentication and is not a compatibility or implementation validation.
- An initial response summarizer looked for nested fields rather than the flattened `data.data` object; its reported count of 49 keys is unreliable. A second request to the same endpoint was parsed with the corrected shape. It returned HTTP 200/API code 0, 45,943 decoded bytes in 0.707 seconds, SHA-256 `d554128a2628c0a1e415af4abd2d9f5806bef8737d0c8f7e3961360c5ad2c24c`.
- That captured sample contained 50 namespace keys (43 `channel_list.*`, 7 side-menu keys), 39 sort keys resolving to 39 roots and 204 children, with 33 roots having `enableSub=0` and 6 roots having it set to one. It included 13 explicit zero TIDs, 200 empty URLs, and 43 protocol-relative URLs. These are observations about the captured response only, not stable count guarantees or evidence of the effective visible menu.
- Research was anonymous, with redirects and inherited proxies disabled; no credentials were read or sent. Private evidence: `/private/tmp/bbdown-next-channel-20261023-01a0dd39/plan.md`, `channel-kv-trace.txt`, `kv-response-2-summary.json`, and `checkpoint.json`.

## Validation

- `just ci` passed on 2026-10-23 from 16:15:34.692722Z to 16:17:25.936746Z (111.244 seconds): formatting, strict Clippy, Rust 1.95 workspace check, 1,051 workspace tests passed with 4 ignored, a separate 216-test CLI suite passed, and the 43-file core package dry run succeeded. No package was published. CI made no Bilibili, credential, or media requests.
- Bounded actual CLI acceptance passed three cases in 1.952 seconds against a working-tree build based on `d2452f6` (not a clean-base build). Offline JSON returned 28 roots and 114 children. Live official JSON matched all 243 parsed entries recursively and in order against the independent captured KV response (39 roots, 204 children); live human output verified labels, versions, root ordering, grouping, a configured child under a root with `enable_sub=0`, a blank URL, and an independent zero TID. All commands exited 0 with empty stderr. The credential sentinel remained absent; intentionally invalid endpoint/profile/resolver settings were bypassed by both offline and live early paths (the live API base was valid). This validates observed CLI behavior against the captured response, not current website visibility or a stable upstream count guarantee.
- Acceptance binary SHA-256: `5bd5190e0f203332f0f47b2d1de71b5ebdab7d41ea1fd400135f04855ae1f04c`. The six feature source hashes and captured KV response SHA-256 `d554128a2628c0a1e415af4abd2d9f5806bef8737d0c8f7e3961360c5ad2c24c` remained stable during acceptance. Safe summary: `/private/tmp/bbdown-next-channel-20261023-01a0dd39/accept-channel-menu-cli-summary.json`.
- The source observation and bounded CLI acceptance remain distinct evidence. Neither establishes a full taxonomy or the site's current effective/visible menu.

## Next Steps

- Full taxonomy, current effective/visible menu resolution, dynamic channel feeds, pagination, and topic pages remain separate future work.

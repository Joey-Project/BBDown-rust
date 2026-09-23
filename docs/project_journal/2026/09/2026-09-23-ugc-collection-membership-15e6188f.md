---
id: 20260923-ugc-collection-membership-15e6188f
title: UGC Collection Membership Resolution
status: completed
created: 2026-09-23
updated: 2026-09-23
branch: wip/ugc-collection-membership
pr:
supersedes: []
superseded_by:
---

# UGC Collection Membership Resolution

## Summary
- 为普通 BV 视频增加显式的 UGC 合集/系列归属查询，并保留普通视频解析与下载规划的单视频默认语义。
- 新的 reference 可显式复用现有空间合集/系列分页解析器，得到 `VideoCollectionResolution`。

## Current State
- `ViewData` 解析可选的 `ugc_season` 稳定元数据，严格拒绝缺少必要字段和未知 `season_type`。
- 公共 API、模型、双语 embedding 文档和定向测试已加入；新增 5 个 UGC 测试与 crate 根 public API 测试已通过。

## Next Steps
- 保持普通 BV 的单视频解析/下载规划语义；后续调用方可通过 reference 显式进入合集解析路径。

## Evidence
- Branch: `wip/ugc-collection-membership` from `origin/master` at `4df3c7d`.
- Targeted validation: `cargo test -p bbdown-core ugc --locked`.
- Final validation: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

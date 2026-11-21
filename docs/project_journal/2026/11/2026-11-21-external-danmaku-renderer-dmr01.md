---
id: 20261121-dmr01
title: 迁移 ASS 渲染依赖
status: completed
created: 2026-11-21
updated: 2026-11-21
branch:
pr:
supersedes: []
superseded_by:
---

# 迁移 ASS 渲染依赖

## Summary

- BBDown 的 ASS 输出使用 crates.io 上的 `danmaku-renderer`。完整文档走 Naive，归档更新使用兼容旧输入的入口。保留 SDK 的 XML 命名空间筛选、公开 API 上限和 XML 合并逻辑。

## Validation

- 覆盖普通与命名空间 XML、错误输入、公开 API 上限以及下载和归档更新的 ASS 路径。

## Evidence

- [danmaku-renderer 0.1.0](https://crates.io/crates/danmaku-renderer/0.1.0)

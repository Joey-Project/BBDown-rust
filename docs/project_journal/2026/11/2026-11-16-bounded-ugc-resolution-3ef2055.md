---
id: 20261116-ugc-resolution-limits-3ef2055
title: Bounded UGC Collection Resolution
status: completed
created: 2026-11-16
updated: 2026-11-16
branch: codex/ugc-resolution-limits
pr: "https://github.com/Joey-Project/BBDown-rust/pull/126"
supersedes: []
superseded_by:
---

# Bounded UGC Collection Resolution

## Summary
- 为 UGC 合集和系列解析新增显式限额 API；原有完整结果 API 保持兼容。
- 在读取、反序列化和展开各阶段限制响应字节、请求数、原始档案、视频页及返回字符串字节，超限返回稳定的 `InvalidInput`。

## Current State
- 新 API 的默认上限为每响应 1 MiB、10 个合集请求、100 条档案、100 个展开页及 4 MiB 返回字符串；硬上限分别为 16 MiB、512、4096、4096 和 64 MiB。
- 新增 mock 覆盖 Content-Length 头超限、chunked 响应、上游超报、声明总量、请求预算、页展开、字符串扩展、取消、Unicode、系列元数据及旧 API 兼容。
- 默认完整结果 API 保持兼容；新增选项 API 在解码及展开前实施响应和集合预算，并在克隆/扩展结果字符串前执行聚合字节预算。

## Validation
- 独立 `rust-cache/core` 下的 `cargo test --locked -p bbdown-core` 通过：769 passed、0 failed、2 ignored；全量日志确认新增 Content-Length 与 chunked 测试均实际运行。
- 最终 Content-Length 头超限定向回归通过：1 passed、0 failed；raw TCP 服务只发送超限头并等待客户端关闭或有界读超时，验证客户端不等待/解析 body。
- 严格 workspace/all-target Clippy 与 `cargo fmt --all -- --check` 在独立 core cache 下通过。

## Evidence
- Branch `codex/ugc-resolution-limits`, based on `3ef20552d7e81aa8e3816a4dd6f006391319ef9c`.
- Focused mock tests are in `crates/bbdown/src/client/ugc_collection.rs`; public exports are covered by `crates/bbdown/tests/public_api.rs`.
- Independent-cache full test log: `ugc-isolated-tests-corecache-1794832090660587000.log`.
- Final focused Content-Length log: `ugc-isolated-content-length-final-1794832700622927000.log`.
- Final strict Clippy log: `ugc-core-strict-clippy-final-1794832727208889000.log`.
- Final workspace format log: `ugc-core-fmt-final-1794832861550979000.log`.

---
id: 20260930-pgc-web-playurl-routes
title: Independent PGC Web Playurl Routes For Embedding Clients
status: completed
created: 2026-09-30
updated: 2026-09-30
branch: codex/pgc-web-route-modes
pr:
supersedes: []
superseded_by:
---

# Independent PGC Web Playurl Routes

## Summary

- 为嵌入式调用方增加独立的 PGC Web playurl 路由，使 LAN server 可自行并发探测官方与指定反代。
- 默认官方解析、区域错误时顺序回退反代的行为保持不变；PGC metadata 和 TV/APP playurl 不受影响。

## Current State

- `ClientConfig::pgc_web_playurl_route` 支持 `OfficialThenProxy`、`OfficialOnly` 和
  `ProxyOnly(RestrictedAreaProxy)`；proxy-only 只选择一个反代服务器，API 风格反代可在该
  服务器上依次尝试 Web 与 Web v2 路径。
- 反代请求继续使用可选的通用 `Credentials::access_key`，不转发 Bilibili Web cookie。
  失败摘要和解析诊断继续脱敏路径、查询参数和密钥。
- `PlaybackEntry.diagnostics` 透传已有的 resolver 诊断；空诊断不写入 JSON，旧 JSON
  缺少该字段时仍可反序列化。LAN server 负责校验内容标识与 source 并选取胜出结果。
- core 不启动后台解析任务；调用方取消规划 future 即可停止对应请求。

## Validation

- 4 个定向 mock 测试覆盖并发双 client、官方失败不回退、proxy-only 失败脱敏和 API 风格
  反代 Web/v2 路径；既有默认回退测试继续保留。
- 完整 `cargo test --workspace --locked`、严格 Clippy、Rust 1.95 检查、CLI mock e2e、
  格式检查及 core 打包 dry-run 均通过。
- 真实站点 live e2e 需要读取本机 Web cookie 与 access key，向 Bilibili 官方接口及
  `atri.ink` 发请求。本次执行被本机审批拦截；未绕过，也未将凭证复制或提交到 worktree。

## Follow-up

- LAN server 使用独立 client 并发规划，按内容标识、来源和脱敏诊断验证结果后选择。
- 在得到明确凭证发送授权后执行 restricted Bangumi live e2e；发布新 crate 版本须另行决定。

---
id: 20260930-pgc-web-playurl-routes
title: Independent PGC Web Playurl Routes For Embedding Clients
status: completed
created: 2026-09-30
updated: 2026-10-04
branch: codex/pgc-web-route-modes
pr: https://github.com/Joey-Project/BBDown-rust/pull/79
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
- CI 使用最新 stable；Rust 1.99 新增的 `assert_is_empty` 检查要求既有测试采用长度
  断言。保留严格 lint 门槛和相同测试语义，仅将 29 处集合空/非空断言改为显式长度比较。
- 新增显式忽略的嵌入 API live e2e，使用 `restricted-bangumi-episode ep664928`，从本机
  BBDown 默认 profile 读取 Web cookie 与通用 access key，并选取本机忽略的 manifest 中唯一
  的香港 API 反代。只执行 `plan_playback`，不下载媒体；错误输出仅包含安全分类，不输出凭据或
  原始反代 URL。
- 2026-10-04 在明确授权向 Bilibili 官方接口发送 Web cookie、向 manifest 所选 `atri.ink`
  反代发送通用 access key 后运行：
  `BBDOWN_PGC_LIVE_CREDENTIAL_FILE=<local-profile-store> cargo test -p bbdown-cli --test live_e2e live_pgc_web_playurl_routes_for_restricted_episode --locked --offline -- --ignored --exact`
  通过。`OfficialOnly` 收到区域限制码 `-10403`；`ProxyOnly` 返回可播放的 `PgcProxy`
  条目，且诊断中没有 `PgcWeb` 尝试。最初 manifest 指向的旧凭据路径已不存在，因此本次通过
  仅用于该测试的环境变量覆盖凭据文件。

## Follow-up

- LAN server 使用独立 client 并发规划，按内容标识、来源和脱敏诊断验证结果后选择。
- 发布新 crate 版本须另行决定。

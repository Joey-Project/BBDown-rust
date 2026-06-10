[ [English](user-guide.md) | 简体中文 ]

# 用户指南

## 范围

BBDown Rust 当前提供可复用的 `bbdown-core` package / `bbdown_core` crate 和一个 CLI，用
于确定性的元数据解析、下载计划解析、媒体下载执行、旁路文件下载，以及可选 `ffmpeg` 封装。
支持的输入家族包括普通视频、PGC 和 intl 分集、PUGV/cheese 课程、B23 短链接、收藏夹、
空间投稿、合集和系列。

## 发布归档

GitHub release 提供预打包的 `bbdown` CLI 归档，覆盖 Linux x86_64、macOS x86_64、
macOS aarch64 和 Windows x86_64。下载与你的平台对应的归档，按需使用旁边的 `.sha256`
文件校验，解压后把 `bbdown` 或 `bbdown.exe` 二进制放到 `PATH` 中。归档也包含英文和简体
中文 README、用户指南、嵌入指南、架构指南，以及 `LICENSE`。安装二进制后，可运行
`bbdown --version` 确认归档版本。

## Crate 发布

crates.io 发布目标是可复用的 `bbdown-core` library package。使用 `just publish-dry-run`
可以在本地执行锁定版本的 dry run，并允许工作树存在未提交修改；使用
`just publish-dry-run-strict` 或 `cargo publish --dry-run -p bbdown-core --locked` 可以复现
干净 CI 门禁。`bbdown-cli` 包标记为 `publish = false`；CLI 应通过 GitHub release 归档安
装或分发。当前开发线是已发布 `0.1.0` 之后的 `0.2.0`，因为批量 collection metadata 增
加了 `ResolvedContent::Collection`。嵌入调用方仍应优先使用 `DownloadOptions::new`、
`StreamSelection::new`、`Default` 等构造器，而不是 public struct 字面量，并把公开的 plan
输出容器视为会随 crate 成熟继续新增字段的被消费数据表面。

## Library 嵌入

当另一个 Rust 项目需要 typed metadata、下载计划、下载执行报告或受限区域解析器诊断，而
不希望启动 CLI 时，可以使用 `bbdown-core` package 中的 `bbdown_core` crate。仓库的
[嵌入指南](embedding.zh-CN.md)提供了 `ClientConfig`、`EndpointConfig`、credentials、受限
区域代理和 `DownloadOptions` 的可复制示例。

嵌入调用方应保持配置构造基于 `Default`、`new` 和 `with_*` 方法。此工作区中的 CLI 使用
同一路径，因此这些 builder 会被常规 CI 和 mock e2e 测试覆盖。

## 元数据

使用 `info` 解析元数据：

```bash
bbdown info av170001 --json
bbdown info BV1qt4y1X7TW --json
bbdown info ep267851 --json
bbdown info ss26801 --select latest --json
bbdown info md22718131 --select episode:267851 --json
bbdown info https://b23.tv/example --json
bbdown info cheese/ep101 --json
bbdown info cheese/ss202 --select latest --json
bbdown info fav456 --json
bbdown info mid123 --select page:1 --json
bbdown info collection456 --json
bbdown info series456 --select latest --json
bbdown info https://www.bilibili.com/medialist/detail/ml1103407912 --json
bbdown info https://www.bilibili.com/list/ml1103407912 --json
bbdown info 'https://www.bilibili.com/list/1958703906?sid=547718' --json
bbdown info 'https://space.bilibili.com/123/favlist?fid=456' --json
bbdown info 'https://space.bilibili.com/123/lists/456?type=series' --json
```

Season、media 和 `cheese/ss...` 输入在非交互模式下需要 `--select`。支持的 selector 是
`latest`、`all`、`episode:<epid>` 和 `page:<index>`。`current` 只对 `ep`、`cheese/ep`
和 `bilibili.tv` 分集 URL 有意义，因为这些输入本身已经标识当前分集。收藏夹、空间投稿、
合集和系列是批量输入；不传 `--select` 时会解析全部条目。使用 `--select page:<index>` 可
选择一个集合条目，使用 `--select latest` 可选择最新解析到的条目。JSON metadata 会在
`collection.collection.items` 保留完整解析到的集合条目列表，并在
`collection.selected_items` 报告当前选中子集；空集合是有效的空列表。

收藏夹 URL 可来自 shorthand id、空间 favlist 页面、canonical `/list/ml...` 页面，以及
`/medialist/.../ml...` 页面。空间合集和系列 URL 会保留
`/space.bilibili.com/<mid>/...` 或 `/list/<mid>?sid=...` 中的 owner mid，让解析器可以使用
新的 owner-scoped 空间 API。

## 下载计划

使用 `plan` 解析流、字幕和弹幕可用性：

```bash
bbdown plan av170001 --json
bbdown plan ep267851 --json
bbdown plan ss26801 --select latest --json
bbdown plan https://www.bilibili.tv/en/play/34613/341736 --json
bbdown plan cheese/ep101 --json
bbdown plan fav456 --select page:1 --json
bbdown plan 'https://space.bilibili.com/123/channel/collectiondetail?sid=456' --select all --json
```

JSON 输出包含：

- `entries`：选中的页面、分集或批量集合条目。
- `streams.qualities`：当前可选择的 DASH 视频质量 id，并可附带 playurl 响应中的说明。
- `streams.accept_quality`：为兼容保留的原始 accepted video quality id。
- `streams.videos`：DASH 视频轨道。
- `streams.audios`：DASH 音频轨道，包括可用的 Dolby 或 FLAC 音频。
- `streams.flv_segments`：playurl 响应使用 `durl` 时的 legacy FLV 分段。
- `subtitles`：发现的字幕轨道。
- `danmaku.xml_url`：条目 `cid` 对应的 XML 弹幕端点。

规划没有副作用。它不会创建文件、下载媒体或调用 ffmpeg。对于批量输入，规划只抓取并输
出选中的条目，因为 collection metadata 属于 `info` 输出。面向人类的 plan 输出会列出相同
的可选质量 id 和流摘要，因此用户无需手动解析 JSON 也能选择下载质量。
PGC 和 intl 规划仍可能需要符合条件的账号或区域访问。当 intl 元数据返回 region-limit
payload 时，CLI 会把它报告为访问限制。配置后，PGC playurl 解析会回退到受限区域代理候选，
并在条目 JSON 中包含解析器诊断。

## 下载

使用 `download` 解析计划并写入文件：

```bash
bbdown download av170001 --output-dir downloads
bbdown download ss26801 --select latest --output-dir downloads
bbdown download fav456 --select page:1 --output-dir downloads
bbdown download av170001 --output-dir downloads --no-mux --json
bbdown download av170001 --video-quality 64 --audio-quality 30216 --output-dir downloads
bbdown download av170001 --output-dir downloads --archive-file downloads/archive.json --on-duplicate keep-both
```

默认情况下，命令会为每个条目下载第一组完整 DASH 视频/音频。先使用 `bbdown plan` 查看可
用 id，再传入 `--video-quality <ID>` 或 `--audio-quality <ID>` 选择特定 DASH 视频或音频流。
请求的 id 必须存在于该条目的 plan 中；否则命令会报告可用 id，并在写入媒体前失败。当
DASH 媒体不完整而 legacy FLV `durl` 分段可用时，会改为下载这些分段。显式质量选择要求
DASH 媒体，因此会禁用该条目的 FLV 回退。如果两种形态都不完整，下载会在写入媒体前失败。
字幕和弹幕旁路文件默认启用，可用 `--no-subtitles` 和 `--no-danmaku` 关闭。

下载默认通过 HTTP range 请求续传部分文件，并在计划提供信息时校验 `Content-Range` 和声
明媒体大小。使用 `--no-resume` 可强制重新写入；失败的新写入会保留已有目标。如果服务器
忽略续传 range，旧 partial 会在临时完整重试通过可用长度校验后被替换；没有长度信号时，
完整重试会被拒绝，从而保留已有文件。重试行为由 `--retry-attempts` 和
`--retry-backoff-ms` 限定。条目目录包含内容身份，DASH 媒体文件名包含流元数据身份，因此
同标题视频和不同 codec 变体不会共享同一个续传目标。没有写入任何字节却完成的媒体下载会
被拒绝。

当调用方需要重复预检查和已完成下载的持久记录时，使用 `--archive-file <path>`。归档是按
内容身份索引的本地 JSON 文件；它记录输出路径、条目 id、旁路文件路径、mux 输出路径和完
成时间戳，但不存储媒体 URL 或凭据。输出、旁路文件和 mux 路径会在记录时保存为绝对路径，
因此归档可以从另一个工作目录复用。条目身份使用稳定的 aid/cid 媒体 id，因此同一 PGC 分
集之后通过 BV/av URL 规划时，即便某种形式缺少 BVID，也仍能匹配。当计划内容 key、条目
身份或归档输出目录已经存在时，CLI 需要一个重复决策：

- `--on-duplicate replace` 会先删除已有的计划输出目录或文件，再重新下载，并替换指向该
  输出路径的陈旧归档记录。
- `--on-duplicate keep-both` 会写入下一个可用的带后缀输出目录，例如 `Mock video (2)`，
  并保留旧归档记录，包括旧输出目录已不在磁盘上的 archive-only 记录。
- `--on-duplicate cancel` 会在下载前停止，并在 `--json` 模式下输出
  `{"status":"canceled","preflight":...}`，以便自动化检查已有记录和输出冲突。

没有显式决策时，人类 TTY 模式会在 stderr 提示。`--json` 模式和非 TTY 模式永远不会提示；
它们会失败并提示传入 `--on-duplicate`。没有 `--archive-file` 时，下载行为不变，也不会进
行重复预检查。`--archive-file` 必须指向一个 JSON 文件路径，且不能与所选内容和重复决策的
输出目录重叠；对 `keep-both` 来说，检查应用于实际带后缀输出目录。如果 `--archive-file`
是符号链接，保存会更新符号链接目标，使多个调用方能够共享一份归档路径而不拆分历史。CLI
还会在保存归档前，根据 executor 报告的实际输出目录再次检查 archive-file 保护。

`--request-timeout-seconds` 作用于 API 请求。媒体正文读取使用
`--download-idle-timeout-seconds`；传入 `0` 可禁用该 idle timeout。

默认通过 `ffmpeg` 启用 mux。使用 `--ffmpeg <path>` 选择二进制，或使用 `--no-mux` 仅保留
下载的媒体旁路文件。可复用 crate 通过 `DownloadOptions::mux` 让外部进程执行保持显式。
重新 mux 会先写入并校验临时输出，然后替换最终文件，因此失败的 mux 会保留已有 mux 文件。

## 凭据

当端点需要账号访问时，导入凭据：

```bash
bbdown auth import-cookie --stdin
bbdown auth import-access-key --stdin
bbdown auth login-web
bbdown auth login-tv
bbdown auth status
bbdown auth logout
```

没有输入标志时，密钥导入命令也会读取 `BBDOWN_COOKIE` 和 `BBDOWN_ACCESS_KEY`。使用
`--credential-file <path>` 可以把测试凭据与默认平台配置路径隔离。`auth login-web` 打印二
维码登录 URL，轮询到扫码确认，并保存得到的 cookie。`auth login-tv` 使用 TV 二维码流程，
保存 TV 专用 access key 供未来 TV/app 流程使用，不会覆盖由 `auth import-access-key` 导入
的通用 intl/Bstar access key。使用 `--json` 时，二维码登录会打印换行分隔 JSON 事件：
`ticket` 在轮询前包含扫码 URL，`saved` 只包含脱敏后的凭据布尔值。请把扫码 URL 当成临时
登录密钥，因为它包含二维码登录 key。状态输出或 `saved` JSON 事件不会打印 token 值。

## 端点覆盖

CLI 接受端点覆盖，用于 mock 测试和未来解析链：

```bash
bbdown --api-base http://127.0.0.1:8080 plan av170001 --json
bbdown --pgc-base http://127.0.0.1:8080 --api-base http://127.0.0.1:8080 plan ep267851 --json
bbdown --intl-base http://127.0.0.1:8080 plan https://www.bilibili.tv/en/play/34613/341736 --json
bbdown --comment-base http://127.0.0.1:8080 download av170001 --output-dir downloads
bbdown --passport-base http://127.0.0.1:8080 auth login-web
bbdown --tv-passport-base http://127.0.0.1:8080 auth login-tv
bbdown --tv-passport-base http://127.0.0.1:8080 --tv-passport-poll-base http://127.0.0.1:8081 auth login-tv
```

当前 intl 支持使用官方 intl 元数据/字幕端点，以及在配置 access key 时使用官方签名 intl
OGV playurl 端点。弹幕 XML 下载使用可配置的 comment 端点。WEB 二维码登录使用
`--passport-base`；TV 二维码登录使用 TV 专用 passport 覆盖。提供 `--tv-passport-base` 时，
TV 二维码轮询会跟随该覆盖；对 split-host mock 或代理，请设置 `--tv-passport-poll-base`。

## Live E2E 样例

`just live-e2e` 是面向真实 Bilibili 样例的本地专用验证门禁。它不是默认 CI 的一部分，因为
结果依赖网络、账号、token 和区域资格。该 recipe 需要仓库根目录存在被忽略的
`live-e2e.samples.json` manifest；请使用已跟踪的 `live-e2e.samples.example.json` 作为结构
参考。

manifest 可以用 `credential_file` 指向已有凭据文件，用 `access_key_file` 读取纯文本
access key，设置 `request_timeout_seconds`，并用与 CLI 相同的区域名配置受限代理候选。
`restricted_api_proxy_all_areas` 和 `restricted_area_proxy_all_areas` 会把每个列出的 URL 展
开为 `cn`、`th`、`hk` 和 `tw` 候选。每个 case 声明 `kind`、`url`、可选 `selection`、可选
受限区域提示、`info` 或 `plan` 等 action，以及预期 JSON 形态。测试框架只会把
cookie/access-key 字段复制到该 case 的临时凭据文件，并在运行真实 `bbdown` 二进制前移除
CLI 覆盖环境变量。未知 manifest 字段会被拒绝，因此拼错的 expectation key 会快速失败，
而不是静默削弱 live 门禁。
使用 `allowed_plan_sources` 可以拒绝意外来源；当某个来源必须至少出现一次时，使用
`required_plan_sources`。对于依赖可变账号或区域资格的受限样例，case 可以设置
`allow_plan_error: true` 和 `plan_error_contains`：成功的 `plan` 仍必须匹配流断言，而受限
失败必须是 access-restricted failure，并包含列出的诊断片段。

## 受限区域代理

工具不包含公共代理默认值。只配置你自己运营或信任的代理主机。PGC playurl 回退只会在官
方 PGC playurl 响应报告区域限制后尝试。其他官方失败（例如 VIP/paywall 错误、解析失败或
网络错误）会保留原错误，而不是尝试代理主机。

```bash
bbdown --restricted-area hk --restricted-area-proxy hk=https://proxy.example/playurl plan ep267851 --json
bbdown --restricted-api-proxy tw=https://proxy.example/bili/api plan ss26801 --select latest --json
```

代理 spec 使用 `area=url` 或裸 URL。支持区域为 `cn`、`th`、`hk` 和 `tw`。裸 URL 是通用候
选。`--restricted-area <area>` 是一个提示，会把匹配候选移到最前。没有提示时，顺序为通用
候选、`cn`、`th`、`hk`、`tw`，并移除重复项。重复命令行代理标志在同一区域优先级内保留
声明顺序。当命令行和环境变量代理值同时存在时，先尝试命令行候选，再尝试环境 playurl 代
理，最后尝试环境 API-path 代理。每个来源组都会按区域提示、通用候选、固定区域顺序排序。

`--restricted-area-proxy` 面向 BBDown/BiliPlus 风格的 HTTP(S) playurl 代理端点，原 PGC
playurl 查询会发送到配置 URL。`--restricted-api-proxy` 面向镜像 `api.bilibili.com` 路径结
构的 HTTP(S) 代理，因此 CLI 会先调用该 base URL 下的 `/pgc/player/web/playurl`，以匹配常
见 BALH 风格 API 代理主机，然后对实现旧路径的 API 代理回退到
`/pgc/player/web/v2/playurl`。如果配置的 API proxy base URL 已经包含 query string，该
query 会在追加 PGC playurl 参数之前保留。代理响应可以包在 `data` / `result` 中，也可以返
回 helper 风格的顶层 `dash` / `durl`、`timelength` 和质量元数据；legacy 字符串状态字段
（例如 `result: "suee"`）会被容忍。两个标志都可以重复传入。
`BBDOWN_RESTRICTED_AREA_PROXY` 和 `BBDOWN_RESTRICTED_API_PROXY` 也接受逗号分隔列表。

如果通过 `auth import-access-key` 导入了通用 access key，代理 playurl 请求会以
`access_key` 包含它。Bilibili cookie 不会转发到受限区域代理主机。解析器诊断会记录官方失
败和代理尝试，但 endpoint 字段会压缩到 URL origin，敏感错误消息值会被脱敏，因此不会打
印 token 值。

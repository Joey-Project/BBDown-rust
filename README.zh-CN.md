[ [English](README.md) | 简体中文 ]

# BBDown Rust

`BBDown Rust` 是 BBDown 的 Rust 原生重写，目标有两个：

- 提供可被其他 Rust 项目复用的 `bbdown-core` package / `bbdown_core` crate；
- 提供一个 CLI，作为元数据解析和下载流程的端到端使用界面。

本项目以原 [BBDown](https://github.com/nilaoda/BBDown) 项目的 Bilibili 实用行为为重要
参考。感谢 BBDown 及其贡献者提供的参考。

当前实现已经建立 crate / CLI / CI 基础、元数据解析器、流规划、媒体下载、封面/字幕/弹幕
旁路文件下载、重试和断点续传、可选 `ffmpeg` 封装、二维码登录、可选 live 测试框架、带诊
断信息的受限区域代理排序、UPOS/PCDN 媒体 host 控制，以及 builder 风格的 crate 集成 API。
它还支持显式下载归档，用于重复下载预检查，以及 CLI 的 replace / keep-both / cancel 决
策。输入解析覆盖普通视频、PGC 和 intl 分集、PUGV/cheese 课程、B23 短链接、收藏夹、空
间投稿、合集和系列。URL 解析包括 canonical `bilibili.com/list/...` 页面、path-based
medialist 收藏夹 URL，以及带 uploader mid 的空间合集 / 系列 URL，以便使用较新的空间
API。

## 当前 CLI

解析元数据并输出 JSON：

```bash
bbdown info av170001 --json
bbdown info BV1qt4y1X7TW --json
bbdown info ep267851 --json
bbdown info ss26801 --select latest --json
bbdown info md22718131 --select latest --json
bbdown info https://b23.tv/example --json
bbdown info cheese/ep101 --json
bbdown info fav456 --json
bbdown info https://www.bilibili.com/list/ml1103407912 --json
bbdown info 'https://space.bilibili.com/123/lists/456?type=series' --json
```

生成下载计划并输出 JSON：

```bash
bbdown plan av170001 --json
bbdown plan ep267851 --json
bbdown plan ss26801 --select latest --json
bbdown plan https://www.bilibili.tv/en/play/34613/341736 --json
bbdown plan fav456 --select page:1 --json
bbdown plan cheese/ss202 --select latest --json
```

`plan` 会解析所选条目、可用 DASH 或 FLV 流 URL、字幕 URL，以及每个 `cid` 的弹幕 XML
URL。PGC 和 intl 规划仍可能需要符合条件的账号或区域访问。PGC playurl 解析可以回退到
用户配置的受限区域代理。它不会下载文件。集合类输入默认解析全部条目；使用
`--select page:<index>` 可规划一个集合条目，使用 `--select latest` 可规划上游列表顺序中的第一个解析条目。
`info --json` 会在 `collection.collection.items` 保留完整解析到的集合 metadata；`plan` 只
输出选中条目。

下载所选媒体文件：

```bash
bbdown download av170001 --output-dir downloads
bbdown download ss26801 --select latest --output-dir downloads
bbdown download fav456 --select page:1 --output-dir downloads
bbdown download av170001 --output-dir downloads --no-mux --json
bbdown download av170001 --only cover --output-dir downloads --json
bbdown download av170001 --output-dir downloads --archive-file downloads/archive.json --on-duplicate keep-both
bbdown download av170001 --upos-host upos-sz-mirrorcoso1.bilivideo.com --no-mux
```

`download` 会解析下载计划，下载第一组完整的 DASH 视频/音频，或下载 FLV 分段；默认写入
封面、字幕和弹幕旁路文件；通过 HTTP range 请求续传部分文件；对有限的瞬时失败进行有界重
试；在存在声明媒体大小时进行校验；拒绝不完整的媒体形态；除非传入 `--no-mux`，否则运行
`ffmpeg`。使用 `--no-cover`、`--no-subtitles` 或 `--no-danmaku` 可以跳过对应旁路文件。
弹幕输出默认是 XML；传入 `--danmaku-format ass` 会只生成 ASS，传入
`--danmaku-format xml,ass` 可同时保留 XML 和 ASS 旁路文件。
使用 `--only video`、`--only audio`、`--only subtitle`、`--only danmaku` 或
`--only cover` 可只写入一种输出；single-output 模式会跳过 mux。
CLI 默认会避开疑似 PCDN 媒体 URL，同时保留本地和私网 host。传入 `--allow-pcdn` 可保留
原始 PCDN 候选；传入 `--upos-host <HOST>` 可把 DASH/FLV 媒体候选改写到指定 UPOS host；
传入 `--force-replace-host` 可把媒体候选改写到内置 BBDown fallback host。封面、字幕和弹
幕旁路 URL 不会被改写。
传入 `--archive-file <path>` 后，CLI 会按内容身份记录已完成下载。归档输出、旁路文件和
mux 路径会在记录时保存为绝对路径，因此同一份归档可以从另一个工作目录复用。条目身份使
用稳定的 aid/cid 媒体 id，因此同一 PGC 分集即便之后通过 BV/av URL 规划，且其中一种形式
缺少 BVID，也仍能匹配。当同一内容、条目或归档输出目录再次出现时，非交互 JSON 模式需要
提供 `--on-duplicate replace`、`--on-duplicate keep-both` 或 `--on-duplicate cancel`；
交互式人类模式在没有决策时会提示。`replace` 会先删除已有的计划输出根目录，再重新下载，
并替换指向该输出路径的陈旧归档记录；`keep-both` 会写入下一个带后缀的输出根目录，并避
开所有归档记录输出路径；`cancel` 会只报告预检查状态，不下载。归档文件本身不能是所选输
出根目录，也不能位于该根目录之内；CLI 对归档保存旁路路径应用相同保护。如果归档文件是
符号链接，保存会更新符号链接目标，以免共享归档历史被拆分。

`ss` 和 `md` 输入在非交互模式下需要显式选择：

```bash
bbdown info ss26801 --select latest
bbdown info ss26801 --select all
bbdown info ss26801 --select episode:267851
bbdown info ss26801 --select page:1
```

`cheese/ss...` 输入遵循相同的显式选择规则。收藏夹、空间投稿、合集和系列是批量输入；
不传 `--select` 时会解析全部条目。

管理本地凭据：

```bash
bbdown auth import-cookie --stdin
bbdown auth import-cookie --file cookie.txt
bbdown auth import-access-key --stdin
bbdown auth login-web
bbdown auth login-tv
bbdown auth status
bbdown auth logout
```

凭据默认存储在平台配置目录。使用 `--credential-file <path>` 可以为集成测试或本地实验覆
盖该路径。没有输入标志时，密钥导入命令也会读取 `BBDOWN_COOKIE` 或
`BBDOWN_ACCESS_KEY`，这样调用方无需把凭据放到进程参数里。二维码登录命令会轮询
Bilibili 二维码状态机，并只保存最终得到的凭据。WEB 二维码登录保存 cookie；TV 二维码登
录保存 TV 专用 access key，不会覆盖通用 intl/Bstar access key。使用 `--json` 时，二维
码登录输出换行分隔 JSON 事件：先输出带扫码 URL 的 `ticket` 事件，再在凭据保存后输出
`saved` 事件。请把扫码 URL 当成临时登录密钥；状态输出和 `saved` 事件只暴露脱敏布尔值。

使用 `--request-timeout-seconds` 或 `BBDOWN_REQUEST_TIMEOUT_SECONDS` 调整 API 请求时限。
媒体正文读取使用 `--download-idle-timeout-seconds`；传入 `0` 可禁用 idle timeout。使用
`--comment-base` 或 `BBDOWN_COMMENT_BASE` 可以把弹幕 XML 下载指向 mock 或代理端点。
使用 `--passport-base` 可配置 WEB 二维码登录 mock 或代理；使用 `--tv-passport-base` /
`--tv-passport-poll-base` 可配置 TV 二维码登录 mock 或代理。只有在提供 TV 专用覆盖时，
TV 二维码轮询才会跟随 `--tv-passport-base`；否则除非显式设置 `--tv-passport-poll-base`，
它会使用上游 TV 轮询默认值。

通过显式代理主机配置受限区域 PGC playurl 回退。只有官方 PGC playurl 响应报告区域限制时
才会回退：

```bash
bbdown --restricted-area hk --restricted-area-proxy hk=https://proxy.example/playurl plan ep267851 --json
bbdown --restricted-api-proxy tw=https://proxy.example/bili/api plan ss26801 --select latest --json
```

`--restricted-area-proxy` 面向 BBDown/BiliPlus 风格的 HTTP(S) playurl 代理端点。
`--restricted-api-proxy` 面向镜像 `api.bilibili.com` 路径的 HTTP(S) 代理，并保留已存在
于代理 base URL 上的查询参数。可以重复传入标志，或使用逗号分隔的
`BBDOWN_RESTRICTED_AREA_PROXY` / `BBDOWN_RESTRICTED_API_PROXY` 值配置多个候选。重复命
令行标志会在同一区域优先级内保留声明顺序。当命令行和环境变量代理值同时存在时，先尝试
命令行候选，再尝试环境 playurl 代理，最后尝试环境 API-path 代理。每个来源组都会按区域
提示、通用候选、固定区域顺序排序。主机由用户提供；工具不内置公共代理默认值。代理请求
不会转发 Bilibili cookie。解析器诊断会把端点压缩到 URL origin，并脱敏错误消息里的敏感值。

## 发布构建

GitHub tag release 会通过两阶段 release candidate 和 promotion workflow 为 Linux x86_64、
macOS x86_64、macOS aarch64 和 Windows x86_64 构建预打包的 `bbdown` CLI 归档。手动
release artifact workflow 运行也可以构建相同归档，但不会发布 tag、GitHub Release 或
crate。每个归档包含 CLI 二进制、英文和简体中文 README、英文和简体中文用户指南、嵌入指
南、发布 runbook 和架构指南，以及 `LICENSE`。每个归档旁边还有 `.sha256` 校验文件。
maintainer 发布步骤见 [发布 Runbook](docs/release.zh-CN.md)。

## 开发命令

```bash
just fmt-check
just lint
just test
just e2e
just publish-dry-run
just publish-dry-run-strict
just live-e2e
just ci
```

本地默认 `just ci` 会运行格式检查、clippy、单元测试、mock e2e 测试，以及对可发布的
`bbdown-core` library package 执行 dirty-tree-friendly 的 crates.io dry run。GitHub CI
运行相同测试门禁，并在干净 checkout 上执行严格 crates.io dry run。CLI crate 不是 crates.io 发布目
标；二进制分发使用 GitHub release 归档。`just live-e2e` 被有意排除在默认 CI 之外，且在
被忽略的本地 `live-e2e.samples.json` 不存在时会快速失败。请从
`live-e2e.samples.example.json` 开始，把 `credential_file` 和 `access_key_file` 指向本地
密钥文件，并列出要探测的 public、PGC、intl 或 restricted PGC 样例。live 测试框架会为每
个 case 写入隔离的临时凭据存储，并在运行前移除 CLI 覆盖环境变量，因此样例行为由 manifest
驱动，而不是由 shell 状态驱动。

## 文档

- Crate API 说明：可发布 package 是 `bbdown-core`，导入时使用 `bbdown_core`。此重写处于
  已发布 `0.1.0` 之后的 `0.2` 开发线，因为批量 collection metadata 增加了新的
  `ResolvedContent::Collection` API 形态；嵌入项目应优先使用 `Default`、`new` 和
  `with_*` 构造器，例如 `ClientConfig::default().with_*`、`EndpointConfig::default().with_*`、
  `RestrictedAreaConfig::default().with_*`、`DownloadOptions::new(...).with_*` 和
  `RetryPolicy::new(...)`，而不是结构体字面量，这样新增配置字段时破坏性更小。输出模型
  结构体（例如 `DownloadEntry`）意图作为返回值消费或通过 serde 消费；结构体字面量构造
  不被视为稳定兼容面。对于重复处理，嵌入项目可以检查
  `DownloadPreflight`，把已有 `DownloadArchiveRecord` 展示给用户，然后传入显式
  `DuplicateDecision`。批量元数据通过 `ResolvedContent::Collection` 返回；下载规划会把
  选中的集合条目映射回普通视频的 stream planning 条目。
- 用户指南：[docs/user-guide.zh-CN.md](docs/user-guide.zh-CN.md)
- 嵌入指南：[docs/embedding.zh-CN.md](docs/embedding.zh-CN.md)
- 架构：[docs/architecture/rust-rewrite.zh-CN.md](docs/architecture/rust-rewrite.zh-CN.md)
- 英文 README：[README.md](README.md)
- 英文用户指南：[docs/user-guide.md](docs/user-guide.md)
- 英文嵌入指南：[docs/embedding.md](docs/embedding.md)
- 英文架构指南：[docs/architecture/rust-rewrite.md](docs/architecture/rust-rewrite.md)
- 面向 agent 的项目跟踪文档，不做本地化：[项目状态](docs/PROJECT_STATE.md)、
  [项目 TODO](docs/PROJECT_TODO.md)、[工作流日志](docs/project_journal/)。

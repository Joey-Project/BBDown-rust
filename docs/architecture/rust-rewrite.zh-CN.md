[ [English](rust-rewrite.md) | 简体中文 ]

# Rust 重写架构

## 目标

- 构建一个 Rust crate，让其他项目无需 shell out 到 CLI 即可嵌入。
- 保持 CLI 作为用户面工具和 e2e 测试表面。
- 保留 BBDown 的实用 Bilibili 知识，同时用 typed data 取代 CLI 日志解析。
- 支持普通视频、`ep`、`ss`、`md`、intl 分集，以及用户配置的受限区域解析器。

## 工作区

- `crates/bbdown`：以 `bbdown-core` 发布、在 Rust 代码中以 `bbdown_core` 导入的 library
  package，包含 typed input parsing、metadata models、credential store、client config 和
  resolver API。
- `crates/bbdown-cli`：只通过 crate public API 使用 crate 的 CLI wrapper。
- `docs/`：架构、面向用户说明、项目状态和项目日志条目。

## Public API 形态

可复用 crate 通过 `Default`、`new` 和 builder 风格 `with_*` 方法，让配置对嵌入者保持易用。
`EndpointConfig`、`ClientConfig`、`RestrictedAreaConfig`、`Credentials`、
`DownloadOptions`、`RetryPolicy`、`StreamSelection` 和 `MuxOptions` 都有构造路径，因此下游
项目在普通集成代码中不需要结构体字面量。CLI 使用相同 public builder，这让它成为 crate
API 的仓库内集成测试表面。

输出模型保持为 typed data surfaces。当 crate 仍处于预发布阶段时，调用方应读取字段或序列
化它们，而不是把输出结构体视为稳定的构造目标。

## 解析器模型

输入会规范化为 `Input`：

- `Aid` 和 `Bvid` 用于普通视频。
- `Episode`、`Season` 和 `Media` 用于 Bilibili PGC URL 和 id。
- `IntlEpisode` 用于 `bilibili.tv` 分集 URL。

library 会把元数据解析为 `ResolvedContent`：

- `VideoMetadata` 包含标题、描述、owner、tag、封面、发布时间和页面。
- `SeasonResolution` 包含 season metadata 和选中的分集集合。

library 会把媒体可用性解析为 `DownloadPlan`：

- `DownloadEntry` 记录选中的 `aid`、`bvid`、`cid`、可选 `epid`、标题和来源。
- `StreamSet` 保存 DASH video/audio 轨道、FLV 分段、原始 accepted quality id、结构化可选
  DASH 质量标签和时长。
- `StreamDiagnostics` 记录非默认解析尝试，例如受限区域代理回退。
- `SubtitleTrack` 记录语言元数据、规范化 URL 和基本格式分类。
- `DanmakuTrack` 记录从 `cid` 和配置的 comment endpoint base 推导出的 XML 弹幕端点。

`ss` 和 `md` 在非交互上下文中需要 `Selection`。CLI 未来会增加交互式提示，但 library 保
持契约显式，避免集成方意外下载整季。

## 流规划

`BiliClient::plan` 是从已解析 `Input` 构建 typed download plan 的 public crate API，不执行
文件 I/O。`BiliClient::plan_download` 保留为面向 CLI 风格调用方的 raw-string convenience
wrapper。规划当前支持三种官方来源模式：

- `NormalWeb` 用普通 web playurl 端点处理 `aid` / `bvid` 输入。
- `PgcWeb` 用 PGC web playurl 端点处理 `ep`、`ss` 和 `md` 输入。
- `IntlWeb` 用 BiliIntl mobile signing 参数调用 intl OGV playurl 端点，并在调用方配置时包
  含 access key。

字幕发现会跟随所选来源。普通和 PGC 条目使用 player subtitle endpoint。Intl 条目使用 intl
subtitle endpoint。字幕失败会按 BBDown 的实用行为视为可选轨道缺失，而 stream resolution
失败仍是 hard error。

Intl season metadata 可能返回 `code: 0`，同时包含 region-limit payload 且没有分集列表。解
析器会把它保留为 access-restricted error，而不是报告泛化的 selection failure。

CLI 通过 `bbdown plan` 暴露这一层。该命令被有意设计为规划表面：它打印 typed JSON 或简
短的人类摘要，但不会下载、合并或修改输出文件。

## 下载执行

`BiliClient::download_plan` 执行调用方提供的 `DownloadPlan`。`BiliClient::download` 和
`BiliClient::download_input` 是先规划再执行的 convenience wrapper。executor 返回 typed
`DownloadReport`，而不是抓取 CLI 输出。

执行行为由 `DownloadOptions` 控制：

- 输出目录；
- 有界重试策略；
- 可选 DASH video/audio stream id 选择；
- HTTP range resume 开关；
- 媒体读取 idle timeout；
- 是否包含字幕和弹幕旁路文件；
- 禁用 mux 或显式 `ffmpeg` 二进制路径。

对每个条目，执行优先使用 plan 中完整的 DASH 视频/音频组合。默认是第一个视频流和第一个
音频流；调用方可以设置 `StreamSelection::new(...)` 请求精确的 DASH video 或 audio stream
id。如果请求的 id 不可用，executor 会报告可用 id，并在媒体写入前失败。如果 DASH 媒体不
完整且 FLV `durl` 分段可用，则下载 FLV 分段；显式 stream selection 要求 DASH 媒体，因此
会拒绝 FLV 回退。否则条目会在媒体写入前失败。字幕和弹幕文件保持为 sidecar。当启用 mux
时，executor 使用显式 argv 调用 `ffmpeg`，并在 report 中返回命令和输出路径。

媒体和 sidecar 下载使用不含账号 cookie 的媒体 headers，因为媒体 URL 来自 API payload，
可能指向 CDN 或代理主机。DASH 和 FLV backup URL 是候选列表的一部分。媒体正文读取使用独
立 idle timeout，而不是 metadata request timeout。只有当 `Content-Range` 从本地文件长度
开始，并在 advertised range total 结束，或 expected media size 证明最终长度正确时，resume
才会 append；匹配的 416 响应会被视为已经完成。当没有 expected size 时，会拒绝 wildcard
`Content-Range` total。当 stream 或 FLV segment 声明 size 时，executor 会拒绝最终文件长
度不匹配，并把失败写入回滚到尝试前长度。没有写入任何字节却完成的媒体响应会被拒绝。条
目目录包含内容身份，因此同标题视频不会共享 resume target；字幕 sidecar 名称包含 track
身份；文件名组件按 UTF-8 字节长度限制。如果服务器忽略 `Range` 并对 partial file 返回
`200 OK`，executor 会把完整重试写入临时文件，并只在可用校验通过后替换旧 partial。没有
advertised size、`Content-Length` 或 `Content-Range` 时，完整重试会被拒绝并保留旧文件。
强制 fresh write 在替换已有目标时也使用临时文件，因此失败的 `--no-resume` 重试不会清空
已有输出。DASH 媒体输出名优先使用稳定 stream metadata，只在 metadata 缺失时回退到 URL
path hash，因此 CDN host 或 query 变化不会拆分 resume target。

重复处理在执行前建模，而不是隐藏在 downloader 内部。`DownloadArchive` 按内容身份保存已
完成输出记录，不含媒体 URL 或凭据，并在完成时把输出、sidecar 和 mux 路径记录为绝对路径。
`DownloadPreflight::inspect` 报告 content/archive 命中、same-output 归档记录和计划输出目
录冲突，因此嵌入应用可以展示已有内容并选择 `DuplicateDecision`。`Replace` 会先删除已有
的计划输出根目录，再重新下载，然后替换该输出路径的陈旧归档记录。`KeepBoth` 会写入下一
个带后缀的输出根目录，并避开所有归档记录输出路径；比较使用 normalized output path key，
而不是原始 `PathBuf` 相等。这些 key 会在折叠 parent 组件前解析已有符号链接前缀，与归档
记录和 CLI overlap guard 的文件系统路径解析保持一致。`DownloadPreflight` 会序列化其保留
输出路径，因此嵌入应用可以在执行 `KeepBoth` 决策前 round-trip preflight state，而不会丢
失 archive-only 输出保留；执行会在应用决策前校验 preflight 仍匹配当前归档。entry-level
archive identity 使用稳定 aid/cid content id，而不是展示 index、可选 BVID 或可选 epid，因
此重新排序的页面和 episode-vs-BV URL 形态仍能检测为重复。
`Cancel` 是调用方层面的停止决策。CLI 通过 `--archive-file` 和 `--on-duplicate` 暴露同一
模型，拒绝与所选输出根目录重叠的 archive file path（同时检查 lexical path 和 canonical
target），并对 archive save sidecar path 应用相同保护。JSON/非 TTY 模式要求显式决策，不
会提示。展示 preflight state 后，CLI 会用同一 preflight 执行，因此如果输出根目录在
preflight 和执行之间出现，无冲突默认值不会被升级为隐式 replace；保存归档前，它也会再次
根据实际输出目录检查 archive-file guard。`DownloadArchive::save` 写入前拒绝目录目标；当
archive path 是符号链接时，它会写入符号链接目标，使共享归档保持一份历史。
Output-root occupancy checks 使用 symlink metadata，因此 stale 或 broken symlink root 会与
replacement cleanup 一致处理；metadata error（例如不可访问 parent）会报告给调用方，而不
是永远重试为带后缀输出根目录。

crate 默认禁用 mux，因此嵌入项目不会意外启动外部进程。CLI `download` 命令默认启用
ffmpeg，并为用户和 mock e2e 测试暴露 `--no-mux`。Mux subprocess 的 stdin、stdout 和
stderr 与 CLI stdio 隔离。Mux 会先写入临时输出，校验后再替换最终文件，因此失败的 rerun
会保留已有 mux 文件，并保持 JSON report 可解析且准确。

## 受限区域和 Intl

项目不能硬编码公共代理服务。受限区域支持被设计为一个用户配置的解析链：

- 官方 web 和 PGC API；
- 在有调用方提供的 access key 时使用 intl API；
- 用户配置的 BBDown/BiliPlus 风格 playurl 代理主机；
- 用户配置的镜像 `api.bilibili.com` 路径的代理；
- 用户配置的区域提示，例如 `cn`、`hk`、`tw` 或 `th`。

`ClientConfig::restricted_area` 保存 per-client `RestrictedAreaConfig`。嵌入者可以通过
`RestrictedAreaConfig::new`、`RestrictedAreaConfig::default().with_area_hint(...)`、
`with_proxy(...)` 或 `with_proxies(...)` 设置可选区域提示和 `RestrictedAreaProxy` 候选列
表。候选排序遵循 bilibili-helper 思路，但不使用浏览器本地缓存：匹配区域提示优先，其次
通用候选，然后是 `cn`、`th`、`hk` 和 `tw`，并移除重复的 `(base_url, area, kind)` 候选。
CLI 创建的配置还会在区域分组前保留来源优先级，因此显式命令行代理候选会先于环境变量派
生的代理候选尝试。

PGC stream planning 首先调用官方 PGC web playurl 端点。如果响应明确报告区域限制，且配置
了受限区域代理，client 会按顺序尝试候选，直到某个候选返回有效 DASH 或 FLV stream 形态。
非区域类官方失败会保留原错误，不会联系代理主机。BBDown/BiliPlus 风格的 HTTP(S) playurl
代理会在配置 URL 上接收 PGC playurl query。Bilibili API HTTP(S) 代理会在配置 base URL 下
的 `/pgc/player/web/playurl` 接收同一 query，以匹配常见 BALH 风格 API 代理主机，然后再尝
试 `/pgc/player/web/v2/playurl`，兼容已有 API proxy 部署。两条路径都会保留配置 base URL
上已存在的 query 参数。代理 playurl 响应可以使用官方 `data` / `result` wrapper，也可以
使用较老 helper 形态，把 `dash` / `durl`、`timelength` 和质量元数据返回在顶层。对这些顶
层 helper payload，legacy 字符串状态字段（例如 `result: "suee"`）会被容忍。
当 `Credentials::access_key` 中存在通用 access key 时，代理请求会把它作为 `access_key`
包含；TV 专用 access key 不会复用到这个流程。Bilibili cookie 会有意从受限区域代理请求
中省略。

代理回退成功时，`DownloadEntry.source` 为 `PgcProxy`，`DownloadEntry.diagnostics` 包含
官方失败尝试和成功代理尝试。当所有候选失败时，返回的 access-restricted error 会摘要有序
尝试。诊断 endpoint 字段会压缩到 URL origin，以免打印 path/query/userinfo 密钥；诊断错
误消息也会在通过 JSON 或最终错误暴露前脱敏 URL token 和常见敏感 key-value 模式。

当前实现支持端点覆盖、intl metadata 形态、官方 PGC stream planning、官方 intl OGV 签名
stream planning、配置化 PGC proxy fallback、顶层 helper playurl 响应解析、typed source
reporting、resolver diagnostics 和下载执行。浏览器专用 mobile response rewriting 有意不
在范围内。

## 凭据

CLI 会把凭据存储在平台配置目录下的本地 JSON 文件中，并在 Unix 上使用 `0600` 权限。crate
暴露 `Credentials` 和 `CredentialStore`，让其他项目可以注入自己的存储或把凭据保存在内存
里。

二维码登录在 crate 中建模为显式状态机。WEB 二维码登录创建 `QrLoginTicket`，轮询
waiting-for-scan、waiting-for-confirmation、expired 和 succeeded 状态，然后返回 cookie 凭
据。TV 二维码登录使用 BBDown-compatible app signed form flow，并返回 TV 专用 access-key
凭据。这与通用 intl/Bstar `access_key` 分离，因为 Bilibili app token 绑定 appkey。WEB QR
成功优先使用响应 `Set-Cookie` headers，并回退到从 cross-domain success URL 提取
BBDown-compatible cookie。TV auth-code 创建和 TV polling 分别可配置，因此测试和受控代理
可以镜像上游 split-host 流程或单一本地端点。TV ticket 会保留生成的 device session context，
使 polling 复用同一 device identity。二维码登录请求即使 client 存有凭据，也使用 anonymous
headers。CLI `auth login-web` 和 `auth login-tv` 命令会在 succeeded state 后重新加载当前
本地 credential store，再合并返回凭据，因此长时间二维码等待不会用等待前的陈旧快照覆盖另
一个命令的凭据更新。

密钥永远不会包含在状态输出中；`auth status` 和二维码登录 `saved` JSON 输出只报告布尔值。
二维码登录 `ticket` 事件和面向人类的扫码输出会有意暴露扫码 URL，方便用户认证；调用方应
把该 URL 视为临时登录密钥。public QR state enum 有意不 derive serde traits，因为 succeeded
state 携带完整凭据，供自行处理存储的嵌入调用方使用。QR ticket debug 输出会脱敏，因为
ticket key 和扫码 URL query string 可作为预认证密钥。HTTP request error 转换时不会保留完
整 URL，因此 intl `access_key` 等 query 密钥不会出现在面向用户的错误中。

## 测试和 CI

默认 CI 是确定性的：

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- 使用 `cargo +1.95.0 check --workspace --locked` 检查 declared MSRV
- 单元测试
- CLI mock e2e 测试
- 对可发布 `bbdown-core` library package 执行 crates.io dry-run packaging

Release packaging 是单独的 GitHub Actions workflow stack。`Release Artifacts` 是可复用且
仅手动/被调用的 workflow：它会构建 Linux x86_64、macOS x86_64、macOS aarch64 和 Windows
x86_64 CLI 归档，但不发布 tag、GitHub Release 或 crate。`Create Release Candidate` 会验
证 repository default branch、构建这些归档，并通过 release GitHub App 创建 annotated
`vX.Y.Z-rc.N` tag，但会先拒绝已经存在 final tag 或 GitHub Release 的 version。Create
workflow 也会在真正写入 RC tag 前重复 final tag 和 GitHub Release 检查。`Promote Release
Candidate` 必须从请求版本的最新 RC tag 运行；它会重新验证、重新构建正式归档，在
发布前再次确认选中的 RC 仍然最新，创建正式 annotated `vX.Y.Z` tag，发布 GitHub Release，
然后通过受保护的 `crates-io` environment 发布 `bbdown-core` 到 crates.io。RC 创建和
promotion 共用按 version 分组的 concurrency group，因此同一 version 正在 promotion 时不会
再创建更高编号 RC。归档包含 `bbdown` 二进制、英文和简体中文 README、英文和简
体中文用户指南、嵌入指南、发布 runbook 和架构指南，以及 `LICENSE`。每个归档旁边也有平
台专用 checksum 文件。GitHub Release notes 会从上一条非 RC 正式 release tag 生成，避免
把 RC tag 当成正式 release 的比较起点。promotion 也支持 GitHub Release 创建被中断后的重
试：draft release 会被删除并重新创建，已发布 release 只有在预期 asset 集合已经完整时才
会被复用，并且必须匹配 `uploaded` 状态、字节大小和 SHA-256 digest。workflow 会按
`tag_name` 列出 releases，而不是只依赖仅面向 published release 的 tag endpoint，因此
release GitHub App token 能看到 draft release。crates.io publish step 会先检查 exact
`bbdown-core` version；如果匹配版本已经发布，则把它视为恢复成功，以覆盖 upload 已被接
受但 runner 失败的情况。release workflows 使用 GitHub-hosted runner 自带的 `rustup` 和
`rust-toolchain.toml` 中的 floating stable Rust channel；有意不使用第三方 Rust toolchain
或 cache actions。它们也会安装 Rust 1.95.0 来运行与 crate `rust-version` metadata 匹配的
`cargo check` gate。包名会把 release ref 规范化到打包器安全的 `[A-Za-z0-9._-]` 字符集，
因此 SemVer build metadata 等 tag 不会导致打包失败。

Crate publishing 被有意限制在可复用 `bbdown-core` library package，在 Rust 代码中导入为
`bbdown_core`。该 crate 有 crates.io metadata、package-local README 和 LICENSE、
dirty-tree-friendly 的本地 publish dry-run validation，以及 CI 支持的
`cargo publish --dry-run -p bbdown-core --locked` validation。`bbdown-cli` 保持
`publish = false`，因为 CLI 分发通过 GitHub release 归档处理。

Plan output 现在暴露结构化 stream quality data。library 保留原始 `StreamSet::accept_quality`
以兼容，并增加 `StreamSet::qualities`，其中包含实际可选 DASH video id 和从
`accept_description`、`support_formats` 派生的可选 label。CLI 人类摘要会在 video/audio
stream 摘要旁打印相同 id，而 JSON 调用方可以通过 `DownloadOptions::stream_selection` 选
择精确 DASH stream。

可复用 crate 仍在准备第一次 crates.io 发布，因此此分支会在发布前刻意加固 public structs，
而不是保留本地预发布 struct-literal 实验。嵌入者应通过 constructor 和 builder API 创建配
置，包括 `ClientConfig::default().with_*`、`EndpointConfig::default().with_*`、
`RestrictedAreaConfig::default().with_*`、`DownloadOptions::new(...).with_*`、
`RetryPolicy::new`、`StreamSelection::new`、`StreamSelection::video` 和
`StreamSelection::audio`。`StreamSet` 和 `StreamQuality` 等 public output containers 标记为
non-exhaustive，因为 plan models 是被消费的数据表面，会在 crate 成熟过程中新增字段。

下载归档和重复处理在 crate 与 CLI 层都有覆盖。单元测试覆盖 preflight archive/output 冲突
检测、entry-level archive overlap 检测、replace 在 fresh write 前删除陈旧输出根目录 artifact、
keep-both 带后缀输出根目录，以及 archive JSON round trip/replacement 不含媒体 URL。它们
还覆盖 archive-only keep-both path reservation、无关 archive-only output path reservation、
same-output archive record replacement、display-index-insensitive entry archive identity、
broken-symlink output roots、metadata error reporting、preflight JSON round-trip reservation
preservation、stale archive/preflight rejection、episode-vs-video entry identity、symlink
archive target saves 和 directory-target archive save rejection。CLI mock e2e 测试覆盖没有
显式决策时的 JSON duplicate failure、`cancel` preflight output、`keep-both` 带后缀输出根
目录、`replace` 覆盖已有文件、symlink archive target updates，以及拒绝与所选输出根目录按
lexical path 或 canonicalized targets 重叠的 archive file path，包括 archive save sidecar paths。

对 Bilibili 的 live tests 只通过 `just live-e2e` 可选运行。recipe 在被忽略的
`live-e2e.samples.json` manifest 不存在时会快速失败，因此分支 CI 不受网络、账号或区域状
态阻塞。已跟踪的 `live-e2e.samples.example.json` 记录 manifest 形态。每个 live case 可以
对 normal、PGC、intl 或 restricted PGC 输入运行 `info`、`plan` 或两者；可以设置
case-specific selection 和 area hint；也可以断言预期 JSON kind、allowed 或 required plan
sources、minimum entry count 和 stream presence。Restricted PGC case 可以显式允许
access-restricted plan failure，并要求诊断片段。manifest parser 会拒绝未知字段，因此
expectation 拼写错误不会静默禁用 source 或 diagnostic assertions。测试框架会从配置的
credential 和 access-key 文件为每个 case 写入临时 credential store，移除 CLI override 环境变
量，并把 all-area restricted proxy shortcuts 展开为固定 `cn`、`th`、`hk`、`tw` 顺序。网络
请求可通过 `ClientConfig` 和 CLI/settings 配置 timeout，因此异常官方或代理端点不会无限挂
起。

## 已规划 PR 切片

1. Workspace、CI、docs、metadata resolver、credential store 和 CLI `info/auth`。已在 PR #1
   完成。
2. Stream resolver chain、download planning、subtitle 和 danmaku discovery。已在 PR #2
   完成。
3. File download、retry/resume policy、ffmpeg mux integration 和 mock e2e downloads。已在
   PR #3 完成。
4. QR login state machine 和 live-test opt-in harness。已在 PR #4 完成。
5. Restricted-area proxy resolver ordering 和 diagnostics。已在 PR #5 完成。
6. Manifest-driven local live e2e sample matrix。已在 PR #7 完成。
7. GitHub binary release packaging。已在 PR #8 完成。
8. Crate publish readiness 和 dry-run validation。已在 PR #9 完成。
9. Clearer stream quality selection 和 listing support。已在 PR #10 完成。
10. Restricted-area proxy response compatibility expansion。已在 PR #11 完成。
11. Integration API 和 documentation hardening。已在 PR #12 完成。
12. Download archive 和 duplicate decision handling。已在 PR #13 完成。

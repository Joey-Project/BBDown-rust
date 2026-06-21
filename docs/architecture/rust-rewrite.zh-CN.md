[ [English](rust-rewrite.md) | 简体中文 ]

# Rust 重写架构

## 目标

- 构建一个 Rust crate，让其他项目无需 shell out 到 CLI 即可嵌入。
- 保持 CLI 作为用户面工具和 e2e 测试表面。
- 保留 BBDown 的实用 Bilibili 知识，同时用 typed data 取代 CLI 日志解析。
- 支持普通视频、`ep`、`ss`、`md`、intl 分集、PUGV/cheese 输入、批量集合和 feed/list 输
  入、B23 短链接，以及用户配置的受限区域解析器。

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

输出模型保持为 typed data surfaces。当前 crate 版本是 `0.5.0`，属于已发布 `0.4.0` 之后
的开发线，重点是 downloader 和 embedding polish：progress callback、可取消的执行、章节
metadata mux、音频语言选择，以及 AI 字幕筛选；调用方应读取字段或序列化输出值，而不是把输出结构体视为稳定的构造目标。

## 解析器模型

输入会规范化为 `Input`：

- `Aid` 和 `Bvid` 用于普通视频。
- `Episode`、`Season` 和 `Media` 用于 Bilibili PGC URL 和 id。
- `CheeseEpisode` 和 `CheeseSeason` 用于 PUGV/cheese 课程。
- `IntlEpisode` 用于 `bilibili.tv` 分集 URL。
- `SpaceVideos`、`FavoriteList`、`CollectionList`、`SeriesList`、`SpaceCollectionList` 和
  `SpaceSeriesList` 用于批量内容。owner-scoped 空间合集 / 系列 variant 会保留 canonical
  URL 中的 uploader mid，以便直接调用较新的空间 API。
- `RecommendationFeed` 用于 `recommendations` shorthand 或 B 站首页 URL 的首页推荐批次。
- `History` 用于 `history` shorthand 或 `/account/history` 页面上的登录态观看历史输入。
- `WatchLater` 用于稍后再看 shorthand、`/watchlater` 或 `/list/watchlater` 页面上的登录态稍后再看输入。
- `FollowingFeed` 和 `SpaceDynamic` 用于登录态关注动态视频 feed，或单个 UP 主的动态页面。
- `ShortLink` 用于 B23 链接，会先通过 HTTP redirect 解析，再进入普通输入分发。

library 会把元数据解析为 `ResolvedContent`：

- `VideoMetadata` 包含标题、描述、owner、tag、封面、发布时间和页面。
- `SeasonResolution` 包含 season metadata 和选中的分集集合。
- `VideoCollectionResolution` 包含 collection metadata，以及收藏夹、空间投稿、合集、系
  列、首页推荐、观看历史、稍后再看、关注 feed 和空间动态 feed 中选中的条目集合。收藏夹解析支持
  shorthand id、path-based medialist 页面和 canonical `/list/ml...` 页面。即使 selector 缩
  小了 `selected_items`，`resolve_input` 也会保留完整解析到的 collection metadata。

collection-like 页面族共享的 feed/list 行为位于内部 `feed_list` resolver 层。它负责
selection 校验、page/range fetch-mode 计算、按 identity 去重，以及一基 item 重编号。现有
public collection 输出形状保持不变；history、recommendation、watch-later、following/UP
页面等页面族会在这一层之上增加各自的页面 fetcher，而不是重新实现 selection 和分页规则。
当前 history、recommendation、watch-later 和 dynamic feed 页面族已经接入。

library 会把媒体可用性解析为 `DownloadPlan`：

- `DownloadEntry` 记录选中的 `aid`、`bvid`、`cid`、可选 `epid`、标题和来源。批量集合条
  目在 stream planning 前会映射回普通视频条目。
- `StreamSet` 保存 DASH video/audio 轨道、FLV 分段、原始 accepted quality id、结构化可选
  DASH 质量标签和时长。
- `StreamDiagnostics` 记录非默认解析尝试，例如受限区域代理回退。
- `SubtitleTrack` 记录语言元数据、规范化 URL、基本格式分类，以及上游存在时的 AI 字幕
  metadata。
- `ChapterTrack` 在上游播放器 metadata 暴露可用章节边界时记录标题和开始/结束秒数。
- `DanmakuTrack` 记录从 `cid` 和配置的 comment endpoint base 推导出的 XML 弹幕端点。

`ss`、`md` 和 `cheese/ss` 在非交互上下文中需要 `Selection`。批量集合和 feed/list 输入默
认选择全部解析条目；调用方可以传入 `Selection::Page(...)` 选择一个条目，传入
`Selection::Indices(IndexSelection)` 对条目 index 做列表/范围选择，或传入
`Selection::Latest` 选择上游列表顺序中的第一个解析条目。同一个 index selection 表面也适
用于普通视频分 P 和 season 分集序号；`Selection::Episode(...)` 则继续表示精确 PGC
episode id。空批量集合在默认/all selection 下会解析为空 selected item 列表。因为
`DownloadPlan` 不暴露 collection metadata，`plan_download` 可以只抓取覆盖所选最大 index
所需的批量条目。推荐输入使用 WEB 首页推荐端点，当前只输出普通视频 `av` 卡片，并在非视频
卡片被跳过后为显式 index selection 在安全上限内继续请求后续 `fresh_idx` 刷新批次。观看历
史输入使用 WEB history cursor 端点，需要已认证 cookie，且当前只保留可以通过普通视频
pipeline 规划的普通视频 `archive` 记录。稍后再看输入使用 WEB toview 端点，需要已认证
cookie，并输出该账号稍后再看列表中的普通视频。关注和空间动态输入使用 WEB dynamic feed 端点，也
需要已认证 cookie，且当前只输出普通视频 archive 卡片。CLI 未来会增加交互式提示，但
library 保持 season-like 契约显式，避免集成方意外下载整季。

Mode-aware planning 使用同一套 resolver 分发，但 sidecar-only mode 会跳过媒体 stream 解
析。当调用方需要为 archive preflight 或 UI 决策生成非默认 `DownloadMode` 的 plan 时，使
用 `plan_download_with_mode` 或 `plan_with_download_mode`，保证 plan 形状和后续 download
options 一致。

## 流规划

`BiliClient::plan` 是从已解析 `Input` 构建 typed download plan 的 public crate API，不执行
文件 I/O。`BiliClient::plan_download` 保留为面向 CLI 风格调用方的 raw-string convenience
wrapper。规划当前支持这些官方来源模式：

- `NormalWeb` 用普通 web playurl 端点处理 `aid` / `bvid` 输入。
- `NormalTv` 在 `ClientConfig.playurl_mode` 为 `PlayurlMode::Tv` 时，用 BBDown-compatible
  TV HTTP playurl 端点处理 `aid` / `bvid` 输入。
- `NormalApp` 在 `ClientConfig.playurl_mode` 为 `PlayurlMode::App` 时，用 BBDown-compatible
  APP gRPC playurl 端点处理 `aid` / `bvid` 输入。
- `PgcWeb` 用 PGC web playurl 端点处理 `ep`、`ss` 和 `md` 输入。
- `PgcTv` 在 `ClientConfig.playurl_mode` 为 `PlayurlMode::Tv` 时，用 BBDown-compatible
  TV HTTP playurl 端点处理 PGC 输入。
- `PgcApp` 在 `ClientConfig.playurl_mode` 为 `PlayurlMode::App` 时，用 BBDown-compatible
  APP PGC gRPC playurl 端点处理 PGC 输入；restricted 或 preview-only 信号仍可回退到已配置的
  restricted-area HTTP playurl proxy。
- `PugvWeb` 用 PUGV/cheese playurl 端点处理 `cheese/ep` 和选中的 `cheese/ss` 输入。
  PUGV metadata 会通过 episode-list 端点跟进 `episode_page` 分页，再应用 season
  selection。
- `IntlWeb` 用 BiliIntl mobile signing 参数调用 intl OGV playurl 端点，并在调用方配置时包
  含 access key。

字幕发现会跟随所选来源。普通和 PGC 条目使用 player subtitle endpoint。Intl 条目使用 intl
subtitle endpoint。字幕失败会按 BBDown 的实用行为视为可选轨道缺失，而 stream resolution
失败仍是 hard error。

Intl season metadata 可能返回 `code: 0`，同时包含 region-limit payload 且没有分集列表。解
析器会把它保留为 access-restricted error，而不是报告泛化的 selection failure。

CLI 通过 `bbdown plan` 暴露这一层。该命令被有意设计为规划表面：它打印 typed JSON 或简
短的人类摘要，但不会下载、合并或修改输出文件。

## 播放集成契约

`BiliClient::plan_playback` 和 `BiliClient::plan_playback_input` 会从与 `DownloadPlan` 相
同的 resolver 路径构建 `PlaybackPlan`。这个表面面向需要媒体请求、但不希望调用文件下载
器的下游播放器、缓存服务器和 API 层。playback entry 会保留所选 aid/bvid/cid/epid、标题、
来源、质量标签、时长，以及一组 playback variants。DASH variant 携带选中的 video/audio
`MediaRequestSpec`；FLV variant 携带有序的 segment spec。

`MediaRequestSpec` 被设计为可序列化且与传输层解耦。它包含主 URL、备用 URL、媒体
headers、mime type、已知时的 exact codec 字符串、codec-family metadata、可选音频语言
metadata、码率、尺寸、时长、大小和结构化 cache key。cache key 基于内容身份、媒体种类、stream id、存在时的 exact codec
字符串，以及去掉 fragment 但保留 query 身份的 source URL hash。这样既避免暴露 URL 明文，
也避免让 query string 区分资源的 proxy URL 发生碰撞。
playback planning 还会暴露 `PlaybackEntry.cache_key`、`PlaybackVariant.cache_key`、
`PlaybackEntry.abr.groups` 和 `PlaybackVariant.abr`，让下游 cache server 可以按 request
key 存储媒体、按 variant key 保留已完成 variants，并把 ABR level 切换映射回同一个
codec/mime-compatible switching group，避免重新获取已经缓存过的 levels。
`PlaybackVariant.selection_hints.avplayer` 增加了面向 AVPlayer 的 profile，包含已知时的
exact codec 字符串、codec family、`format_key`、score/preferred 信号和机器可读 reason。
公开的 `PlaybackCodecPreference` helper 允许下游按自己的 H.264、HEVC、AV1 或其它 codec
顺序给 variants 排名，而不是接受写死的 H.264-first 策略。APP/gRPC streams 会把数值 codec id
暴露为 family metadata，不会伪造 exact MP4 codec 字符串。
同一 planning 路径会遵守 `PlayurlMode::Tv` 和 `PlayurlMode::App`，因此 `DownloadPlan` 和
`PlaybackPlan` 可以暴露 `NormalTv`、`PgcTv`、`NormalApp` 或 `PgcApp` source，同时不改变下
游 request-spec shape。TV mode 使用 `Credentials::tv_access_key`。APP/gRPC mode 优先使用
`Credentials::tv_access_key`，再回退到 `Credentials::access_key`，发送 BBDown-compatible
protobuf gRPC frame，会从 initial headers 和 trailing metadata 读取 gRPC status，并把 APP
DASH/FLV 响应规范化为 `StreamSet`。APP DASH 的 width、height 和 frame-rate metadata 会
保留在 HTTP playurl mode 共用的 `MediaStream` 字段上。APP legacy FLV 响应可能包含多个清
晰度的 segment set；由于 `StreamSet::flv_segments` 是单个有序列表，normalizer 会保留最
高质量的一个 FLV candidate，而不是拼接互不兼容的清晰度。

本仓库不实现播放器运行时职责。下游 cache/player service 负责 `preparing`、
`playback_ready`、`downloading`、`completed`、`failed` 等 task state；HLS session 目录、
playlist、segment 文件、retention、cleanup 和 finalization；例如
`/tasks/{id}/hls/master.m3u8` 的 HTTP serving；下载中的 AVPlayer-compatible event playlist
和完成后的 VOD playlist；以及把完成的 HLS 或 remux 后 MP4 artifact 注册为 library item。
后续 crate 工作可以增加更丰富的 device policy profiles，但不应把这些运行时职责移入
`bbdown-core`。

## 下载执行

`BiliClient::download_plan` 执行调用方提供的 `DownloadPlan`。`BiliClient::download` 和
`BiliClient::download_input` 是先规划再执行的 convenience wrapper。executor 返回 typed
`DownloadReport`，而不是抓取 CLI 输出。
Progress 是 opt-in 的执行侧 observer。`*_with_progress` 变体接收
`DownloadProgressSink`，并在 plan、条目、文件和 mux milestone 发出 typed
`DownloadProgressEvent`。非 progress 方法会保留，并走 no-op sink。CLI 只有在传入
`--progress-json` 时才暴露同一事件流，并把 JSON Lines 写到 stderr，因此 stdout 仍然是人类
输出或最终 `DownloadReport` JSON。
取消对嵌入调用方同样是 opt-in 的。`*_with_cancellation` 和
`*_with_progress_and_cancellation` 变体接收 `DownloadCancellationToken`，调用方可以从另一
个 task 取消该 token。executor 会在规划、条目边界、sidecar 生成、retry sleep、HTTP
request/response streaming 和 muxing 过程中检查 token。取消会返回 `Error::Cancelled`，并
发出与 archive duplicate 取消相同的 plan-level `PlanCancelled` 终态 progress event。已经完
成的条目保持不变；新创建的部分文件会被删除，续传文件会截断回本次尝试前的大小。CLI 为
下载执行时的 `Ctrl-C` 安装同一个 token，因此终端用户和嵌入调用方共享同一套取消语义。
交互式 archive duplicate prompt 是 CLI 侧例外：终端 `stdin` 输入不能像 executor task 一样回
滚，所以在该提示中按 `Ctrl-C` 会立即以 130 退出。

执行行为由 `DownloadOptions` 控制：

- 输出目录；
- 输出根目录、条目目录和 mux 文件名 stem 的路径模板；
- 有界重试策略；
- 可选 DASH video/audio stream id 选择；
- all-output 或 single-output 下载模式；
- HTTP range resume 开关；
- 媒体读取 idle timeout；
- 是否包含封面、字幕和弹幕旁路文件；
- 弹幕旁路格式集合（`xml`、`ass` 或 `xml,ass`）；
- media host 替换和 PCDN 处理策略；
- 禁用 mux 或显式 `ffmpeg` 二进制路径。

对每个条目，执行优先使用 plan 中完整的 DASH 视频/音频组合。默认是第一个视频流和第一个
音频流；调用方可以设置 `StreamSelection::new(...)` 请求精确的 DASH video 或 audio stream
id，也可以附加 `StreamSelection::with_audio_language(...)` 来选择第一条 `MediaStream.language`
或 `language_doc` 匹配的音频流。如果请求的 id 或语言不可用，executor 会报告可用 id 或语
言，并在媒体写入前失败。如果 DASH 媒体不完整且 FLV `durl` 分段可用，则下载 FLV 分段；
显式 stream selection 要求 DASH 媒体，因此会拒绝 FLV 回退。否则条目会在媒体写入前失败。
封面、字幕和弹幕文件保持为 sidecar。当启用 mux 时，executor 使用显式 argv 调用 `ffmpeg`，
并在 report 中返回命令和输出路径。如果 plan 条目携带章节，ffmpeg mux 会加入临时 ffmetadata 输入，从该输入映射章节，并用
`MuxReport::chapter_count` 报告写入数量。

Plan 条目保留 canonical 弹幕 XML 端点；只有当执行层选择的 `DanmakuFormats` 集合包含
`DanmakuFormat::Ass` 时，executor 才会把 XML 转为 ASS。ASS 生成支持常见滚动、顶部、底
部和反向滚动弹幕，并会跳过高级定位弹幕，避免写出误导性的坐标。

Append-only 弹幕刷新被建模为单独的 archive-backed 执行路径，而不是普通下载的隐式副作用。
`BiliClient::update_danmaku_for_archive` 接收新的 `DownloadPlan`、可变 `DownloadArchive` 和
`DanmakuUpdateOptions`；它用稳定 aid/cid 身份匹配已有归档条目，下载当前 XML payload，只
把新的弹幕 block 合并进 canonical `danmaku.xml`，再从合并后的 XML 重新生成所选派生格式，
例如 ASS。底层 `merge_xml_append_only` helper 也是 public API，供自行管理 sidecar storage
且不使用 `DownloadArchive` 的调用方复用。

输出命名由 `DownloadPathTemplates` 驱动。输出根目录模板从 plan context 渲染；条目目录
和 mux 文件名 stem 模板从 entry context 渲染。渲染结果会作为单个文件名组件清洗，因此模
板不能注入嵌套路径。媒体、封面、字幕和弹幕旁路文件名会继续由 metadata 生成并保持稳定，
以支持续传行为、重复轨道命名和归档路径记录。重复 preflight 会比较后续执行使用的同一份
`DownloadOptions` 渲染出的计划输出目录。executor 会拒绝让同一计划中多个条目渲染到同一
目录的 entry 模板，因为共享条目目录会让续传和 sidecar 输出变得歧义。

`DownloadMode` 将默认 all-output 路径和单独输出 workflow 分开。`VideoOnly` 和 `AudioOnly`
只下载一个匹配的 DASH stream，并跳过 sidecar 和 mux。`SubtitleOnly`、`DanmakuOnly` 和
`CoverOnly` 会跳过媒体要求，只写入请求的 sidecar family，并拒绝 stream quality 选择，因
为这些模式不选择媒体 stream。
single-output 下载的 archive content key 也会包含 mode，而完整下载继续保留 legacy key；
因此现有归档仍能匹配完整下载，single-output 记录不会宣称完整条目已经下载完成。
显式 stream selection 会把 stream token 写入 archive key，避免不同清晰度或音频语言互相满
足 duplicate preflight。非默认 subtitle AI policy 也会写入 archive-key token，因为它会改变
要下载的字幕旁路文件集合。

媒体和 sidecar 下载使用不含账号 cookie 的媒体 headers，因为媒体 URL 来自 API payload，
可能指向 CDN 或代理主机。DASH 和 FLV backup URL 是候选列表的一部分。媒体正文读取使用独
立 idle timeout，而不是 metadata request timeout。`DownloadPlan` 保留上游媒体 URL；
`MediaHostOptions` 只在 executor 构建具体 DASH/FLV 候选列表时应用。配置了 `upos_host`
会改写全部媒体候选，`force_replace_host` 会改写到内置 BBDown fallback host；CLI 默认只
会在未设置 `--allow-pcdn` 时改写 PCDN-like 的非本地候选。Sidecar URL 不会被改写。只有
当 `Content-Range` 从本地文件长度开始，并在 advertised range total 结束，或 expected
media size 证明最终长度正确时，resume 才会 append；匹配的 416 响应会被视为已经完成。
当没有 expected size 时，会拒绝 wildcard `Content-Range` total。当 stream 或 FLV segment
声明 size 时，executor 会拒绝最终文件长度不匹配，并把失败写入回滚到尝试前长度。没有写
入任何字节却完成的媒体响应会被拒绝。条目目录包含内容身份，因此同标题视频不会共享
resume target；字幕 sidecar 名称包含 track
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
此重新排序的页面和 episode-vs-BV URL 形态仍能检测为重复。`DownloadArchive::records_for_plan`
会返回同一内容在所有 download mode 下的归档记录，而 `records_for_plan_with_mode` 会把查
询收窄到一个 `DownloadMode`。
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
会保留已有 mux 文件，并保持 JSON report 可解析且准确。临时章节 ffmetadata 会在成功、失
败或取消 mux 后移除。

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

PGC stream planning 首先根据 `PlayurlMode` 调用选中的官方 PGC playurl 端点，即 web HTTP
或 APP gRPC。如果响应明确报告区域限制，且配置了受限区域代理，client 会按顺序尝试候选，
直到某个候选返回有效 DASH 或 FLV stream 形态。对于 APP gRPC，fallback 信号可以来自区域限制
消息、permission-denied gRPC status，或 `view_info.dialog`、stream limit、preview-only
business state 等 PGC response-body metadata。非区域类官方失败会保留原错误，不会联系代理主机。BBDown/BiliPlus 风格的 HTTP(S) playurl
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

当前实现支持端点覆盖、B23 redirect 解析、PUGV/cheese metadata 和 stream planning、收藏
夹/空间投稿/合集/系列/观看历史/动态 feed 的批量 metadata planning、intl metadata 形态、
官方 PGC stream planning、官方 intl OGV 签名 stream planning、配置化 PGC proxy fallback、
顶层 helper playurl 响应解析、typed source reporting、resolver diagnostics 和下载执行。浏
览器专用 mobile response rewriting 有意不在范围内。

## 凭据

CLI 会把凭据存储在平台配置目录下的本地 JSON 文件中，并在 Unix 上使用 `0600` 权限。crate
暴露 `Credentials` 和 `CredentialStore`，让其他项目可以注入自己的存储或把凭据保存在内存
里。
`CredentialStore::load()` 和 `CredentialStore::save()` 会继续为默认 profile 读写旧的扁平
JSON credential 形态，因此现有用户和测试 fixture 不需要迁移步骤。需要 profile-aware 存储
的调用方可以使用 `CredentialProfiles`、`load_profiles`、`save_profiles`、`load_profile`、
`save_profile` 和 `remove_profile`，在 versioned profile document 中保存多个命名凭据集。
通过 profile API 读取旧扁平文件时，它会被包成 `default` profile；保存命名 profile 时，会把
文件迁移到 profile document，同时保留默认凭据。`Credentials` 和 `CredentialProfiles` 的
debug 输出都不会暴露原始凭据值。
`CredentialProfileSelection` 是 CLI 和嵌入调用方共用的选择层：默认选择保留旧的
`load`/`save` 行为，命名选择则通过 profile document API 路由，并在写入时保留其它 profile。
CLI 通过全局 `--credential-profile` flag 和 `BBDOWN_CREDENTIAL_PROFILE` 暴露该能力；
`auth logout` 在旧的默认选择下清空整个 store，在显式选择命名 profile 时只移除该 profile。
profile document 可以包含按 profile 和 credential kind 索引的可选 lifecycle metadata。metadata
会记录来源、获取/检查/过期时间戳，以及 `refresh_token_present` 布尔提示，但不会在 metadata
map 中复制原始 token 值。normalize 时会丢弃空 metadata；当 profile 没有 credentials 时，
orphan metadata 也会被移除；未知或 malformed 的可选 metadata 会在加载时被忽略，因此旧的
flat credential 文件和有效 profile document 仍可在没有 lifecycle metadata 的情况下加载。
`CredentialLifecyclePolicy` 会把这些已持久化 metadata 转换成确定的 stale / expiring /
expired 状态输出，且不发起网络请求。policy 要求调用方显式传入 `now_unix_millis`，并允许
embedding app 自行选择 stale 和 expiring 窗口，因此 UI preflight、后台任务和测试可以在不从
model 内部读取 wall-clock time 的情况下做出一致的 lifecycle 判断。
credential health diagnostics 是同一 credential model 上的只读层。crate 暴露
`CredentialHealthReport` 和 `BiliClient::check_credential_health()`，让嵌入调用方可以在选
择登录或 fallback flow 之前，分别检查 WEB cookie、通用 `access_key` 和 TV `tv_access_key`。
每个 probe 会用 `kind` 记录 credential storage slot，用 `scope` 记录实际检查的消费场景。
WEB cookie probe 使用 `/x/web-interface/nav`；token probe 使用
`/x/passport-login/oauth2/info`，把凭据作为 signed `access_key` app query 值发送，且不会发送
cookie。通用 token probe 当前通过配置的 `passport_base` 检查 intl/Bstar scope；这不代表同一
个已存储 `access_key` 对 APP gRPC 或 restricted-area proxy 也一定有效。TV token probe 使用
配置的 `tv_passport_poll_base`。probe failure 会按凭据独立记录为 `missing`、`valid`、
`rejected` 或 `request_failed`，不会让整份报告失败。
`CredentialHealthReport::summary()` 会给下游 UI 一个紧凑的 aggregate status，同时保留每个
kind 的 probe，供精确 policy decision 使用。

通用 access-key 获取被建模为 BiliPlus/BALH-compatible browser handoff，而不是官方 Bilibili
poller。`AccessKeyLoginConfig` 会用 `balh_auth=1` 和归一化 callback origin 构造授权 URL；
`AccessKeyLoginTicketOutput` 会暴露 URL、QR payload、预期 message origin 和 callback origin，
供嵌入 UI 使用。parser 接受历史 `balh-login-credentials:` message prefix；payload 可以是
JSON credentials，也可以是使用 `access_key` 或 `access_token` 的 URL/query callback。
`AccessKeyLoginCredentials` 会保留可选的 `refresh_token`、绝对 `oauth_expires_at` 和相对
`expires_in` metadata，但转回 `Credentials` 时只保存通用 `access_key`。自行管理存储的嵌入
方应显式把过期时间和 refresh-token presence 复制到 `CredentialLifecycleMetadata`。CLI 登录
路径保存所选 profile 时会执行这一步：绝对 expiry 直接保存，相对 `expires_in` 按获取时间
换算，并且 lifecycle metadata 只记录 refresh token 是否存在。refresh scheduling 和 token
rotation 仍然是独立 lifecycle 工作，让嵌入方可以选择自己的策略。
browser `postMessage` consumer 应通过 ticket/output 的 `credentials_from_message` helper
解析，它会先把 sender origin 与可信 auth origin 或 callback origin 校验，再使用 raw BALH
payload parser。
CLI 用 `auth login-access-key` 包装这个 core API：它会打印同一组授权 URL 和 QR payload，
通过 `--stdin` 或 `--file` 接收粘贴的 message 或 callback data，然后把得到的通用
`access_key` 和安全 lifecycle metadata merge 到当前选择的 credential profile。它会刻意避免交互式 secret paste
prompt，因为终端 echo 可能把 callback token 泄露到 scrollback 中；`--stdin` 也要求来自
pipe 或 redirect，`--file` 会拒绝 terminal-backed path，并且命令会拒绝隐式 stdin，调用方
必须先显式选择才会消费 pipe 或 redirect 输入。自动化可以读取换行分隔 JSON ticket/saved
事件，stdout 中不会包含 token 值。

二维码登录在 crate 中建模为显式状态机。WEB 二维码登录创建 `QrLoginTicket`，它可以转换为
`QrLoginTicketOutput`，提供稳定的可序列化扫码 URL 和 QR payload 表面；随后轮询
waiting-for-scan、waiting-for-confirmation、expired 和 succeeded 状态，然后返回 cookie 凭据。TV
二维码登录使用 BBDown-compatible app signed form flow，并返回 TV 专用 access-key 凭据。这与通用
intl/Bstar `access_key` 分离，因为 Bilibili app token 绑定 appkey。WEB QR 成功优先使用响应
`Set-Cookie` headers，并回退到从 cross-domain success URL 提取 BBDown-compatible cookie。TV
auth-code 创建和 TV polling 分别可配置，因此测试和受控代理可以镜像上游 split-host 流程或单一本地端
点。TV ticket 会保留生成的 device session context，使 polling 复用同一 device identity。二维码登录
请求即使 client 存有凭据，也使用 anonymous headers。CLI `auth login-web` 和 `auth login-tv` 命令会
在 succeeded state 后重新加载当前本地 credential store，再合并返回凭据，因此长时间二维码等待不会用等
待前的陈旧快照覆盖另一个命令的凭据更新。

密钥永远不会包含在状态输出中；`auth status` 和二维码登录 `saved` JSON 输出只报告布尔值。
二维码登录 `ticket` 事件和面向人类的扫码输出会有意暴露扫码 URL 与 QR payload，方便用户认证；调用
方应把这些值视为临时登录密钥。public QR state enum 有意不 derive serde traits，因为 succeeded
state 携带完整凭据，供自行处理存储的嵌入调用方使用。QR ticket 和 QR ticket-output debug 输出会脱敏，因为
ticket key 和扫码 URL query string 可作为预认证密钥。Credential health report 不会包含原始凭据值，
API message 在序列化前会先经过与 restricted-area diagnostics 相同的 diagnostic sanitizer。HTTP request
error 转换时不会保留完整 URL，因此 intl `access_key` 等 query 密钥不会出现在面向用户的错误中。

## 测试和 CI

默认 CI 是确定性的：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- 使用 `cargo +1.95.0 check --workspace --locked` 检查 declared MSRV
- 使用 `cargo test --workspace --locked` 运行单元测试和 workspace integration tests
- 本地 CLI mock e2e 测试使用 `cargo test -p bbdown-cli --test cli_e2e --locked`
- 对可发布 `bbdown-core` library package 执行 crates.io dry-run packaging

Release packaging 是单独的 GitHub Actions workflow stack。`Release Artifacts` 是可复用且
仅手动/被调用的 workflow：它会构建 Linux x86_64、macOS x86_64、macOS aarch64 和 Windows
x86_64 CLI 归档，但不发布 tag、GitHub Release 或 crate。`Release Verification` 也是可复用
workflow：RC 创建和 RC promotion 都会调用它，对选中 commit 运行 formatter、clippy、
declared MSRV、测试和 crates.io dry-run validation。`Create Release Candidate` 会验证
repository default branch 或 `release/*` source branch、构建这些归档，并通过 release GitHub
App 在 workflow ref commit 上创建 annotated `vX.Y.Z-rc.N` tag，但会先拒绝已经存在 final tag
或 GitHub Release 的 version。Create workflow 也会在真正写入 RC tag 前重复 final tag 和
GitHub Release 检查。`Promote Release
Candidate` 必须从请求版本的最新 RC tag 运行；它会重新验证、重新构建正式归档，在
发布前再次确认选中的 RC 仍然最新，创建正式 annotated `vX.Y.Z` tag，发布 GitHub Release，
然后通过受保护的 `crates-io` environment 发布 `bbdown-core` 到 crates.io。它也会在创建
正式 tag 或 GitHub Release 前检查 crates.io 上任何已存在的 `bbdown-core` version 没有被
yank 且 package checksum 符合预期，并在 publish job 中重复检查，之后才会把已存在 crate
version 视为恢复成功。RC 创建和
promotion 共用按 version 分组的 concurrency group，因此同一 version 正在 promotion 时不会
再创建更高编号 RC。归档包含 `bbdown` 二进制、英文和简体中文 README、英文和简
体中文用户指南、嵌入指南、发布 runbook 和架构指南，以及 `LICENSE`。每个归档旁边也有平
台专用 checksum 文件。GitHub Release notes 会从上一条非 RC 正式 release tag 生成，避免
把 RC tag 当成正式 release 的比较起点。promotion 也支持 GitHub Release 创建被中断后的重
试：draft release 会被删除并重新创建，已发布 release 只有在预期 asset 集合已经完整时才
会被复用。复用要求 asset 名称集合与预期完全一致、状态为 `uploaded`、大小非零，并且下载
后的归档会被其已发布 `.sha256` sidecar 点名并校验通过；重新构建出的 `dist` 归档也必须通
过自己的 sidecar 校验，并且 archive checksum 与已发布 assets 相同。Release archives 会规
范化条目顺序、时间戳、owner、group 和归档容器 metadata，因此同一组已编译输入会产生稳定
package checksum。workflow 会按 `tag_name`
列出 releases，而不是只依赖仅面向 published release 的 tag endpoint，因此 release GitHub App token 能看到 draft release。crates.io publish step 会先检查 exact
`bbdown-core` version，然后重新打包选中的 RC 源码，并要求本地 `.crate` SHA256 与
crates.io checksum 匹配后，才把已发布版本视为恢复成功。这样可以覆盖 upload 已被接受但
runner 失败的情况，同时不会让同一版本的不同 package 通过恢复路径。release workflows 使用 GitHub-hosted runner 自带的 `rustup` 和
`rust-toolchain.toml` 中的 floating stable Rust channel；有意不使用第三方 Rust toolchain
或 cache actions。它们也会安装 Rust 1.95.0 来运行与 crate `rust-version` metadata 匹配的
`cargo check` gate。包名会把 release ref 规范化到打包器安全的 `[A-Za-z0-9._-]` 字符集，
因此 SemVer build metadata 等 tag 不会导致打包失败。共享 release shell helpers 放在
`scripts/release/`，这样 tag/release API query 和 Cargo version 提取逻辑可以脱离 YAML 做 lint。

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

可复用 crate 当前处于已发布 `0.4.0` 之后的 `0.5.0` 开发线，因此 public configuration
structs 会通过 constructor 和 builder API 刻意加固，而不是保留本地 struct-literal 实验。
嵌入者应通过这些 API 创建配置，包括
`ClientConfig::default().with_*`、`EndpointConfig::default().with_*`、
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

Append-only 弹幕更新覆盖 XML merge 单元测试、会重新生成 ASS 的 archive-backed core update 测
试，以及验证 JSON report 和 archive sidecar path 更新的 CLI mock e2e 覆盖。

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
13. 更多输入解析和批量集合解析，覆盖短链接、PUGV/cheese、收藏夹、空间投稿、合集和系
    列。已在本切片完成。
14. collection-like 页面族的 shared feed/list resolver abstraction。已在本切片完成。

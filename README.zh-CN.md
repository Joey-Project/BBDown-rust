[ [English](README.md) | 简体中文 ]

# BBDown Rust

`BBDown Rust` 是 BBDown 的 Rust 原生重写，目标有两个：

- 提供可被其他 Rust 项目复用的 `bbdown-core` package / `bbdown_core` crate；
- 提供一个 CLI，作为元数据解析和下载流程的端到端使用界面。

本项目以原 [BBDown](https://github.com/nilaoda/BBDown) 项目的 Bilibili 实用行为为重要
参考。感谢 BBDown 及其贡献者提供的参考。

当前实现已经建立 crate / CLI / CI 基础、元数据解析器、流规划、媒体下载、封面/字幕/弹幕
旁路文件下载、重试和断点续传、可选 `ffmpeg` 封装、二维码登录、可选 live 测试框架、带诊
断信息的受限区域代理排序、面向下游流式播放 / 缓存集成的播放请求规格、UPOS/PCDN 媒体
host 控制，以及 builder 风格的 crate 集成 API。它还支持显式下载归档，用于重复下载预检
查、CLI 的 replace / keep-both / cancel 决策，以及已下载归档条目的 append-only 弹幕旁路
文件更新。已发布的 `0.7.0` 版本增加显式 CDN host pool、限额 Range 探测、可选并行
Range 下载、
内置 CDN/公共 resolver 目录，以及独立的 PGC Web playurl 路由 API；这些功能不会自动选择
路由。`0.8.0` 版本的 Preserve 更新策略会按正 `p[7]`
ID 去重弹幕；ID 缺失、为零或无效时，会回退到完整 `p` 属性和解码后的文本进行匹配。它保留 XML 历史，从合并后的
XML 重新生成所选 ASS 输出，再将暂存后的旁路文件与归档一起发布。`0.9.0` 为
progress callback 和 CLI `--progress-json` 增加结构化 CDN 传输诊断，并通过 WBI 签名空间动态
feed 请求，包括 offset 分页。输入解析覆盖普通视频、PGC 和 intl 分集、PUGV/cheese 课程、B23 短链接、收藏
夹、空间投稿、合集、系列、首页推荐、单视频相关推荐、观看历史、
稍后再看列表、关注视频 feed 和空间动态视频 feed。URL 解析包括 canonical
`bilibili.com/list/...` 页面、
path-based medialist 收藏夹 URL，以及带 uploader mid 的空间合集 / 系列 URL，以便使用较
新的空间 API；同时支持 B 站首页、空间动态页面和普通视频 URL 相关推荐输入（包括
`m.bilibili.com/video/...`），以及需要登录态的
`bilibili.com/account/history` 观看历史页面，以及 `bilibili.com/watchlater` 和
`bilibili.com/list/watchlater` 稍后再看页面。

`0.10.0` 引入单视频相关推荐、适用于关注与空间动态 feed 的有界元数据分页 API，以及认证
SpaceDynamic 请求缺少设备 Cookie 值时的初始化逻辑。它还记录了可复现的 CDN 性能 baseline；两次新增样本中，
各自的顺序下载都快于对应的 Range 对照组。`0.10.0` 未实现自动 CDN 路由或自适应策略。使用新的库 API 需要
`bbdown-core = "0.10"`；`0.9` 依赖不包含这些 API。如果 registry 中尚无 `0.10.0` package，请使用对应的源码检出
或本地 path dependency。
`0.11.0` 源码版本增加普通视频搜索及选中条目的 PGC 识别、国内 PGC 分集分组、番剧与国创时间表 metadata、电影/电视剧索引 metadata、热门视频 metadata 和全站排行榜 metadata。排行榜会返回所有有效条目，不再使用此前任意设置的 100 条上限；每个响应的默认大小上限为 1 MiB。这些功能不实现频道、话题、自动 CDN 路由、持久化 CDN 健康状态或自适应 CDN 策略。请参阅 [v0.11.0 发布说明](docs/release-notes/v0.11.0.zh-CN.md)，并查看[官方 Releases](https://github.com/Joey-Project/BBDown-rust/releases)了解归档可用性。
当前源码检出也支持按 capability 声明 metadata service，用于国内 PGC 分集/season、media-review、时间表与电影/电视剧索引 metadata。索引路由需要独立的 `index` capability；这不代表支持搜索、PUGV 或 Intl metadata。

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
bbdown info recommendations --select latest --json
bbdown info related:av170001 --select latest --json
bbdown info history --select latest --json
bbdown info watch-later --select latest --json
bbdown info following --select latest --json
bbdown info https://www.bilibili.com/list/ml1103407912 --json
bbdown info https://space.bilibili.com/123/dynamic --select latest --json
bbdown info 'https://space.bilibili.com/123/lists/456?type=series' --json
bbdown search "Rust async" --page 2 --order pubdate --json
bbdown timetable --type anime --json
bbdown timetable --type guochuang --json
bbdown --pgc-metadata-service-base https://metadata-gateway.example \
  --pgc-metadata-service-capabilities timetable timetable --type anime --json
bbdown pgc-index list --type movie --page 1 --page-size 20 --order play --json
bbdown pgc-index filters --type tv --json
bbdown popular --json
bbdown popular --page 2 --page-size 10 --json
bbdown ranking --json
bbdown ranking --max-response-bytes 2097152 --json
```

`timetable` 输出番剧或国创的上游 PGC 时间表 metadata。它只发起一次匿名请求，不会解析流或下载分集。
该命令包含在 `v0.11.0` 源码版本中。请查看[官方 Releases](https://github.com/Joey-Project/BBDown-rust/releases)了解 CLI 归档可用性。

`pgc-index` 可按页列出 PGC 电影或电视剧 metadata，也可读取该类别支持的筛选值。它只提供匿名 metadata，
不会解析或下载列表中的 season。该功能包含在 `v0.11.0` 源码版本中。请查看[官方 Releases](https://github.com/Joey-Project/BBDown-rust/releases)了解 CLI 归档可用性，并查看[crates.io](https://crates.io/crates/bbdown-core)了解 crate package 可用性。查询与路由说明见
[用户指南](docs/user-guide.zh-CN.md)和[嵌入指南](docs/embedding.zh-CN.md)。

`popular` 只使用 API base 和请求超时，返回一页有界的匿名热门视频 metadata；不会读取凭据 profile、解析流或下载列表中的视频。
该功能包含在 `v0.11.0` 源码版本中。请查看[官方 Releases](https://github.com/Joey-Project/BBDown-rust/releases)了解 CLI 归档可用性，并查看[crates.io](https://crates.io/crates/bbdown-core)了解 crate package 可用性。请求限制和 Rust 示例见
[用户指南](docs/user-guide.zh-CN.md)和[嵌入指南](docs/embedding.zh-CN.md)。

`ranking` 返回一页有界的匿名全站排行榜（`rid=0`、`type=all`、`web_location=333.934`），使用配置的 API base、请求超时和响应大小选项。它最多先向 `/x/web-interface/nav` 发起一次匿名请求以获取公开 WBI 密钥，再发起一次签名排行榜请求；`--max-response-bytes` 设置一个字节上限，并将同一上限分别应用于每个响应；每个响应都可使用完整上限，不共用合计字节预算，默认上限为 1 MiB。所有有效条目均按上游顺序返回，不设任意条目数上限。
它不会读取凭据、补充详情、解析 playurl 或请求媒体。排行榜没有固定时间周期，顺序会变化，响应不承诺固定快照或总数。
该功能包含在 `v0.11.0` 源码版本中。请查看[官方 Releases](https://github.com/Joey-Project/BBDown-rust/releases)了解 CLI 归档可用性，并查看[crates.io](https://crates.io/crates/bbdown-core)了解 crate package 可用性。Rust API 和条目字段见
[用户指南](docs/user-guide.zh-CN.md)和[嵌入指南](docs/embedding.zh-CN.md)。

`search` 返回一页有界的普通视频关键词 metadata。若要解析、规划、播放或下载搜索结果，可将
`search:<keyword>` 或官方搜索 URL 传给 `info`、`plan`、`playback` 或 `download`，并显式选择结果序号；只有
被选中的结果会继续解析。普通视频使用 P1，确认的 PGC 分集按下文所述进入 PGC 路径。页数限制和 Rust 示例见[用户指南](docs/user-guide.zh-CN.md)
与[嵌入指南](docs/embedding.zh-CN.md)。
该功能包含在 `v0.11.0` 源码版本中。请查看[官方 Releases](https://github.com/Joey-Project/BBDown-rust/releases)了解 CLI 归档可用性，并查看[crates.io](https://crates.io/crates/bbdown-core)了解 crate package 可用性。
用搜索输入进行选择会重新查询；直接使用展示条目的 BVID 或 aid 可按现有直接视频行为指定该普通视频身份。
PGC 自动识别只在通过搜索输入选择条目时运行；确认分集后，后续命令可使用 plan 返回的 `epid` / `ep` 精确指定分集。
搜索列表仍是普通视频搜索。若选中 BVID 或 aid 的官方 view 响应包含官方 PGC 分集重定向，且身份校验通过，
规划会保留分集身份并进入现有 PGC 路径。搜索 `api_base`、PGC metadata `pgc_base` 和 PGC Web playurl 路由分别配置；
PGC Web 使用配置的 Web playurl 路由；Web 和 APP 沿用现有的受限区域代理回退，TV 语义保持不变。
当前源码检出可为国内分集/season hydration、media-review metadata、PGC 时间表和电影/电视剧索引 metadata 显式选择
capability 声明的 PGC metadata service，使用 `--pgc-metadata-service-base` 和
`--pgc-metadata-service-capabilities`；这两个参数必须同时提供。时间表和索引路由分别需要单独声明
`timetable` 和 `index` capability，capability 之间互不隐含。该 service-only 路由不会回退到 `--pgc-base` 或代理。
支持的 endpoint、capability 和响应大小限制见[嵌入指南](docs/embedding.zh-CN.md)。

国内 PGC season metadata 会在 `info --json` 输出的 `episode_groups` 字段中包含按上游顺序排列的
`main` 和 `section` 分集组。组成员使用完整 season 分集列表中的零基位置，因此即使 `--select` 只
返回一个子集，分组仍可正确对应。Intl 和 PUGV season 输出仍保持平铺结构。

生成下载计划并输出 JSON：

```bash
bbdown plan av170001 --json
bbdown plan ep267851 --json
bbdown plan ss26801 --select latest --json
bbdown plan https://www.bilibili.tv/en/play/34613/341736 --json
bbdown plan fav456 --select page:1 --json
bbdown plan recommendations --select latest --json
bbdown plan related:BV1xx411c7mD --select 1 --json
bbdown plan history --select latest --json
bbdown plan watch-later --select latest --json
bbdown plan following --select latest --json
bbdown plan cheese/ss202 --select latest --json
```

`plan` 会解析所选条目、可用 DASH 或 FLV 流 URL、字幕 URL，以及每个 `cid` 的弹幕 XML
URL。PGC 和 intl 规划仍可能需要符合条件的账号或区域访问。PGC playurl 解析可以回退到
用户配置的受限区域代理。它不会下载文件。集合类输入默认解析全部条目；使用
`--select page:<index>` 可规划一个集合条目，使用 `--select latest` 可规划上游列表顺序中的第一个解析条目。
`info --json` 会在 `collection.collection.items` 保留完整解析到的集合 metadata；`plan` 只
输出选中条目。

生成播放请求规格并输出 JSON：

```bash
bbdown playback av170001 --json
bbdown --playurl-mode tv playback av170001 --json
bbdown --playurl-mode app playback av170001 --json
bbdown playback ss26801 --select latest --json
bbdown playback fav456 --select 1,3-5 --json
```

`playback` 会解析与 `plan` 相同的选中条目，然后输出选中的 DASH 视频/音频请求规格，或
FLV segment 规格，其中包含主 URL、备用 URL、headers、mime/codec metadata、时长/大小和
entry/variant/media cache key、ABR switching group，并包含 `selection_hints.avplayer`
metadata：已知时的 exact codec 字符串、codec family、`format_key` 和面向 AVPlayer 的排序信号。
下游可以用 `PlaybackCodecPreference` 自定义 H.264、HEVC、AV1 或其它 codec 的优先级，
再用 `PlaybackVariant.abr` 和 `PlaybackEntry.abr` 在切换 level 时保留已经缓存的
codec/mime-compatible variants。它不会下载文件、创建 HLS playlist 或运行播放器。
设置 `--playurl-mode tv` 或 `BBDOWN_PLAYURL_MODE=tv` 后，普通视频和 PGC 分集会通过
BBDown-compatible TV HTTP playurl 端点解析。TV mode 使用 `auth login-tv` 保存的 TV 专用
access key，并可用 `--tv-api-base` / `BBDOWN_TV_API_BASE` 覆盖端点。
设置 `--playurl-mode app` 或 `BBDOWN_PLAYURL_MODE=app` 后，普通视频和 PGC 分集会通过
BBDown-compatible APP gRPC playurl 端点解析。APP mode 优先使用
Bilibili main/BALH 通用 `Credentials::access_key`，再回退到 `Credentials::tv_access_key`；如果当前
profile 把通用 key 标记为 `bili_intl_oauth2`，或者这个 profile 来自旧版扁平 credential file
而没有 provider metadata，APP mode 会优先使用 TV key，只有没有 TV key 时才回退到该通用
key。mock 或代理端点可用 `--app-grpc-base` / `BBDOWN_APP_GRPC_BASE` 和
`--app-pgc-grpc-base` / `BBDOWN_APP_PGC_GRPC_BASE` 覆盖；普通视频和 PGC APP 默认都使用
`https://grpc.biliapi.net`。PGC APP gRPC
响应如果带有区域限制或 preview-only 信号，仍会回退到已配置的 restricted-area HTTP playurl
proxy；信号可以来自区域限制消息、APP permission-denied gRPC status 或 PGC response-body
metadata。proxy fallback URL 只会使用通用导入
的 `Credentials::access_key`，不会转发 TV 专用 token。非零 gRPC status 会同时读取 initial
headers 和 trailing metadata。APP DASH 响应中的分辨率和帧率等 metadata 会保留到
playback/API 输出。如果 APP 响应返回多个
legacy FLV 分段清晰度，规范化后的 `StreamSet` 只暴露最高质量的一组 segment，不会把不同
清晰度的分段混合到同一列表。

下载所选媒体文件：

```bash
bbdown download av170001 --output-dir downloads
bbdown download ss26801 --select latest --output-dir downloads
bbdown download fav456 --select page:1 --output-dir downloads
bbdown download av170001 --output-dir downloads --no-mux --json
bbdown download av170001 --audio-language ja-JP --output-dir downloads
bbdown download av170001 --only cover --output-dir downloads --json
bbdown download av170001 --output-dir downloads --archive-file downloads/archive.json --on-duplicate keep-both
bbdown download av170001 --output-template "{title}-{entry_count:02}" --entry-template "{index:02}-{entry_title}" --no-mux
bbdown danmaku update av170001 --archive-file downloads/archive.json --danmaku-format xml,ass
bbdown download av170001 --upos-host upos-sz-mirrorcoso1.bilivideo.com --no-mux
bbdown download av170001 --output-dir downloads --no-mux --json --progress-json
```

`download` 会解析下载计划，下载第一组完整的 DASH 视频/音频，或下载 FLV 分段；默认写入
封面、字幕和弹幕旁路文件；通过 HTTP range 请求续传部分文件；对有限的瞬时失败进行有界重
试；在存在声明媒体大小时进行校验；拒绝不完整的媒体形态；除非传入 `--no-mux`，否则运行
`ffmpeg`。使用 `--no-cover`、`--no-subtitles` 或 `--no-danmaku` 可以跳过对应旁路文件。
弹幕输出默认是 XML；传入 `--danmaku-format ass` 会只生成 ASS，传入
`--danmaku-format xml,ass` 可同时保留 XML 和 ASS 旁路文件。
使用 `--subtitle-ai include|prefer-non-ai|exclude-ai|only-ai` 控制 AI 生成字幕旁路文件。
默认 `include` 会保留所有发现的字幕轨道。
使用 `--only video`、`--only audio`、`--only subtitle`、`--only danmaku` 或
`--only cover` 可只写入一种输出；single-output 模式会跳过 mux。
使用 `bbdown plan` 查看 `streams.audios[*].language` 和 `language_doc` 后，可以传入
`--audio-language <LANG>` 选择第一条匹配的 DASH 音频流。显式视频/音频清晰度或语言选择
会写入 archive key，因此不同媒体 variant 不会互相满足 duplicate preflight。
使用 `--output-template`、`--entry-template` 和 `--mux-template` 可定制输出根目录、条目
目录和 mux 文件名 stem。模板输出会按文件名组件清洗；媒体、封面、字幕和弹幕旁路文件名
仍保持稳定，以保证续传和重复轨道处理安全。条目模板必须在选中条目之间渲染出唯一目录名。
CLI 默认会避开疑似 PCDN 媒体 URL，同时保留本地和私网 host。传入 `--allow-pcdn` 可保留
原始 PCDN 候选；传入 `--upos-host <HOST>` 可把 DASH/FLV 媒体候选改写到指定 UPOS host；
传入 `--force-replace-host` 可把媒体候选改写到内置 BBDown fallback host。封面、字幕和弹
幕旁路 URL 不会被改写。
传入 `--progress-json` 会把 typed progress event 以 JSON Lines 流式写到 stderr，同时让
stdout 保持人类输出或最终 `--json` report。
传入 `--archive-file <path>` 后，CLI 会按内容身份记录已完成下载。归档输出、旁路文件和
mux 路径会在记录时保存为绝对路径，因此同一份归档可以从另一个工作目录复用。条目身份使
用稳定的 aid/cid 媒体 id，因此同一 PGC 分集即便之后通过 BV/av URL 规划，且其中一种形式
缺少 BVID，也仍能匹配。当同一内容、条目或归档输出目录再次出现时，非交互 JSON 模式需要
提供 `--on-duplicate replace`、`--on-duplicate keep-both` 或 `--on-duplicate cancel`；
交互式人类模式在没有决策时会提示，且在该提示中按 `Ctrl-C` 会立即以 130 退出。`replace`
会先删除已有的计划输出根目录，再重新下载，并替换指向该输出路径的陈旧归档记录；
`keep-both` 会写入下一个带后缀的输出根目录，并避开所有归档记录输出路径；`cancel` 会只
报告预检查状态，不下载。归档文件本身不能是所选输出根目录，也不能位于该根目录之内；CLI
对归档保存旁路路径应用相同保护。如果归档文件是符号链接，保存会更新符号链接目标，以免
共享归档历史被拆分。

使用 `bbdown danmaku update <input> --archive-file <path>` 可以在不重新下载媒体的情况下
刷新已有归档记录的弹幕旁路文件。命令会重新规划输入，用稳定的 aid/cid 身份查找匹配归档
条目，下载当前 XML 弹幕 payload，只把新增弹幕 append-merge 到 `danmaku.xml`，再重新生成
所选派生格式，例如 `danmaku.ass`，并把更新后的旁路文件路径写回归档。XML 始终是 canonical
更新目标；`--danmaku-format ass` 会基于合并后的 XML 新增或刷新 ASS。可选
`--update-policy preserve` 会保留原 XML 历史，并把暂存后的旁路文件与 archive 一起发布；每次
都会从完整合并 XML 重新生成所选 ASS，因此不会保留旧 ASS 的自定义样式或事件。有效的正 ASCII 十进制
`p[7]` id 相同的 XML 弹幕即使拉取后的元数据或文本改变，也会视为已有弹幕；id 不同的弹幕即使文本
相同也会追加。id 缺失、为零或格式无效时，回退为完整 `p` 属性和解码后的文本匹配。没有 XML 基线时，
旧 ASS 事件不作为历史，ASS 只从本次拉取的 XML payload 生成。默认仍是 `legacy`。

`ss` 和 `md` 输入在非交互模式下需要显式选择：

```bash
bbdown info ss26801 --select latest
bbdown info ss26801 --select all
bbdown info ss26801 --select episode:267851
bbdown info ss26801 --select page:1
```

`cheese/ss...` 输入遵循相同的显式选择规则。收藏夹、空间投稿、合集、系列、首页推荐、观
看历史、稍后再看列表、关注 feed 和空间动态 feed 都是批量输入。推荐输入可使用
`recommendations`、
`recommendation`、`recommend` 或 B 站首页 URL；它会解析首页推荐批次，目前只包含普通视
频 `av` 卡片，并在显式 index selection 需要时在安全上限内继续请求后续 `fresh_idx` 刷新
批次来覆盖过滤后的条目。单视频相关推荐使用 `related:<video>`，其中 `<video>` 可为 `av`
或 `BV` 标识、数字 aid 或 B 站官方桌面站/移动站视频 URL（包括
`https://m.bilibili.com/video/...`）。它只读取该源视频的一批有限相关推荐，不会
翻页或递归抓取相关推荐；结果按上游顺序排列，排除源视频、无效条目和 OGV 条目，并按 aid
去重。`--select` 按这份结果列表的序号选择；homepage aliases 和首页 URL 仍表示首页推荐。
此功能于 `0.10.0` 引入；`0.9` 版本不包含此功能。请查看[crates.io](https://crates.io/crates/bbdown-core)了解 package 可用性，或使用匹配的源码检出或本地 path dependency。
观看历史输入可使用 `history` 或 `https://www.bilibili.com/account/history`，需要已认证的
WEB cookie，目前只包含普通视频 `archive` 历史记录。稍后再看输入可使用 `watchlater`、
`watch-later`、`watch_later`、`later`、`toview` 或
`https://www.bilibili.com/watchlater` 或 `https://www.bilibili.com/list/watchlater`，需要已
认证的 WEB cookie，并包含该账号稍后再看列表中的普通视频。关注输入可使用 `following`、
`https://t.bilibili.com/` 或 `https://www.bilibili.com/account/dynamic`，需要已认证的 WEB
cookie，目前包含普通视频 archive 卡片。空间动态输入使用
`https://space.bilibili.com/<mid>/dynamic`，可以匿名访问，目前包含公开普通视频 archive 卡片。

管理本地凭据：

```bash
bbdown auth import-cookie --stdin
bbdown auth import-cookie --file cookie.txt
bbdown auth import-access-key --stdin
bbdown auth login-access-key --stdin < balh-callback.txt
bbdown auth login-access-key --file balh-callback.txt
bbdown auth renew-access-key --json
bbdown auth renew-access-key --stdin < balh-callback.txt
bbdown auth login-web
bbdown auth login-tv
bbdown auth renew-web --json
bbdown auth renew-tv --json
bbdown auth status
bbdown auth switch intl
bbdown auth health --json
bbdown auth logout
```

凭据默认存储在平台配置目录。使用 `--credential-file <path>` 可以为集成测试或本地实验覆
盖该路径。没有输入标志时，密钥导入命令也会读取 `BBDOWN_COOKIE` 或
`BBDOWN_ACCESS_KEY`，这样调用方无需把凭据放到进程参数里。`auth login-access-key` 会打
印 BiliPlus/BALH-compatible 授权 URL 和 `qr_payload`，然后从 `--stdin` 或 `--file` 读取
粘贴的 `balh-login-credentials:` message 或 callback URL/query，并保存得到的通用
intl/Bstar access key。它不会提供交互粘贴 prompt，因为终端 echo 可能把 token 值暴露在
scrollback 中；`--stdin` 必须来自 pipe 或 redirect，并会拒绝 terminal stdin；`--file` 也
会拒绝 terminal-backed path。命令不会隐式消费 stdin；pipe 或 redirect 必须显式传
`--stdin`。如果输入来自浏览器 `postMessage`，请使用 `--message-origin`，这样 CLI 会把
sender origin 与本次 login ticket 校验；兼容 mock 或部署可以使用 `--auth-base` 和
`--callback-origin`。单次命令可以用 `--credential-profile <name>` 选择账号；如果要把某个已
存在 profile，或当前默认 profile，持久设为后续命令的默认账号，使用 `auth switch <name>`。
`auth renew-access-key` 会评估当前所选 profile 的 access-key lifecycle
metadata 并输出结构化 renewal decision。fresh credential 会返回 `no_action`；missing、
unknown、stale、expiring、expired 或 forced credential 会返回 BiliPlus/BALH 重新授权 ticket。
同一个命令如果传入 `--stdin` 或 `--file`，就会完成这次重新授权并保存新的通用 access key。
命令会报告 `automatic_refresh_readiness`，方便嵌入方区分“metadata 显示曾出现 refresh
token”和“本地已经按 provider 保存 refresh secret”。refresh secret 会以明文 provider section
保存在同一个私有 credential file 中，并在 status、debug 和 JSON 输出中脱敏。所选 provider
状态为 `ready` 时，`auth renew-access-key` 可以非交互刷新通用 access key。credential import、
access-key login 和 renewal 只会保存当前选择的 profile，并会在协作 store lock 内重新读取最新
profile document 后再 merge 新的 credential、lifecycle 和 provider-secret 字段。这样不会覆盖
其它 profile，也能避免另一个 `bbdown` 进程刚刷新的 provider secret 被旧快照写回。可以确认
owner 进程已退出的 stale lock file，以及旧式没有 owner-pid metadata 的 lock，会在一个较短
恢复窗口后自动接管；仍在运行或无法确认是否退出的 owner 不会被接管。每次写入都会在临时文件写入完成后、真正替换 credential
file 前，校验自己的 lock token 仍然有效；lock 获取和 stale reclaim 会用同一个 companion guard
串行化，释放 lock 前也会校验同一个 token，因此仍在运行的 writer 不会在被接管后覆盖较新的
store，也不会删掉新 writer 的 lock。如果较慢的自动刷新 response 或 access-key 浏览器 handoff
保存已经不再匹配当前选择的 profile name，或 refresh response 不再匹配该 profile 的
credential / refresh-secret 状态，JSON 输出会发出 `refresh_skipped`，并带上
`reason=profile_changed`，同时保持当前
credential store 不变。直接运行的 `auth renew-access-key` / `auth renew-web` /
`auth renew-tv` 会把这种 stale save 视为刷新失败并以非零状态退出；media preflight 会重新评估当前
profile 后再决定是否继续。
WEB QR 和 TV QR login response 也可能包含 refresh token。出现时，CLI 会把它们作为明文
profile secret 保存到 `profile_secrets.<profile>.cookie` 或
`profile_secrets.<profile>.tv_access_key`，lifecycle metadata 只记录 token 是否出现以及时间戳。
`auth renew-web` 会通过 Bilibili cookie refresh flow 刷新当前选择的 WEB cookie；
`auth renew-tv` 会通过 TV OAuth refresh endpoint 刷新当前选择的 TV token。两个命令在非交互
刷新成功时都会输出 `decision`、`refreshed` 和 `saved` JSON event，脱敏原始 cookie/token，
并使用与 access-key refresh 相同的 stale-response guard。如果 Bilibili 返回当前 WEB cookie
暂时不需要 refresh，`auth renew-web --json` 会输出 `refresh_not_needed`，并保持已存 cookie、
refresh token 和 lifecycle 获取时间不变，同时更新 lifecycle 检查时间。
缺少 WEB/TV refresh secret 或 refresh 服务端失败时，命令会输出 `refresh_failed` 并以非零状态退出，
因为这两个命令没有交互式回退流程。
`plan`、
`playback` 和 `download` 也支持 `--credential-preflight warn|fail|renew`，方便调用方在 media
request 前检查当前 profile，也会覆盖使用通用 `access_key` 的 intl/Bstar media path；同样可以用
`BBDOWN_CREDENTIAL_PREFLIGHT`、`BBDOWN_CREDENTIAL_STALE_AFTER_SECONDS` 和
`BBDOWN_CREDENTIAL_EXPIRING_WITHIN_SECONDS` 配置。WEB playurl cookie 是 optional，stale
cookie metadata 只会 warning，不会阻断匿名公开视频；history、watch-later 和 following
这类账号级 feed 输入会要求 WEB cookie。公开的 space dynamic 页面可以匿名访问，不会加入
required-cookie preflight。preflight 对 restricted-area proxy 只有在
已配置通用 `access_key` 时才检查该 token；缺失 proxy access key 不会阻断自带认证或允许匿名
fallback 的 proxy URL。diagnostic 会写到 stderr，JSON stdout 仍保持为单个 plan 或 report
payload；`download --progress-json` 会抑制 preflight 纯文本 diagnostic，但失败时最终 CLI error
行仍可能写到 stderr，因此 wrapper 应只解析 JSON object line。在 `renew` 模式下，如果缺失的
required credential 不是当前 refresh 能修复的对象，就会阻止无关 refresh 抢跑。对 archive
下载，如果初次 plan 成功，`--credential-preflight renew` 会先完成已保存 credential 的 refresh
和 replanning，再进入 duplicate handling，让重复决策基于刷新后的 plan。`--on-duplicate cancel`
仍会在媒体下载和 archive 写入前停止，但当 WEB/TV credential 可刷新时，可能调用 refresh endpoint
并更新已保存 credential。如果初次 archive planning 因类似 auth 的 credential 错误失败，即使
本地 lifecycle metadata 仍认为该 key fresh，CLI 也会刷新 ready 的通用 access key 并重试一次
planning，然后才报告失败。
当当前选择的 WEB cookie 或 TV `tv_access_key` 已存在、lifecycle metadata 不是 fresh，且本地
保存了 refresh secret 时，`--credential-preflight renew` 会在解析 media request 前先尝试刷新
该 credential。这覆盖需要 WEB cookie 的账号级 feed 输入，以及选择 TV token 的 APP/TV playurl
路径。
二维码登录命令会轮询
Bilibili 二维码状态机，并只保存最终得到的凭据。WEB 二维码登录保存 cookie；TV 二维码登录保存
TV 专用 access key，不会覆盖由通用 access-key 命令导入或获取的 intl/Bstar access key。
使用 `--json` 时，登录命令输出换行分隔 JSON 事件：先输出带登录 URL 和 `qr_payload` 的
`ticket` 事件，再在凭据保存后输出
`saved` 事件。当前 WEB 和 TV 登录流程会直接使用扫码 URL 作为 QR payload。请把登录 URL
和 QR payload 当成临时登录密钥；状态输出和 `saved` 事件只暴露脱敏布尔值。
如果 QR poll response 包含 refresh metadata，这些 secret 会被保存下来，供之后
`auth renew-web`、`auth renew-tv` 和 preflight renewal 使用。
`auth health` 会在不打印密钥值的情况下检查已配置凭据：WEB cookie 通过 web nav 端点检查；
通用 `access_key` 和 TV `tv_access_key` 会通过 OAuth info 端点以 signed `access_key` app
query 值检查。通用 token probe 当前覆盖 intl/Bstar scope，并使用 `--passport-base`；它不会
证明同一 token 对所有 APP gRPC 或 proxy 消费者都可用。有 TV passport 覆盖时，TV token
refresh 会跟随 `--tv-passport-poll-base`；否则保留 `--passport-base` 兼容行为。TV token probe
使用 `--tv-passport-poll-base`；如果只提供 `--tv-passport-base`，poll base 会跟随该 TV 覆盖。
JSON 输出会用 `kind` 表示凭据槽位、用 `scope` 表示实际检查的消费场景，并报告 `missing`、
`valid`、`rejected` 或 `request_failed` 状态，便于嵌入调用方和自动化使用。

使用 `--request-timeout-seconds` 或 `BBDOWN_REQUEST_TIMEOUT_SECONDS` 调整 API 请求时限。
媒体正文读取使用 `--download-idle-timeout-seconds`；传入 `0` 可禁用 idle timeout。使用
`--comment-base` 或 `BBDOWN_COMMENT_BASE` 可以把弹幕 XML 下载指向 mock 或代理端点。
使用 `--passport-base` 可配置 WEB 二维码登录和通用 token health mock 或代理；使用
`--tv-passport-base` / `--tv-passport-poll-base` 可配置 TV 二维码登录、TV token refresh 和
TV token health mock 或代理。提供 TV 专用覆盖时，TV 二维码轮询、TV token refresh 和 TV token
health probe 会跟随 `--tv-passport-base`；否则 TV token refresh 保留 `--passport-base` 兼容行为，
TV 轮询/probe 除非显式设置 `--tv-passport-poll-base`，会使用上游 TV 轮询默认值。
当 `plan`、`playback` 或 `download` 需要使用 TV playurl host 而不是默认 web playurl host
时，可同时使用 `--playurl-mode tv` 和 `--tv-api-base`。
当 `plan`、`playback` 或 `download` 需要使用 APP gRPC playurl host 时，可同时使用
`--playurl-mode app`、`--app-grpc-base` 和 `--app-pgc-grpc-base`。

通过显式代理主机配置受限区域 PGC playurl 回退。只有官方 PGC playurl 响应报告区域限制，
或 APP gRPC mode 报告 permission-denied status / preview-only PGC response-body 信号时
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
`live-e2e.samples.example.json` 开始，并列出要探测的 public、PGC、intl 或 restricted PGC
样例。匿名 PGC metadata-only 检查请参考 `live-e2e.pgc-metadata.samples.example.json`；其中
`ep`、`ss` 和 `md` info 样例使用按 capability 声明的 metadata service，不请求 playurl 或媒体。
`credential_file` 支持旧版 BBDown 或 Telegram Video Downloader 扁平凭据文件，以及
新版 BBDown versioned credential store。可在 manifest 中选择 `credential_profile`；case 中的
`credential_file` 或 `credential_profile` 会分别独立覆盖 manifest 的对应值。省略 profile 时
使用所选 store 的默认 profile。未知或空 profile、指定 profile 但缺少凭据文件，以及空或损坏
的显式 store 都会报错；没有凭据来源或 profile 时仍允许匿名样例。可选的
`access_key_file` 从纯文本文件读取通用 key；case 级文件会覆盖 manifest 级文件，且其 trim
后非空的值会覆盖所选 profile 的
`access_key`。

源凭据文件只读。live 测试框架会把所选 profile 的 cookie、通用 access key 和 TV access key
写入每个 case 独立的临时凭据存储；不会复制其他 profile 或凭据生命周期 metadata。运行前会移除
CLI 覆盖环境变量，因此样例行为由 manifest 驱动，而不是由 shell 状态驱动。

## 文档

- Crate API 说明：可发布 package 是 `bbdown-core`，导入时使用 `bbdown_core`。`0.7.0` 发布版
  增加了可选 CDN host 选择、探测和并行传输、内置网络目录，以及独立的 PGC Web playurl 路由
  API；目录不会触发自动 CDN 路由。单视频相关推荐和有界元数据分页 API 于 `0.10.0` 引入；`0.9` crate 不包含这些 API。`0.11.0` 源码版本增加搜索、PGC 时间表与索引、热门视频和排行榜 metadata API。请查看[crates.io](https://crates.io/crates/bbdown-core)了解 package 可用性，或使用当前源码检出或本地 path dependency。
  `0.5.0` 的 download API 仍包含
  `DownloadProgressEvent` callback、基于 `DownloadCancellationToken` 的 graceful
  cancellation，以及当 plan 条目暴露章节时的 ffmpeg 章节 metadata mux；嵌入项目应优先使用
  `Default`、`new` 和
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
- 版本说明：[docs/release-notes/README.zh-CN.md](docs/release-notes/README.zh-CN.md)
- 英文 README：[README.md](README.md)
- 英文用户指南：[docs/user-guide.md](docs/user-guide.md)
- 英文嵌入指南：[docs/embedding.md](docs/embedding.md)
- 英文架构指南：[docs/architecture/rust-rewrite.md](docs/architecture/rust-rewrite.md)
- 英文版本说明：[docs/release-notes/README.md](docs/release-notes/README.md)
- 面向 agent 的项目跟踪文档，不做本地化：[项目状态](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/PROJECT_STATE.md)、
  [项目 TODO](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/PROJECT_TODO.md)、
  [工作流日志](https://github.com/Joey-Project/BBDown-rust/tree/master/docs/project_journal/)。

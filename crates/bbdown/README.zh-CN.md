[ [English](README.md) | 简体中文 ]

# bbdown-core

`bbdown-core` 是 BBDown Rust 背后的可复用 Rust library package。Rust 代码中以
`bbdown_core` 导入。它把 Bilibili 和 Bilibili intl 输入解析为 typed metadata、下载计划、
媒体下载、单独输出下载模式、封面/字幕/XML 或 ASS 弹幕旁路文件、二维码登录凭据、下载归档预检查数
据、播放请求规格、批量集合 metadata，以及受限区域代理诊断。下载执行也可以通过
`MediaHostOptions` 应用显式 UPOS host 替换或 BBDown-like PCDN 规避，`*_with_progress` /
`*_with_cancellation` 下载方法会为嵌入 UI 发出 typed progress callback，并接收显式
cancellation token。原始输入解析覆盖普
通视频、PGC 和 intl 分集、PUGV/cheese 课程、B23 短链接、收藏夹、空间投稿、合集、系列、
首页推荐、单视频相关推荐、观看历史、稍后再看列表、关注视频 feed 和空间动态视频 feed。

使用 `cargo add bbdown-core` 从 crates.io 安装 crate，然后以 `bbdown_core` 导入。
`0.8.0` 增加了保留历史的弹幕刷新和账号身份 API。`0.9.0` 增加了类型化 CDN 传输字节和诊断 progress event（包括
`probe_media_cdns_with_progress` API），以及带 offset 续页的 WBI 签名个人空间动态 feed 请求。
传输字节事件统计应用已消费的响应正文，与写入或发布到文件的字节不同，也不代表全部线路传输量。

单视频相关推荐 API 和仅支持 `FollowingFeed`、`SpaceDynamic` 的有界元数据分页 API 于
`bbdown-core` `0.10.0` 引入。此版本的认证 SpaceDynamic feed 解析可为临时请求 header 初始化缺失的设备
Cookie 值。元数据分页不改变完整 `resolve`/`info` 行为。使用这些 API 需要 `0.10` 依赖版本线；`0.9` 依赖不包含这些
API。如果 registry 中尚无 `0.10.0` package，请使用对应的源码检出或本地 path dependency。
`0.11.0` 源码版本增加搜索 API（`VideoSearchQuery` / `BiliClient::search_videos`）、PGC 时间表与索引 metadata、热门视频 metadata 和全站排行榜 metadata API，也增加国内 PGC 分集分组。请查看[crates.io](https://crates.io/crates/bbdown-core)了解当前 package 可用性；[官方 Releases](https://github.com/Joey-Project/BBDown-rust/releases)页面列出 CLI 归档。
当前源码检出也支持嵌入应用为国内分集/season 与 media-review hydration 显式选择按 capability 声明的 PGC metadata service；若服务单独声明 `PgcMetadataCapability::Timetable`，也可用于时间表 metadata。该路由仅访问所声明 endpoint，不覆盖索引列表、搜索、PUGV 或 Intl metadata；请求与响应边界见仓库嵌入指南。

crate 提供 staged XML 历史保留弹幕刷新，按有效的正 `p[7]` ID 去重；ID 缺失、为零或无效时，使用完整
`p` 属性和解码文本作为匹配依据。它从合并 XML 重新生成 ASS，同时包含 CDN 控制和独立 PGC Web 路由 API。PGC 区域与代理解析独立于媒体
CDN 选择与传输。嵌入项目
应优先使用 constructor 和 builder 风格 API，例如 `ClientConfig::default().with_*()`、
`EndpointConfig::default().with_*()`、`RestrictedAreaConfig::default().with_*()`、
`DownloadOptions::new(...).with_*()`、`RetryPolicy::new(...)` 和
`StreamSelection::new(...)`，而不是对随着 crate 成熟可能继续增长的配置值使用结构体字面
量。Public plan output containers 是被消费的数据表面，之后可能继续新增字段。

## 示例

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadMode, DownloadOptions, DownloadPathTemplates,
    MediaHostOptions, RetryPolicy, Selection, StreamSelection,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download_with_mode(
            "BV1qt4y1X7TW",
            Some(Selection::Current),
            DownloadMode::VideoOnly,
        )
        .await?;
    let options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::video(80))
        .with_download_mode(DownloadMode::VideoOnly)
        .with_path_templates(
            DownloadPathTemplates::new()
                .with_output_dir("{title}-{entry_count:02}")
                .with_entry_dir("{index:03}-{entry_title}-{content_id}"),
        )
        .with_media_hosts(
            MediaHostOptions::new().with_upos_host("upos-sz-mirrorcoso1.bilivideo.com"),
        )
        .with_retry_policy(RetryPolicy::new(3, Duration::from_millis(250)));

    println!("{} entries", plan.entries.len());
    println!("download output root: {}", options.output_dir.display());
    Ok(())
}
```

`fav456`、`mid123`、`collection456`、`series456`、`recommendations`、`related:<video>`、`history`、
`watch-later`、`following` 和空间动态 URL 等批量输入会通过
`ResolvedContent::Collection` 返回；
`resolve_input` 会保留完整解析到的 collection metadata，`selected_items` 则携带当前选中子
集。选中的条目随后通过普通视频 pipeline 规划和下载。嵌入应用可以用
`Selection::Page(index)` 选择单个条目，或用 `Selection::Indices(...)` 配合
`IndexSelection` / `IndexSelector` 表示类似 `1,3-5` 的列表和范围选择。推荐输入会拉取首
页推荐批次，目前只输出普通视频 `av` 卡片，并在显式 index selection 需要时在安全上限内继
续请求后续 `fresh_idx` 刷新批次来覆盖过滤后的条目。观看历史、稍后再看和动态 feed 输入
需要在 client credentials 中提供 cookie；观看历史目前只输出普通视频 `archive` 记录，稍后
再看会输出该账号稍后再看列表中的普通视频，动态 feed 目前只输出普通视频 archive 卡片。
单视频相关推荐可通过 `resolve_input("related:<video>", ...)` 查询；Rust 输入/API 分别为
`Input::RelatedRecommendations(aid)` 和 `BiliClient::resolve_related_recommendations(aid,
selection)`。`<video>` 可为 aid/`av`/`BV` 标识或 B 站官方桌面站/移动站视频 URL（包括
`https://m.bilibili.com/video/...`）。它只读取一批有限结果，保留上游顺序，过滤源视频、无效和 OGV 条目，并按 aid
去重。`Selection` 作用于这份相关推荐列表；接口不会翻页或递归请求。它复用普通 web API
client，也可能发送已配置的 cookie；上游可能限制匿名请求。
单视频相关推荐 API 于 `bbdown-core` `0.10.0` 引入；`0.9.0` 不包含此 API。请查看[crates.io](https://crates.io/crates/bbdown-core)了解 package 可用性，或使用匹配的源码检出或本地 path dependency。

library 默认保留 plan 中的媒体 URL。嵌入应用需要自定义 UPOS host、强制替换，或
CLI-compatible PCDN fallback 处理时，应显式设置 `MediaHostOptions`。
服务器需要验证登录或续期是否属于同一账号时，可使用
`BiliClient::credential_account_identity(CredentialKind::Cookie)`，或选择
`AccessKey`/`TvAccessKey`。返回的身份数据只用于服务器私有账号绑定，不是面向客户端的
健康报告，也不授权调用方覆盖凭据。
已知通用 key 的签发 keypair（用于请求签名的应用密钥对）时，可调用
`BiliClient::access_key_account_identity_with_keypair(AccessKeyIdentityKeypair::Android)`，
也可选择 `IntlBstar`、`BiliTv` 或 `AndroidB`。缺少签发信息时，通用身份方法只会在明确的
签名或应用鉴权拒绝后，尝试最多四组 keypair；所有尝试共享一次请求超时预算，不会在身份
响应异常或网络失败后继续探测。
BiliTv 签名遵循 `tv_passport_poll_base` 配置；若该地址保持默认且只修改了
`passport_base`，则与原有刷新路由一样，兼容使用主 passport 地址。
这些新增身份 API 包含在已发布的 `0.8.0` 版本中，`0.7.0` crate 不提供这些 API。
当嵌入应用需要给下游 streaming/cache service 使用的可序列化 DASH video/audio 或 FLV
segment 请求规格时，使用 `BiliClient::plan_playback`。`PlaybackPlan` 包含主 URL、备用
URL、媒体 headers、mime/codec metadata、时长、大小、entry/variant/media cache key，以及
codec/mime-compatible ABR group/level metadata，但不实现播放器状态、HLS playlist 生成或
HTTP segment serving。每个 `PlaybackVariant` 还包含 `selection_hints.avplayer`，提供 exact
codec 字符串（已知时）、codec-family metadata、`format_key` 和排序信号。嵌入客户端需要优先
H.264、HEVC、AV1 或其它 codec 顺序时，可以使用 `PlaybackCodecPreference`。
当嵌入应用需要给普通视频或 PGC 分集使用 BBDown-compatible TV HTTP playurl 解析时，可设置
`ClientConfig::with_playurl_mode(PlayurlMode::Tv)` 和 `EndpointConfig::with_tv_api_base`。
TV mode 使用 `Credentials::tv_access_key`，不会复用通用 intl access key。
当嵌入应用需要 BBDown-compatible APP gRPC playurl 解析时，可设置
`ClientConfig::with_playurl_mode(PlayurlMode::App)`、
`EndpointConfig::with_app_grpc_base` 和 `EndpointConfig::with_app_pgc_grpc_base`。APP mode
适用于普通视频和 PGC 分集。它会优先使用 Bilibili main/BALH 通用
`Credentials::access_key`，再回退到 `Credentials::tv_access_key`；intl OAuth key 和缺少
provider metadata 的 legacy profile 会保持 TV-key-first 行为。APP mode 会输出规范化后的
`StreamSet` / `PlaybackPlan` 媒体规格；PGC
出现 restricted 或 preview-only 信号时，仍可回退到已配置的 restricted-area HTTP playurl
proxy，且 proxy URL 只使用通用 access key。这些信号可以来自区域限制消息、APP
permission-denied gRPC status 或 PGC response-body metadata。APP gRPC 非零 status 会读取 initial headers 和
trailing metadata；APP DASH 的分辨率和帧率 metadata 会保留到规范化媒体规格；APP 数值
codec id 会暴露为 `codec_family` metadata，不会伪造 exact MP4 codec 字符串；legacy FLV
segment 响应会规范化为最高质量的一组 segment。
嵌入应用需要 BBDown 风格或应用自定义输出名时，可以设置 `DownloadPathTemplates`。模板会
为输出根目录、条目目录和 mux 后文件名 stem 渲染经过清洗的文件名组件；媒体和 sidecar 文
件名保持稳定，以支持续传和归档记录。条目模板必须在选中条目之间渲染出唯一目录名。

受限区域代理、端点覆盖、凭据、下载归档和下载执行示例见仓库嵌入指南：
[英文](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/embedding.md) /
[简体中文](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/embedding.zh-CN.md)。

CLI 包通过 GitHub release 归档分发。此工作区的 crates.io dry-run 目标是 `bbdown-core` library
package。

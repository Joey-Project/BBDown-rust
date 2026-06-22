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
首页推荐、观看历史、稍后再看列表、关注视频 feed 和空间动态视频 feed。

使用 `cargo add bbdown-core` 安装，然后用 `bbdown_core` 导入。

当前 crate 版本是 `0.5.0`，属于已发布 `0.4.0` 之后的开发线，重点是 downloader 和
embedding polish：progress callback、可取消的执行、章节 metadata mux、音频语言选择，以及 AI 字幕筛选。嵌入项目
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

`fav456`、`mid123`、`collection456`、`series456`、`recommendations`、`history`、
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

library 默认保留 plan 中的媒体 URL。嵌入应用需要自定义 UPOS host、强制替换，或
CLI-compatible PCDN fallback 处理时，应显式设置 `MediaHostOptions`。
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
适用于普通视频和 PGC 分集，优先使用 `Credentials::access_key`，再回退到
`Credentials::tv_access_key`，并输出规范化后的 `StreamSet` / `PlaybackPlan` 媒体规格；PGC
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

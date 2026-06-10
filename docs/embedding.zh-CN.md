[ [English](embedding.md) | 简体中文 ]

# 嵌入指南

## 范围

`bbdown_core` crate 通过 `bbdown-core` package 发布，是 Rust 项目的集成表面，适用于需要
typed Bilibili metadata、下载计划、媒体下载、字幕旁路文件、弹幕旁路文件、二维码登录状态
、批量集合解析和受限区域代理诊断，但不希望 shell out 到 CLI 的场景。

当前 crate 版本是 `0.2.0`，属于已发布 `0.1.0` 之后的开发线，并通过
`ResolvedContent::Collection` 增加批量 collection metadata。配置应优先使用构造器和
builder 风格 API，并把 metadata 和 plan 结构体视为只读输出表面。这样在 crate 成熟过程
中新增字段时，嵌入代码更不容易受影响。

## 仅规划

对原始 CLI 风格输入使用 `BiliClient::plan_download`，也可以自行解析 `Input` 后调用
`BiliClient::plan`。

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, Selection};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download("https://www.bilibili.com/video/BV1qt4y1X7TW", Some(Selection::Current))
        .await?;

    for entry in &plan.entries {
        println!("{}: {} streams", entry.title, entry.streams.videos.len());
    }

    Ok(())
}
```

Season 和 media 输入需要显式 `Selection`，除非输入本身已经标识某个分集。这是有意设计，
避免 library 意外规划整季内容。

## 批量和集合输入

`BiliClient::resolve_input` 接受 CLI 风格原始输入，例如 B23 短链接、`fav...`、`mid...`、
`collection...`、`series...`、canonical 收藏夹 `/list/ml...` URL、path-based
`/medialist/.../ml...` URL、空间合集 URL 和空间系列 URL。批量输入会解析为
`ResolvedContent::Collection`，其中包含完整 collection metadata 和选中的条目。
owner-scoped 空间列表 URL 会保留 uploader mid，让解析器可以使用较新的空间合集和系列
API。不带 selector 时，集合类输入会选择全部解析条目；传入 `Selection::Page(index)` 可
选择一个条目，传入 `Selection::Latest` 可选择上游列表顺序中的第一个解析条目。空集合会表示为空 item
列表，而不是 missing-field 错误。

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, ResolvedContent, Selection};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let resolved = client
        .resolve_input("fav456", Some(Selection::Page(1)))
        .await?;

    if let ResolvedContent::Collection(collection) = resolved {
        println!(
            "{} selected from {}",
            collection.selected_items.len(),
            collection.collection.title
        );
    }

    Ok(())
}
```

下载规划会把选中的集合条目映射回普通视频条目，因此下游下载执行、归档重复检查、stream
selection、字幕和弹幕旁路文件仍使用与普通视频下载相同的 API。因为 `DownloadPlan` 不暴
露 collection metadata，规划阶段可以只抓取选中的批量条目集合。PUGV/cheese 分集输入会
解析为 season；当 API 报告还有更多 episode 页时会继续跟进分页，并通过
`StreamSource::PugvWeb` 规划。

## 凭据

嵌入项目可以使用 `CredentialStore`，也可以从自己的存储注入凭据。不要记录原始凭据值。
`Credentials` 的 debug 输出会脱敏，但应用日志仍应把凭据视为密钥。

```rust,no_run
use bbdown_core::{ClientConfig, Credentials};

let credentials = Credentials::default()
    .with_cookie("SESSDATA=...")
    .with_access_key("...");

let config = ClientConfig::default().with_credentials(credentials);
```

## 受限区域 PGC 规划

crate 不会内置公共代理默认值。只配置你自己运营或信任的代理主机。受限区域回退只会针对
PGC playurl 区域错误尝试，而不是针对任意官方 API 失败。

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, RestrictedArea, RestrictedAreaConfig, RestrictedAreaProxy, Selection,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let restricted_area = RestrictedAreaConfig::default()
        .with_area_hint(RestrictedArea::Hk)
        .with_proxy(RestrictedAreaProxy::playurl(
            "https://proxy.example/playurl",
            Some(RestrictedArea::Hk),
        ))
        .with_proxy(RestrictedAreaProxy::bilibili_api(
            "https://api-proxy.example",
            Some(RestrictedArea::Tw),
        ));

    let client = BiliClient::new(ClientConfig::default().with_restricted_area(restricted_area));
    let plan = client.plan_download("ep664928", Some(Selection::Current)).await?;

    println!("planned {} entries", plan.entries.len());
    Ok(())
}
```

回退成功时，条目会报告 `StreamSource::PgcProxy` 并包含解析器诊断。诊断端点会压缩到
origin，诊断消息会脱敏常见密钥模式。

## 下载执行

下载是显式动作。library 默认禁用 mux，因此嵌入应用不会在未选择的情况下启动 `ffmpeg`。

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, DownloadOptions, MuxOptions, RetryPolicy, StreamSelection};
use std::time::Duration;

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::video(80))
        .with_retry_policy(RetryPolicy::new(3, Duration::from_millis(250)))
        .with_download_idle_timeout(Some(Duration::from_secs(30)))
        .with_subtitles(true)
        .with_danmaku(true)
        .with_mux(MuxOptions::ffmpeg("ffmpeg"));

    let report = client
        .download_input("BV1qt4y1X7TW", None, options)
        .await?;

    println!("wrote {} entries", report.entries.len());
    Ok(())
}
```

当 UI 需要展示质量选择时，先使用 `bbdown plan` 或 `BiliClient::plan_download`。
`StreamSelection::video`、`StreamSelection::audio` 和 `StreamSelection::new` 会从 plan 中
选择精确的 DASH stream id。

## 下载归档和重复决策

嵌入应用应保持重复处理显式。用 `DownloadPreflight` 检查计划，把已有归档记录或输出冲突
展示给用户，然后用同一份 preflight 和用户选择的 `DuplicateDecision` 调用 executor。crate
不会提示用户。如果应用在展示和执行之间序列化 preflight，请存储完整 preflight 对象，这样
`KeepBoth` 仍会避开检查时保留的 archive-only 输出目录。executor 会在应用决策前校验
preflight 仍匹配当前归档，因此当另一个进程可能更新归档时，调用方应重新检查。

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadArchive, DownloadOptions, DownloadPreflight,
    DuplicateDecision, MuxOptions,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client.plan_download("BV1qt4y1X7TW", None).await?;
    let options = DownloadOptions::new("downloads").with_mux(MuxOptions::Disabled);
    let archive_path = "downloads/archive.json";
    let mut archive = DownloadArchive::load(archive_path)?;
    let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;

    if preflight.requires_decision() {
        println!(
            "{} possible duplicate records",
            preflight.archived_records.len()
        );
        if let Some(conflict) = &preflight.output_conflict {
            println!("output exists: {}", conflict.path.display());
        }
    }

    let decision = if preflight.requires_decision() {
        DuplicateDecision::KeepBoth
    } else {
        DuplicateDecision::Cancel
    };
    let report = client
        .download_plan_with_archive_preflight_decision(
            &plan,
            options,
            &mut archive,
            &preflight,
            decision,
        )
        .await?;
    archive.save(archive_path)?;

    println!("wrote {}", report.output_dir.display());
    Ok(())
}
```

`DuplicateDecision::Replace` 会在已有计划输出根目录存在时先删除它，再重新下载；完成后的记
录会替换指向同一输出路径的陈旧归档记录。`DuplicateDecision::KeepBoth` 会写入下一个带后
缀的输出根目录，并保留旧归档记录，包括旧输出目录已被移除的 archive-only 记录。如果 UI
在重复预检查后选择取消，应在 preflight 后停止，不要调用下载 executor。对无冲突 preflight
传入 `DuplicateDecision::Cancel` 是安全的继续路径：如果输出冲突在执行前出现，executor
会报告它，而不是隐式替换新的输出根目录。归档记录包含内容身份、绝对输出路径、条目 id、
绝对旁路文件路径、绝对 mux 输出路径和完成时间戳；不包含媒体 URL 或凭据。条目身份使用
aid/cid 媒体 id，而不是可选 BVID 或分集 id，因此通过分集 URL 规划的 PGC 分集可以匹配之
后同一媒体的 BV/av 计划，即便其中一个计划缺少 BVID。
`DownloadPreflight::inspect` 也会把相同计划输出路径的归档记录视为重复，即便内容身份不同
且旧输出目录已经不在磁盘上。把归档存放在所选输出根目录和任何归档保存旁路路径之外的
JSON 文件路径；`DownloadArchive::save` 会拒绝目录目标。如果归档路径是符号链接，
`DownloadArchive::save` 会更新符号链接目标，而不是替换链接本身。

## 端点覆盖

测试、本地 mock 或受控 gateway 部署可使用 endpoint builder。

```rust,no_run
use bbdown_core::{ClientConfig, EndpointConfig};

let endpoints = EndpointConfig::default()
    .with_api_base("http://127.0.0.1:8080")
    .with_pgc_base("http://127.0.0.1:8080")
    .with_comment_base("http://127.0.0.1:8081");

let config = ClientConfig::default().with_endpoints(endpoints);
```

## 兼容性建议

- 使用 `Default`、`new` 和 `with_*` 方法构造配置值，而不是结构体字面量。
- 通过字段访问或 serde 序列化读取输出模型；除非测试确实需要本地 fixture，否则避免在下
  游代码中构造输出结构体。
- 不要把凭据和二维码登录扫码 URL 写入日志或 crash report。
- 把受限区域代理主机视为可信基础设施，因为媒体 URL 和 access key 可能经过这些主机。

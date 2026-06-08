[ [English](README.md) | 简体中文 ]

# bbdown

`bbdown` 是 BBDown Rust 背后的可复用 Rust library。它把 Bilibili 和 Bilibili intl 输入解析
为 typed metadata、下载计划、媒体下载、字幕、弹幕旁路文件、二维码登录凭据、下载归档预
检查数据，以及受限区域代理诊断。

crate 仍在准备第一次 crates.io 发布。这个预发布分支会在发布前刻意加固 public structs：
嵌入项目应优先使用 constructor 和 builder 风格 API，例如
`ClientConfig::default().with_*()`、`EndpointConfig::default().with_*()`、
`RestrictedAreaConfig::default().with_*()`、`DownloadOptions::new(...).with_*()`、
`RetryPolicy::new(...)` 和 `StreamSelection::new(...)`，而不是对可能在 minor release 之间
增长的配置值使用结构体字面量。Public plan output containers 是被消费的数据表面，之后可
能会标记为 non-exhaustive。

## 示例

```rust,no_run
use bbdown::{BiliClient, ClientConfig, DownloadOptions, RetryPolicy, Selection, StreamSelection};
use std::time::Duration;

#[tokio::main]
async fn main() -> bbdown::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download("BV1qt4y1X7TW", Some(Selection::Current))
        .await?;
    let options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::video(80))
        .with_retry_policy(RetryPolicy::new(3, Duration::from_millis(250)));

    println!("{} entries", plan.entries.len());
    println!("download output root: {}", options.output_dir.display());
    Ok(())
}
```

受限区域代理、端点覆盖、凭据、下载归档和下载执行示例见仓库嵌入指南：
[英文](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/embedding.md) /
[简体中文](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/embedding.zh-CN.md)。

CLI 包通过 GitHub release 归档分发。此工作区的 crates.io dry-run 目标是 `bbdown` library
crate。

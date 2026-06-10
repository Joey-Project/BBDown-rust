[ [English](README.md) | 简体中文 ]

# bbdown-core

`bbdown-core` 是 BBDown Rust 背后的可复用 Rust library package。Rust 代码中以
`bbdown_core` 导入。它把 Bilibili 和 Bilibili intl 输入解析为 typed metadata、下载计划、
媒体下载、字幕、弹幕旁路文件、二维码登录凭据、下载归档预检查数据、批量集合 metadata，
以及受限区域代理诊断。原始输入解析覆盖普通视频、PGC 和 intl 分集、PUGV/cheese 课程、
B23 短链接、收藏夹、空间投稿、合集和系列。

使用 `cargo add bbdown-core` 安装，然后用 `bbdown_core` 导入。

当前 crate 版本是 `0.2.0`，属于已发布 `0.1.0` 之后的开发线，并通过
`ResolvedContent::Collection` 增加批量 collection metadata。嵌入项目应优先使用
constructor 和 builder 风格 API，例如 `ClientConfig::default().with_*()`、
`EndpointConfig::default().with_*()`、`RestrictedAreaConfig::default().with_*()`、
`DownloadOptions::new(...).with_*()`、`RetryPolicy::new(...)` 和
`StreamSelection::new(...)`，而不是对随着 crate 成熟可能继续增长的配置值使用结构体字面
量。Public plan output containers 是被消费的数据表面，之后可能继续新增字段。

## 示例

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, DownloadOptions, RetryPolicy, Selection, StreamSelection};
use std::time::Duration;

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
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

`fav456`、`mid123`、`collection456` 和 `series456` 等批量输入会通过
`ResolvedContent::Collection` 返回；`resolve_input` 会保留完整解析到的 collection
metadata，`selected_items` 则携带当前选中子集。选中的条目随后通过普通视频 pipeline 规
划和下载。

受限区域代理、端点覆盖、凭据、下载归档和下载执行示例见仓库嵌入指南：
[英文](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/embedding.md) /
[简体中文](https://github.com/Joey-Project/BBDown-rust/blob/master/docs/embedding.zh-CN.md)。

CLI 包通过 GitHub release 归档分发。此工作区的 crates.io dry-run 目标是 `bbdown-core` library
package。

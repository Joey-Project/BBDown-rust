# bbdown

`bbdown` is the reusable Rust library behind BBDown Rust. It resolves Bilibili and Bilibili intl
inputs into typed metadata, download plans, media downloads, subtitles, danmaku sidecars, QR login
credentials, and restricted-area proxy diagnostics.

The crate is still preparing for its first crates.io release. This pre-release branch intentionally
hardens public structs before publishing: embedding projects should prefer constructor and
builder-style APIs such as `ClientConfig::default().with_*()`, `EndpointConfig::default().with_*()`,
`RestrictedAreaConfig::default().with_*()`, `DownloadOptions::new(...).with_*()`,
`RetryPolicy::new(...)`, and `StreamSelection::new(...)` instead of struct literals for
configuration values that may grow between minor releases. Public plan output containers are
consumed data surfaces and may be marked non-exhaustive.

## Example

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

See the repository embedding guide for restricted-area proxy, endpoint override, credential, and
download execution examples:
<https://github.com/Joey-Project/BBDown-rust/blob/master/docs/embedding.md>.

The CLI package is distributed through GitHub release archives. The crates.io dry-run target for this
workspace is the `bbdown` library crate.

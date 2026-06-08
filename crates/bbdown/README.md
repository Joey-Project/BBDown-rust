# bbdown

`bbdown` is the reusable Rust library behind BBDown Rust. It resolves Bilibili and Bilibili intl
inputs into typed metadata, download plans, media downloads, subtitles, danmaku sidecars, QR login
credentials, and restricted-area proxy diagnostics.

The crate is still preparing for its first crates.io release. This pre-release branch intentionally
hardens public structs before publishing: embedding projects should prefer constructor and
builder-style APIs such as `ClientConfig::default()`, `ClientConfig::new(...).with_*()`, and
`DownloadOptions::new(...)` or `StreamSelection::new(...)` instead of struct literals for
configuration values that may grow between minor releases. Public plan output containers are
consumed data surfaces and may be marked non-exhaustive.

## Example

```rust,no_run
use bbdown::{BiliClient, ClientConfig, Selection};

#[tokio::main]
async fn main() -> bbdown::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download("BV1qt4y1X7TW", Some(Selection::Current))
        .await?;

    println!("{} entries", plan.entries.len());
    Ok(())
}
```

The CLI package is distributed through GitHub release archives. The crates.io dry-run target for this
workspace is the `bbdown` library crate.

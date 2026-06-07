# bbdown

`bbdown` is the reusable Rust library behind BBDown Rust. It resolves Bilibili and Bilibili intl
inputs into typed metadata, download plans, media downloads, subtitles, danmaku sidecars, QR login
credentials, and restricted-area proxy diagnostics.

The crate is still `0.1`; embedding projects should prefer constructor and builder-style APIs such
as `ClientConfig::default()` and `ClientConfig::new(...).with_*()` instead of struct literals for
configuration values that may grow between minor releases.

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

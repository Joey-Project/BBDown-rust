---
id: 20260612-019ebc-tv
title: TV Playurl Mode
status: completed
created: 2026-06-12
updated: 2026-06-12
branch: wip/tv-playurl-mode
pr:
supersedes: []
superseded_by:
---

# TV Playurl Mode

## Summary
- Added a crate and CLI switch for BBDown-compatible TV HTTP playurl resolution without changing
  the existing default web playurl behavior.
- Kept APP/gRPC playurl transport out of this slice because it needs protobuf framing and response
  normalization.

## Current State
- `ClientConfig::with_playurl_mode(PlayurlMode::Tv)` selects TV playurl mode.
- `EndpointConfig::with_tv_api_base` configures the TV API host for mocks, proxies, or the upstream
  default.
- Normal video planning can now emit `StreamSource::NormalTv`.
- PGC episode planning can now emit `StreamSource::PgcTv`.
- CLI callers can use `--playurl-mode tv` and `--tv-api-base`; environment overrides are
  `BBDOWN_PLAYURL_MODE` and `BBDOWN_TV_API_BASE`.
- TV playurl requests use `Credentials::tv_access_key` and do not reuse the generic intl access key.
- TV playurl requests do not send WEB cookies to the TV API or custom TV API proxy.

## Next Steps
- Add APP/gRPC playurl mode as a separate transport-specific slice.
- Continue feed/list resolver work for history, following/UP pages, recommendation pages, and
  watch-later.

## Evidence
- Targeted validation: `cargo test -p bbdown-core tv_playurl --lib`.
- Targeted validation: `cargo test -p bbdown-cli --test cli_e2e playback_json_uses_tv_playurl_mode`.
- Full validation: `just ci`.
- Internal review: helper-backed `codex-readonly` on
  `85f4ce274592d34d8434ecd09dd4877b412caabc..4fdea88149be7df4a234e61a5ad0cf312eddf050`, LGTM.
- PR review fix: helper-backed `codex-readonly` on
  `85f4ce274592d34d8434ecd09dd4877b412caabc..3643e2c12f7e96556880654fb9ffed4813373fcf`
  found that TV playurl requests leaked WEB cookies; fixed by using the no-cookie request path for
  `NormalTv` and `PgcTv`.
- PR review fix: helper-backed `codex-readonly` on
  `85f4ce274592d34d8434ecd09dd4877b412caabc..0434d63479517be5d77041137a8392973e1b62f8`
  found that the PGC TV unit test could hit the default real subtitle API; fixed by mocking
  `/x/player/v2` and pointing `api_base` at the mock server.

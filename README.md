# BBDown Rust

`BBDown Rust` is a Rust-native rewrite of BBDown with two goals:

- expose a reusable `bbdown` crate for other Rust projects;
- provide a CLI that can serve as the e2e surface for metadata resolution and downloads.

The current implementation establishes the crate/CLI/CI foundation, metadata resolver, stream
planning, media downloads, sidecar downloads, retry/resume behavior, optional ffmpeg muxing, QR
login, opt-in live test harnesses, and configured restricted-area proxy ordering with diagnostics.

## Current CLI

Resolve metadata as JSON:

```bash
bbdown info av170001 --json
bbdown info BV1qt4y1X7TW --json
bbdown info ep267851 --json
bbdown info ss26801 --select latest --json
bbdown info md22718131 --select latest --json
```

Build a download plan as JSON:

```bash
bbdown plan av170001 --json
bbdown plan ep267851 --json
bbdown plan ss26801 --select latest --json
bbdown plan https://www.bilibili.tv/en/play/34613/341736 --json
```

`plan` resolves the selected entries, available DASH or FLV stream URLs, subtitle URLs, and the
danmaku XML URL for each `cid`. PGC and intl planning may still require eligible account or region
access. PGC playurl resolution can fall back to user-configured restricted-area proxies. It does
not download files.

Download selected media files:

```bash
bbdown download av170001 --output-dir downloads
bbdown download ss26801 --select latest --output-dir downloads
bbdown download av170001 --output-dir downloads --no-mux --json
```

`download` resolves a plan, downloads the first complete DASH video/audio pair or FLV segments,
writes subtitle and danmaku sidecars by default, resumes partial files with HTTP range requests,
retries bounded transient failures, validates advertised media sizes when present, fails incomplete
media shapes, and runs `ffmpeg` unless `--no-mux` is supplied.

`ss` and `md` inputs require an explicit selection in non-interactive mode:

```bash
bbdown info ss26801 --select latest
bbdown info ss26801 --select all
bbdown info ss26801 --select episode:267851
bbdown info ss26801 --select page:1
```

Manage local credentials:

```bash
bbdown auth import-cookie --stdin
bbdown auth import-cookie --file cookie.txt
bbdown auth import-access-key --stdin
bbdown auth login-web
bbdown auth login-tv
bbdown auth status
bbdown auth logout
```

Credentials are stored in the platform config directory by default. Use
`--credential-file <path>` to override this path for integration tests or local experiments.
Secret import commands also read `BBDOWN_COOKIE` or `BBDOWN_ACCESS_KEY` when no input flag is
provided, so callers can avoid passing credentials through process arguments. QR login commands poll
the Bilibili QR state machine and save only the resulting credential. WEB QR login saves a cookie;
TV QR login saves a TV-specific access key without overwriting the generic intl/Bstar access key.
With `--json`, QR login emits newline-delimited JSON events: a `ticket` event with the scan URL
before polling, then a `saved` event after credentials are stored. Treat the scan URL as a temporary
login secret; status output and the `saved` event expose redacted booleans only.

Use `--request-timeout-seconds` or `BBDOWN_REQUEST_TIMEOUT_SECONDS` to tune API request bounds.
Media body reads use `--download-idle-timeout-seconds`; pass `0` to disable the idle timeout.
Use `--comment-base` or `BBDOWN_COMMENT_BASE` to point danmaku XML downloads at a mock or proxy
endpoint. Use `--passport-base` for WEB QR login mocks or proxies, and use `--tv-passport-base` /
`--tv-passport-poll-base` for TV QR login mocks or proxies. TV QR polling follows
`--tv-passport-base` only when that TV-specific override is supplied; otherwise it uses the upstream
TV poll default unless `--tv-passport-poll-base` is set explicitly.

Configure restricted-area PGC playurl fallback with explicit proxy hosts. Fallback runs only when the
official PGC playurl response reports a region/area restriction:

```bash
bbdown --restricted-area hk --restricted-area-proxy hk=https://proxy.example/playurl plan ep267851 --json
bbdown --restricted-api-proxy tw=https://proxy.example/bili/api plan ss26801 --select latest --json
```

`--restricted-area-proxy` targets BBDown/BiliPlus-style playurl proxy endpoints.
`--restricted-api-proxy` targets proxies that mirror `api.bilibili.com` paths and preserves query
parameters already present on the configured proxy base URL. Use repeated flags or comma-separated
`BBDOWN_RESTRICTED_AREA_PROXY` / `BBDOWN_RESTRICTED_API_PROXY` values to configure multiple
candidates. Repeated command-line flags preserve declaration order within the same area priority.
When command-line and environment proxy values are both present, command-line candidates are tried
first, followed by environment playurl proxies and then environment API-path proxies. Each source
group is ordered by area hint, generic candidates, then fixed area order. Hosts are user supplied;
the tool does not ship public proxy defaults. Proxy requests do not forward Bilibili cookies.
Resolver diagnostics reduce endpoints to URL origins and redact sensitive error-message values.

## Developer Commands

```bash
just fmt-check
just lint
just test
just e2e
just live-e2e
just ci
```

Default CI runs formatter, clippy, unit tests, and mock e2e tests. `just live-e2e` is intentionally
excluded from default CI and fails fast unless `BBDOWN_LIVE_URL` is set. It also accepts
`BBDOWN_LIVE_SELECTION`, `BBDOWN_LIVE_COOKIE`, and `BBDOWN_LIVE_ACCESS_KEY`.

## Documentation

- Crate API note: this rewrite is still `0.1`; embedding projects should prefer
  `ClientConfig::new(endpoints, credentials).with_*` over `ClientConfig { ... }` struct literals so
  added configuration fields are less disruptive. Output model structs such as `DownloadEntry` are
  intended to be consumed as returned values or through serde; their struct-literal construction is
  not treated as a stable compatibility surface before the crate leaves `0.1`.
- User guide: [docs/user-guide.md](docs/user-guide.md)
- Architecture: [docs/architecture/rust-rewrite.md](docs/architecture/rust-rewrite.md)
- Project state: [docs/PROJECT_STATE.md](docs/PROJECT_STATE.md)
- Project TODO: [docs/PROJECT_TODO.md](docs/PROJECT_TODO.md)
- Workstream journals: [docs/project_journal/](docs/project_journal/)

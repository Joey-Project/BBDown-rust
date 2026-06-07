# BBDown Rust

`BBDown Rust` is a Rust-native rewrite of BBDown with two goals:

- expose a reusable `bbdown` crate for other Rust projects;
- provide a CLI that can serve as the e2e surface for metadata resolution and downloads.

The current implementation establishes the crate/CLI/CI foundation, metadata resolver, stream
planning, subtitle discovery, and danmaku discovery. File downloads, muxing, QR login execution,
and restricted-area proxy ordering will land in later PR slices.

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
access. It does not download files yet.

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
bbdown auth status
bbdown auth logout
```

Credentials are stored in the platform config directory by default. Use
`--credential-file <path>` to override this path for integration tests or local experiments.
Secret import commands also read `BBDOWN_COOKIE` or `BBDOWN_ACCESS_KEY` when no input flag is
provided, so callers can avoid passing credentials through process arguments.

Use `--request-timeout-seconds` or `BBDOWN_REQUEST_TIMEOUT_SECONDS` to tune HTTP request bounds.

## Developer Commands

```bash
just fmt-check
just lint
just test
just e2e
just ci
```

Default CI runs formatter, clippy, unit tests, and mock e2e tests. Live Bilibili tests are
intentionally excluded from default CI and will be added behind explicit environment variables.

## Documentation

- User guide: [docs/user-guide.md](docs/user-guide.md)
- Architecture: [docs/architecture/rust-rewrite.md](docs/architecture/rust-rewrite.md)
- Project state: [docs/PROJECT_STATE.md](docs/PROJECT_STATE.md)
- Project TODO: [docs/PROJECT_TODO.md](docs/PROJECT_TODO.md)
- Workstream journals: [docs/project_journal/](docs/project_journal/)

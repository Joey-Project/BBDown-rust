# Rust Rewrite Architecture

## Goals

- Build a Rust crate that other projects can embed without shelling out to a CLI.
- Keep the CLI as the user-facing tool and e2e test surface.
- Preserve BBDown's practical Bilibili knowledge while replacing CLI-log parsing with typed data.
- Support normal videos, `ep`, `ss`, `md`, intl episodes, and configured restricted-area resolvers.

## Workspace

- `crates/bbdown`: library crate with typed input parsing, metadata models, credential store,
  client config, and resolver APIs.
- `crates/bbdown-cli`: CLI wrapper that uses the crate only through public APIs.
- `docs/`: architecture, user-facing notes, project state, and project journal entries.

## Resolver Model

Inputs normalize into `Input`:

- `Aid` and `Bvid` for normal videos.
- `Episode`, `Season`, and `Media` for Bilibili PGC URLs and ids.
- `IntlEpisode` for `bilibili.tv` episode URLs.

The library resolves metadata into `ResolvedContent`:

- `VideoMetadata` includes title, description, owner, tags, cover, pub time, and pages.
- `SeasonResolution` includes season metadata plus the selected episode set.

`ss` and `md` require a `Selection` in non-interactive contexts. The CLI will later add
interactive prompting, but the library keeps the contract explicit so integrations cannot
accidentally download a full season.

## Restricted Area And Intl

The project must not hard-code public proxy services. Restricted-area support is designed as a
configured resolver chain:

- official web and PGC APIs;
- intl API using caller-provided access key when available;
- user-configured proxy web or mobile resolver hosts;
- user-configured area hints such as `cn`, `hk`, `tw`, or `th`.

The current slice implements endpoint override and intl metadata shape. Later slices will add
typed stream resolvers, retry policy, and configured proxy candidate ordering based on the same
principles.

## Credentials

The CLI stores credentials in a local JSON file under the platform config directory with `0600`
permissions on Unix. The crate exposes `Credentials` and `CredentialStore` so other projects can
inject their own storage or keep credentials in memory.

Secrets are never included in status output; `auth status` reports only booleans.

## Testing And CI

Default CI is deterministic:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- unit tests
- CLI mock e2e tests

Live tests against Bilibili will be opt-in only. They must require explicit environment variables
for credentials and sample URLs so branch CI is not blocked by network, account, or regional state.

## Planned PR Slices

1. Workspace, CI, docs, metadata resolver, credential store, and CLI `info/auth`.
2. Stream resolver chain, download planning, subtitle and danmaku discovery.
3. File download, retry/resume policy, ffmpeg mux integration, and mock e2e downloads.
4. QR login state machine and live-test opt-in harness.
5. Restricted-area proxy resolver ordering and diagnostics.

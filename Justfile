set shell := ["bash", "-uc"]

default:
    just --list

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

e2e:
    cargo test -p bbdown-cli --test cli_e2e

live-e2e:
    test -n "${BBDOWN_LIVE_URL:-}" || { echo "BBDOWN_LIVE_URL is required for live-e2e" >&2; exit 2; }
    cargo test -p bbdown-cli --test live_e2e -- --ignored

ci: fmt-check lint test e2e

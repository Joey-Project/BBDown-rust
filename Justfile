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

publish-dry-run:
    cargo publish --dry-run -p bbdown --locked

live-e2e:
    test -f live-e2e.samples.json || { echo "live-e2e.samples.json is required for live-e2e; copy live-e2e.samples.example.json and fill local sample data" >&2; exit 2; }
    cargo test -p bbdown-cli --test live_e2e -- --ignored

ci: fmt-check lint test e2e publish-dry-run

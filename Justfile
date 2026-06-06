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

ci: fmt-check lint test e2e

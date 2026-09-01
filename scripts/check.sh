#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --all-targets --all-features
cargo test --all-features

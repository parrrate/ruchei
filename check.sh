#!/usr/bin/env bash
set -xe
cargo fmt --all
cargo test --workspace --features unstable
cargo clippy --workspace --all-targets --features unstable
cargo hack clippy --workspace --feature-powerset
exec ./doc.sh

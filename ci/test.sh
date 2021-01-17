#!/usr/bin/env bash
set -e

cargo fmt --manifest-path=kernel-rs/Cargo.toml -- --check -l
cargo clippy --manifest-path=kernel-rs/Cargo.toml
make qemu USERTEST=yes RUST_MODE=release

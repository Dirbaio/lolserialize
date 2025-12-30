#!/bin/bash

set -e

RUSTFLAGS=-Dwarnings cargo build

cargo test

cargo run --bin tester -- --lang rust
cargo run --bin tester -- --lang go
cargo run --bin tester -- --lang typescript

cargo run -p rpc-tester -- --all

#!/bin/bash

set -e

cargo run --bin tester -- --lang rust
cargo run --bin tester -- --lang go
cargo run --bin tester -- --lang typescript

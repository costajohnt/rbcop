#!/bin/sh
# Everything that must pass before a merge: formatting, lints, tests, and the oracle gate for every
# implemented cop. usage: RBCOP_CORPUS=... RBCOP_ORACLE=... scripts/verify.sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --release --all-targets -- -D warnings
cargo test --release
cargo build --release
scripts/check-cops.sh all
echo "verify: ok at $(git rev-parse --short HEAD) with $(target/release/rbcop --show-cops | grep -c '^[^ #].*:$' | tr -d ' ') cops"

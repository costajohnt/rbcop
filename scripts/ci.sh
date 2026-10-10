#!/bin/sh
# CI, run locally or on the self-hosted runner (private repo: no hosted Actions).
#   fast tier, always: cargo fmt --check, clippy -D warnings, release build, cargo test
#   corpus tier, when RBCOP_CORPUS and RBCOP_ORACLE are both set: check-cops.sh all (every
#   implemented cop against the recorded oracles on the nine corpora) and check-cold.sh (the cold-cop
#   spec corpus; its -A half runs RuboCop live, see scripts/rubocop.sh)
# usage: scripts/ci.sh   or   RBCOP_CORPUS=... RBCOP_ORACLE=... RUBOCOP=... scripts/ci.sh
# Prints `ci: ok at <sha>` on success; any failure exits non-zero.
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --release --all-targets -- -D warnings
cargo build --release
cargo test --release
if [ -n "${RBCOP_CORPUS:-}" ] && [ -n "${RBCOP_ORACLE:-}" ]; then
  scripts/check-cops.sh all
  scripts/check-cold.sh
else
  echo "ci: corpus tier skipped (set RBCOP_CORPUS and RBCOP_ORACLE to run it)"
fi
echo "ci: ok at $(git rev-parse --short HEAD)"

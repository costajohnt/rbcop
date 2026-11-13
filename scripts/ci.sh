#!/bin/sh
# CI, run locally, on the self-hosted runner, and (fast tier only) by .github/workflows/ci.yml.
#   fast tier, always: cargo fmt --check, clippy -D warnings, release build, cargo test
#   corpus tier, when RBCOP_CORPUS and RBCOP_ORACLE are both set: check-cops.sh all (every
#   implemented cop against the recorded oracles on the nine corpora) and check-cold.sh (the cold-cop
#   spec corpus; its -A half runs RuboCop live, see scripts/rubocop.sh), and check-plugins.sh (each
#   native plugin's cops against its own oracles, docs/plugins.md), and check-generated.sh (every
#   generated table and vendored gem config against its generator; needs $RUBOCOP's gem home on
#   ruby's path, see its header)
# usage: scripts/ci.sh   or   RBCOP_CORPUS=... RBCOP_ORACLE=... RUBOCOP=... scripts/ci.sh
# Prints `ci: ok at <sha>` on success (`<sha>-dirty` with uncommitted changes, which that commit
# does not hold); any failure exits non-zero. --locked: a stale Cargo.lock fails here, as it
# fails the release build, instead of being rewritten.
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --release --locked --all-targets -- -D warnings
cargo build --release --locked
cargo test --release --locked
if [ -n "${RBCOP_CORPUS:-}" ] && [ -n "${RBCOP_ORACLE:-}" ]; then
  scripts/check-cops.sh all
  scripts/check-cold.sh
  scripts/check-plugins.sh
  scripts/check-generated.sh
else
  echo "ci: corpus tier skipped (set RBCOP_CORPUS and RBCOP_ORACLE to run it)"
fi
echo "ci: ok at $(git describe --always --dirty --abbrev=7)"

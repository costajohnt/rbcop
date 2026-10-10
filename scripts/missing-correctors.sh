#!/bin/sh
# Cops that report [Correctable] (Cop::correctable) but never call add_fix: they need a corrector port.
# usage: scripts/missing-correctors.sh   (prints a comma-separated cop list)
cd "$(dirname "$0")/.."
for f in src/cops/*/*.rs; do
  if grep -q "fn correctable" "$f" && ! grep -q "add_fix" "$f"; then
    sed -n 's/^pub const NAME: &str = "\(.*\)";$/\1/p' "$f"
  fi
done | paste -sd, -

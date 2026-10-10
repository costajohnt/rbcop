#!/bin/sh
# Cops that report [Correctable] (Cop::correctable) but never call add_fix: they need a corrector port.
# A cop also counts as corrected when it uses a helper module that calls add_fix
# (`helpers::<name>`) or carries a `// corrector: <where>` marker line.
# ponytail: helper granularity is the file, so a cop using such a helper only for non-corrector
# functions is hidden too; match helper function names if that starts to matter.
# usage: scripts/missing-correctors.sh   (prints a comma-separated cop list)
cd "$(dirname "$0")/.."
fixing=$(grep -l add_fix src/helpers/*.rs | sed 's|src/helpers/\(.*\)\.rs|\1|' | paste -sd'|' -)
for f in src/cops/*/*.rs; do
  if grep -q "fn correctable" "$f" && ! grep -q "add_fix" "$f" && ! grep -q "^ *// corrector: " "$f" \
    && ! grep -Eq "helpers::($fixing)\b" "$f"; then
    sed -n 's/^pub const NAME: &str = "\(.*\)";$/\1/p' "$f"
  fi
done | paste -sd, -

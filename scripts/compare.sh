#!/bin/sh
# Diff rbcop against real RuboCop on a directory, using default config and only the cops rbcop implements.
# usage: scripts/compare.sh <dir>   (needs `rubocop` on PATH; with rbenv set RBENV_VERSION)
set -eu
dir=$(cd "$1" && pwd)
bin=$(cd "$(dirname "$0")/.." && pwd)/target/release/rbcop
out=${TMPDIR:-/tmp}/rbcop-compare
mkdir -p "$out"
cops=$("$bin" --list-cops | paste -sd, -)

# LineLength's [Correctable] tag depends on CheckLineBreakable, which rbcop does not model yet.
norm() { sed -E 's/\[Correctable\] (Layout\/LineLength)/\1/' | LC_ALL=C sort; }

cd "$dir"
rubocop --force-default-config --cache false --format emacs --only "$cops" . 2>/dev/null | norm > "$out/rubocop.txt" || true
"$bin" --only "$cops" . 2>"$out/rbcop.err" | norm > "$out/rbcop.txt" || true

echo "rubocop: $(wc -l < "$out/rubocop.txt") offenses   rbcop: $(wc -l < "$out/rbcop.txt") offenses"
LC_ALL=C comm -23 "$out/rubocop.txt" "$out/rbcop.txt" | sed 's/^/< /' > "$out/diff.txt"
LC_ALL=C comm -13 "$out/rubocop.txt" "$out/rbcop.txt" | sed 's/^/> /' >> "$out/diff.txt"
if [ ! -s "$out/diff.txt" ]; then
  echo "identical"
  exit 0
fi
echo "only in rubocop (<): $(grep -c '^<' "$out/diff.txt" || true)   only in rbcop (>): $(grep -c '^>' "$out/diff.txt" || true)"
sed -E 's/^([<>]).*: [CW]: (\[Correctable\] )?([A-Za-z]+\/[A-Za-z]+):.*/\1 \3/' "$out/diff.txt" | sort | uniq -c | sort -rn
echo "details: $out/diff.txt"

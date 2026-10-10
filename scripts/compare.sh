#!/bin/sh
# Diff rbcop against real RuboCop on a directory.
# usage: scripts/compare.sh [--project-config] <dir> [Cop1,Cop2,...]
#   default: both run with --force-default-config
#   --project-config: both use the directory's own .rubocop.yml files (plugins must be installed for RuboCop)
#   cops default to every cop rbcop implements
# RuboCop from $RUBOCOP (see scripts/rubocop.sh). Results go to $TMPDIR/rbcop-compare/.
set -eu
. "$(dirname "$0")/rubocop.sh"
mode=--force-default-config
if [ "${1:-}" = "--project-config" ]; then
  mode=
  shift
fi
dir=$(cd "$1" && pwd)
bin=$(cd "$(dirname "$0")/.." && pwd)/target/release/rbcop
out=${TMPDIR:-/tmp}/rbcop-compare/$(basename "$dir")${mode:+-default}
mkdir -p "$out"
# Lint/RedundantCopDisableDirective is left out of the default: --only rejects it, and both tools
# would then print nothing and compare as "identical".
cops=${2:-$("$bin" --show-cops | sed -n 's/^\([^ #].*\):$/\1/p' | grep -vx 'Lint/RedundantCopDisableDirective' | paste -sd, -)}

# LineLength's [Correctable] tag depends on CheckLineBreakable, which rbcop does not model yet.
# rubocop-md lints Ruby inside Markdown; rbcop does not, so .md results are dropped.
norm() { grep -v '\.md:[0-9]*:[0-9]*: ' | sed -E 's/\[Correctable\] (Layout\/LineLength)/\1/' | LC_ALL=C sort; }

cd "$dir"
if [ -n "$mode" ]; then
  rubocop $mode --cache false --format emacs --only "$cops" . 2>"$out/rubocop.err" | norm > "$out/rubocop.txt" || true
  "$bin" $mode --format emacs --only "$cops" . 2>"$out/rbcop.err" | norm > "$out/rbcop.txt" || true
else
  # --only would force-enable the cops, so run everything the config enables and keep rbcop's cops.
  pattern=": [CWEFRI]: (\[Correctable\] )?(Lint/Syntax|$(echo "$cops" | tr , '|')): "
  rubocop --cache false --format emacs . 2>"$out/rubocop.err" | grep -E "$pattern" | norm > "$out/rubocop.txt" || true
  "$bin" --format emacs . 2>"$out/rbcop.err" | grep -E "$pattern" | norm > "$out/rbcop.txt" || true
fi

echo "rubocop: $(wc -l < "$out/rubocop.txt") offenses   rbcop: $(wc -l < "$out/rbcop.txt") offenses"
LC_ALL=C comm -23 "$out/rubocop.txt" "$out/rbcop.txt" | sed 's/^/< /' > "$out/diff.txt"
LC_ALL=C comm -13 "$out/rubocop.txt" "$out/rbcop.txt" | sed 's/^/> /' >> "$out/diff.txt"
if [ ! -s "$out/diff.txt" ]; then
  echo "identical"
  exit 0
fi
echo "only in rubocop (<): $(grep -c '^<' "$out/diff.txt" || true)   only in rbcop (>): $(grep -c '^>' "$out/diff.txt" || true)"
sed -E 's/^([<>]).*: [CWEFRI]: (\[Correctable\] )?([A-Za-z]+\/[A-Za-z0-9]+):.*/\1 \3/' "$out/diff.txt" | sort | uniq -c | sort -rn | head -40
echo "details: $out/diff.txt"
exit 1

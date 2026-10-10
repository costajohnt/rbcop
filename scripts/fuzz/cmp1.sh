#!/bin/sh
# Compare RuboCop and rbcop on files (all cops, rbcop-oracle format); prints a diff of each report.
# usage: scripts/fuzz/cmp1.sh [-o Cop1,Cop2] <file>...   (run with RBENV_VERSION=4.0.6)
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
only=
if [ "${1:-}" = "-o" ]; then
  only="--only $2"
  shift 2
fi
for file in "$@"; do
  dir=$(cd "$(dirname "$file")" && pwd)
  f=$(basename "$file")
  (cd "$dir" && rubocop --config "$root/config/all-cops.yml" --cache false -r "$root/scripts/oracle_formatter.rb" \
    -f RuboCop::Formatter::RbcopOracleFormatter $only "$f" 2>&1 | sort > "/tmp/cmp1.$$.a") || true
  (cd "$dir" && "$root/target/release/rbcop" --config "$root/config/all-cops.yml" --format rbcop-oracle $only "$f" 2>&1 \
    | sort > "/tmp/cmp1.$$.b") || true
  if diff "/tmp/cmp1.$$.a" "/tmp/cmp1.$$.b" > "/tmp/cmp1.$$.d"; then echo "$f: same"; else echo "$f:"; cat "/tmp/cmp1.$$.d"; fi
done
rm -f "/tmp/cmp1.$$.a" "/tmp/cmp1.$$.b" "/tmp/cmp1.$$.d"

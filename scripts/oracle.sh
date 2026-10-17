#!/bin/sh
# Record RuboCop's output on a corpus with EVERY cop enabled, for cop-by-cop comparison
# without re-running RuboCop (see scripts/check-cops.sh).
# usage: scripts/oracle.sh <corpus-dir> <out-file>   (RuboCop from $RUBOCOP, see scripts/rubocop.sh)
#   env: CONFIG (default config/all-cops.yml; config/all-cops-performance.yml for plugin oracles)
set -eu
. "$(dirname "$0")/rubocop.sh"
dir=$(cd "$1" && pwd)
out=$2
cfg=${CONFIG:-$(cd "$(dirname "$0")/.." && pwd)/config/all-cops.yml}
fmt=$(cd "$(dirname "$0")" && pwd)/oracle_formatter.rb
# RuboCop rejects cache paths under a symlink (macOS /var), and --parallel then silently drops
# results, so keep the cache next to the output.
cache="$out.cache"
rm -rf "$cache"
cd "$dir"
rubocop -c "$cfg" --cache true --cache-root "$cache" --parallel -r "$fmt" -f RuboCop::Formatter::RbcopOracleFormatter . 2>"$out.err" | LC_ALL=C sort > "$out" || true
rm -rf "$cache"
echo "$(wc -l < "$out" | tr -d ' ') offenses recorded in $out"
# Cops that crashed on a file report nothing there; check-cops.sh skips those (cop, file) pairs.
sed -n 's/^An error occurred while \([^ ]*\) cop was inspecting \(.*\):[0-9]*:[0-9]*\.$/\1	\2/p' "$out.err" | sort -u > "$out.crashes"
echo "$(wc -l < "$out.crashes" | tr -d ' ') cop crashes recorded in $out.crashes"

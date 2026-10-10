#!/bin/sh
# Time RuboCop and rbcop on a directory with the same cops and default config.
# usage: scripts/bench.sh <dir>   (with rbenv set RBENV_VERSION)
set -eu
dir=$(cd "$1" && pwd)
bin=$(cd "$(dirname "$0")/.." && pwd)/target/release/rbcop
cops=$("$bin" --list-cops | paste -sd, -)
cache=$(mktemp -d)
cd "$dir"

secs() { start=$(date +%s.%N 2>/dev/null || python3 -c 'import time; print(time.time())'); "$@" > /dev/null 2>&1 || true; python3 -c "import time; print(f'{time.time() - $start:.2f}s')"; }

echo "files: $("$bin" --list-files . | wc -l | tr -d ' ')   cops: $cops"
echo "rubocop, one process:          $(secs rubocop --force-default-config --cache false --format quiet --only "$cops" .)"
echo "rubocop --parallel, cold cache: $(secs rubocop --force-default-config --cache true --cache-root "$cache" --parallel --format quiet --only "$cops" .)"
echo "rubocop --parallel, warm cache: $(secs rubocop --force-default-config --cache true --cache-root "$cache" --parallel --format quiet --only "$cops" .)"
echo "rbcop:                          $(secs "$bin" --only "$cops" .)"
rm -rf "$cache"

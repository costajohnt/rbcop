#!/bin/sh
# Time RuboCop and rbcop on a directory: default config, every cop rbcop implements (except
# Lint/RedundantCopDisableDirective, which --only rejects), wall time.
# usage: scripts/bench.sh <dir>
#   RuboCop comes from $RUBOCOP (default `rubocop`), e.g. RUBOCOP="rbenv exec rubocop" RBENV_VERSION=4.0.6.
# Fails if either tool errors (exit status above 1) instead of timing the failure.
set -eu
dir=$(cd "$1" && pwd)
root=$(cd "$(dirname "$0")/.." && pwd)
bin=$root/target/release/rbcop
rubocop=${RUBOCOP:-rubocop}
(cd "$root" && cargo build --release -q)
# Word splitting on $rubocop is intended ("rbenv exec rubocop").
# shellcheck disable=SC2086
version=$($rubocop --version 2>/dev/null) || { echo "bench: '$rubocop --version' failed; set RUBOCOP" >&2; exit 1; }
cops=$("$bin" --show-cops | sed -n 's/^\([A-Z][A-Za-z]*\/[A-Za-z0-9]*\):$/\1/p' \
  | grep -vx 'Lint/RedundantCopDisableDirective' | paste -sd, -)
[ -n "$cops" ] || { echo "bench: no cops from $bin --show-cops" >&2; exit 1; }
# Physical path: RuboCop rejects a cache root under a symlink (macOS /var) and --parallel then
# silently drops results (see scripts/oracle.sh).
cache=$(cd "$(mktemp -d)" && pwd -P)
log=$(mktemp)
trap 'rm -rf "$cache" "$log"' EXIT
cd "$dir"

now() { python3 -c 'import time; print(time.time())'; }
# run LABEL CMD...: time CMD, fail loudly unless it exits 0 (clean) or 1 (offenses).
run() {
  label=$1
  shift
  start=$(now)
  status=0
  "$@" > /dev/null 2> "$log" || status=$?
  end=$(now)
  if [ "$status" -gt 1 ]; then
    echo "bench: $label exited $status" >&2
    cat "$log" >&2
    exit 1
  fi
  printf '%-32s %ss\n' "$label:" "$(python3 -c "print(f'{$end - $start:.2f}')")"
}

files=$("$bin" --force-default-config --list-target-files . 2>/dev/null | wc -l | tr -d ' ')
[ "$files" -gt 0 ] || { echo "bench: no target files in $dir" >&2; exit 1; }
echo "$dir: $files files, $(echo "$cops" | tr , '\n' | wc -l | tr -d ' ') cops"
echo "rubocop $version"
echo "$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m), $(sysctl -n hw.ncpu 2>/dev/null || nproc) cores"
# shellcheck disable=SC2086
run "rubocop, one process" $rubocop --force-default-config --cache false --format quiet --only "$cops" .
# shellcheck disable=SC2086
run "rubocop --parallel, cold cache" $rubocop --force-default-config --cache true --cache-root "$cache" --parallel --format quiet --only "$cops" .
# shellcheck disable=SC2086
run "rubocop --parallel, warm cache" $rubocop --force-default-config --cache true --cache-root "$cache" --parallel --format quiet --only "$cops" .
run "rbcop" "$bin" --force-default-config --format quiet --only "$cops" .

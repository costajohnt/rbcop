#!/bin/sh
# Record RuboCop's output for every config scenario under tests/scenarios/<name>/ into expected.txt.
# Each scenario is a self-contained project (its own Gemfile bounds the config search).
# The recorded output covers ALL RuboCop cops; tests/scenarios.rs filters it to the cops rbcop implements.
# usage: scripts/scenarios.sh [name...]   (needs `rubocop` on PATH; with rbenv set RBENV_VERSION)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)/tests/scenarios
names=${*:-$(ls "$root")}
for name in $names; do
  dir="$root/$name"
  [ -d "$dir" ] || continue
  args=$(cat "$dir/args" 2>/dev/null || true)
  # emacs prints absolute paths; store them relative to the scenario root.
  # A scenario with a stderr.txt also records stderr there (e.g. the pending-cops notice), verbatim,
  # and with a status file the exit status there.
  err=/dev/null
  [ -f "$dir/stderr.txt" ] && err=$dir/stderr.txt
  out=$(mktemp)
  status=0
  (cd "$dir/project" && rubocop --cache false --format emacs $args 2>"$err") > "$out" || status=$?
  [ -f "$dir/status" ] && echo "$status" > "$dir/status"
  sed "s|^$dir/project/||" "$out" | LC_ALL=C sort > "$dir/expected.txt"
  # Absolute paths in messages (config file paths) are stored relative to the scenario root too.
  if [ -f "$dir/stderr.txt" ]; then
    sed "s|$dir/project/||g" "$dir/stderr.txt" > "$out"
    cat "$out" > "$dir/stderr.txt"
  fi
  rm -f "$out"
  echo "$name: $(wc -l < "$dir/expected.txt" | tr -d ' ') offenses recorded, exit $status"
done

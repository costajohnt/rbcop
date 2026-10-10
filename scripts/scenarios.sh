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
  # A scenario with a stderr.txt also records stderr there (e.g. the pending-cops notice), verbatim.
  err=/dev/null
  [ -f "$dir/stderr.txt" ] && err=$dir/stderr.txt
  (cd "$dir/project" && rubocop --cache false --format emacs $args 2>"$err") \
    | sed "s|^$dir/project/||" | LC_ALL=C sort > "$dir/expected.txt" || true
  echo "$name: $(wc -l < "$dir/expected.txt" | tr -d ' ') offenses recorded"
done

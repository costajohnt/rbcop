#!/bin/sh
# Record RuboCop's output for every config scenario under tests/scenarios/<name>/ into expected.txt.
# Each scenario is a self-contained project (its own Gemfile bounds the config search).
# The recorded output covers ALL RuboCop cops; tests/scenarios.rs filters it to the cops rbcop implements.
# usage: scripts/scenarios.sh [name...]   (RuboCop from $RUBOCOP, see scripts/rubocop.sh)
set -eu
. "$(dirname "$0")/rubocop.sh"
# rubocop-sorbet's TargetSorbetVersion cops find the lockfile like Bundler: the scenario's own.
unset BUNDLE_GEMFILE BUNDLE_LOCKFILE
root=$(cd "$(dirname "$0")/.." && pwd)/tests/scenarios
names=${*:-$(ls "$root")}
for name in $names; do
  dir="$root/$name"
  [ -d "$dir" ] || continue
  args=$(cat "$dir/args" 2>/dev/null || true)
  # emacs prints absolute paths (and some messages hold them); store them relative to the scenario root.
  # A scenario with a stderr.txt also records stderr there (e.g. the pending-cops notice).
  # The exit status always goes to status.
  err=/dev/null
  [ -f "$dir/stderr.txt" ] && err=$dir/stderr.txt
  out=$(mktemp)
  status=0
  # A scenario's own gem home (gems/, for a gem the pinned set lacks) goes on the load path: rbcop
  # finds the gem on GEM_PATH (tests/scenarios.rs), but RuboCop wrappers such as the CI host's
  # reset GEM_PATH, and its `require:` only needs the gem's lib.
  rubyopt=${RUBYOPT:-}
  for lib in "$dir"/gems/gems/*/lib; do
    [ -d "$lib" ] && rubyopt="-I$lib $rubyopt"
  done
  (cd "$dir/project" && RUBYOPT=$rubyopt rubocop --cache false --format emacs $args 2>"$err") > "$out" || status=$?
  echo "$status" > "$dir/status"
  sed "s|$dir/project/||g" "$out" | LC_ALL=C sort > "$dir/expected.txt"
  # Absolute paths in messages (config file paths) are stored relative to the scenario root too.
  # A failing `require` prints the host's Ruby backtrace (require_unloadable_after_plugin): keep
  # the message only, so a re-record on another host matches.
  if [ -f "$dir/stderr.txt" ]; then
    sed "s|$dir/project/||g" "$dir/stderr.txt" | { grep -vE "^/.*:[0-9]+:in '" || true; } > "$out"
    cat "$out" > "$dir/stderr.txt"
  fi
  rm -f "$out"
  echo "$name: $(wc -l < "$dir/expected.txt" | tr -d ' ') offenses recorded, exit $status"
done

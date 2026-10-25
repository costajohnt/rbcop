#!/bin/sh
# Record RuboCop on each tests/auto_gen/<case>: a copy of input/ (or of the input/ of the case named
# in input_from) is run with the arguments in args, and the resulting tree is stored in expected/,
# with stdout, stderr and status. tests/auto_gen.rs replays the cases with rbcop.
# usage: tests/auto_gen/record.sh [case...]   (RuboCop from $RUBOCOP, see scripts/rubocop.sh)
set -eu
. "$(dirname "$0")/../../scripts/rubocop.sh"
root=$(cd "$(dirname "$0")" && pwd)
names=${*:-$(ls "$root")}
for name in $names; do
  dir=$root/$name
  [ -f "$dir/args" ] || continue
  src=$dir/input
  [ -f "$dir/input_from" ] && src=$root/$(cat "$dir/input_from")/input
  work=$(mktemp -d)
  cp -R "$src/." "$work/"
  status=0
  # ExcludeLimit keeps its values under the cache root, which concurrent RuboCop runs share.
  cache=$(mktemp -d)
  # shellcheck disable=SC2046
  (cd "$work" && XDG_CACHE_HOME=$cache rubocop --cache false $(cat "$dir/args") > "$dir/stdout" 2> "$dir/stderr") || status=$?
  echo "$status" > "$dir/status"
  rm -rf "$dir/expected"
  cp -R "$work" "$dir/expected"
  rm -rf "$work" "$cache"
  echo "$name: exit $status"
done

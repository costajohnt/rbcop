#!/bin/sh
# Record RuboCop's `--show-cops` output for tests/show_cops.rs (cases must match CASES there).
# usage: scripts/show-cops.sh   (RuboCop from $RUBOCOP, see scripts/rubocop.sh)
set -eu
. "$(dirname "$0")/rubocop.sh"
root=$(cd "$(dirname "$0")/.." && pwd -P)
dir=$root/tests/show_cops
mkdir -p "$dir/expected"
record() {
  name=$1; shift
  (cd "$dir/project" && rubocop --cache false "$@") | sed "s|$dir/project|<project>|" > "$dir/expected/$name.txt"
  echo "$name: $(wc -l < "$dir/expected/$name.txt" | tr -d ' ') lines"
}
record all --show-cops
record patterns --show-cops 'Lint/*,Style/StringLiterals,Style/InverseMethods,Metrics/MethodLength,Naming/MethodName'
record preview --preview --show-cops 'Layout/*,Bundler/OrderedGems,Metrics/BlockLength'
record default --force-default-config --show-cops 'Layout/*,Lint/MissingCopEnableDirective,Lint/UriEscapeUnescape'

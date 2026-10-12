#!/bin/sh
# Re-record tests/autocorrect/<case>/expected{,.txt} with RuboCop: `rubocop -A` over a copy of input/.
# usage: tests/autocorrect/record.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
for case in "$here"/*/; do
  case=${case%/}
  # A case may list its own cops.
  cops=$(cat "$case/cops.txt" 2>/dev/null || cat "$here/cops.txt")
  rm -rf "$case/expected"
  cp -R "$case/input" "$case/expected"
  # `all`: no --only (Lint/RedundantCopDisableDirective cannot run under it); config.yml picks the cops.
  only="--only $cops"
  [ "$cops" = all ] && only=
  (cd "$case/expected" && RBENV_VERSION=4.0.6 rbenv exec rubocop -A -c ../config.yml $only --cache false \
    --format emacs . 2>/dev/null | sed "s|^$case/expected/||" > ../expected.txt) || true
done

#!/bin/sh
# Re-record tests/autocorrect/<case>/expected{,.txt} with RuboCop: `rubocop -A` over a copy of input/.
# usage: tests/autocorrect/record.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cops=$(cat "$here/cops.txt")
for case in "$here"/*/; do
  case=${case%/}
  rm -rf "$case/expected"
  cp -R "$case/input" "$case/expected"
  (cd "$case/expected" && RBENV_VERSION=4.0.6 rbenv exec rubocop -A -c ../config.yml --only "$cops" --cache false \
    --format emacs . 2>/dev/null | sed "s|^$case/expected/||" > ../expected.txt) || true
done

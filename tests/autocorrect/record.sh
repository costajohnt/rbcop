#!/bin/sh
# Re-record tests/autocorrect/<case>/expected{,.txt} with RuboCop: `rubocop -A` over a copy of input/.
# usage: tests/autocorrect/record.sh
# A case with a rubocop-output/ dir is a deliberate difference (docs/known-gaps.md): RuboCop's result
# is recorded there (and in rubocop-output.txt) and expected/ is left as rbcop's.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
for case in "$here"/*/; do
  case=${case%/}
  # A case may list its own cops.
  cops=$(cat "$case/cops.txt" 2>/dev/null || cat "$here/cops.txt")
  out=expected
  [ -d "$case/rubocop-output" ] && out=rubocop-output
  rm -rf "${case:?}/$out"
  cp -R "$case/input" "$case/$out"
  # `all`: no --only (Lint/RedundantCopDisableDirective cannot run under it); config.yml picks the cops.
  only="--only $cops"
  [ "$cops" = all ] && only=
  (cd "$case/$out" && RBENV_VERSION=4.0.6 rbenv exec rubocop -A -c ../config.yml $only --cache false \
    --format emacs . 2>/dev/null | sed "s|^$case/$out/||" > "../$out.txt") || true
  # `rubocop --diff -A` stdout (report, then the diffs) into diff.txt, on a scratch copy of input/:
  # RuboCop 1.91.0 can write a file under --diff (docs/known-gaps.md).
  # A case with a no-diff file has no diff.txt (its --diff output is a deliberate difference).
  [ "$out" = expected ] && [ ! -f "$case/no-diff" ] || continue
  rm -rf "$case/diff-run"
  cp -R "$case/input" "$case/diff-run"
  (cd "$case/diff-run" && RBENV_VERSION=4.0.6 rbenv exec rubocop --diff -A -c ../config.yml $only --cache false \
    --format emacs . 2>/dev/null | sed "s|^$case/diff-run/||" > ../diff.txt) || true
  rm -rf "${case:?}/diff-run"
done

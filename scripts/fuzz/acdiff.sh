#!/bin/sh
# Run `rubocop -A` and `rbcop -A` (default config) on copies of one file; diff reports and results.
# usage: scripts/fuzz/acdiff.sh <file>   (run with RBENV_VERSION=4.0.6)
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
w=$(mktemp -d)
mkdir "$w/a" "$w/b"
printf '3.4\n' > "$w/a/.ruby-version"
printf '3.4\n' > "$w/b/.ruby-version"
cp "$1" "$w/a/t.rb"
cp "$1" "$w/b/t.rb"
(cd "$w/a" && rubocop -A --force-default-config --cache false --format emacs t.rb 2>/dev/null | sed "s|^$w/a/||; s|^/private$w/a/||" | sort > ../a.out) || true
(cd "$w/b" && "$root/target/release/rbcop" -A --force-default-config --format emacs t.rb 2>/dev/null | sed "s|^$w/b/||; s|^/private$w/b/||" | sort > ../b.out) || true
if diff "$w/a.out" "$w/b.out" && cmp -s "$w/a/t.rb" "$w/b/t.rb"; then
  echo same
else
  diff "$w/a/t.rb" "$w/b/t.rb" | cat -vt || true
fi
rm -rf "$w"

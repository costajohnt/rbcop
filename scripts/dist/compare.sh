#!/bin/sh
# Check a packaged rbcop against a cargo build: run both on each corpus with one config and compare
# stdout (emacs format), stderr and exit status byte for byte.
# usage: scripts/dist/compare.sh REFERENCE CANDIDATE CONFIG CORPUS_DIR...
#   REFERENCE: e.g. target/release/rbcop; CANDIDATE: e.g. the gem's bin/rbcop (with GEM_HOME set)
#   CONFIG: absolute path, e.g. $PWD/config/all-cops.yml
# Prints one line per corpus: `<name>: identical (...)` or `<name>: DIFFERENT` and the first lines
# of the diff. Exit status 1 if any corpus differs.
set -u
ref=$1 cand=$2 cfg=$3
shift 3
out=$(mktemp -d)
status=0
for dir in "$@"; do
  n=$(basename "$dir")
  (cd "$dir" && "$ref" -c "$cfg" -f e > "$out/$n.ref" 2> "$out/$n.ref.err"; echo $? > "$out/$n.ref.code")
  (cd "$dir" && "$cand" -c "$cfg" -f e > "$out/$n.cand" 2> "$out/$n.cand.err"; echo $? > "$out/$n.cand.code")
  if cmp -s "$out/$n.ref" "$out/$n.cand" && cmp -s "$out/$n.ref.err" "$out/$n.cand.err" \
    && cmp -s "$out/$n.ref.code" "$out/$n.cand.code"; then
    echo "$n: identical ($(wc -l < "$out/$n.ref" | tr -d ' ') offense lines, exit $(cat "$out/$n.ref.code"))"
  else
    echo "$n: DIFFERENT"
    diff "$out/$n.ref" "$out/$n.cand" | head -5
    status=1
  fi
done
rm -rf "$out"
exit $status

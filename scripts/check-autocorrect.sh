#!/bin/sh
# Autocorrect gate: copy corpus files that have offenses for the given cops (per the oracles),
# run `rubocop -A` on one copy and `rbcop -A` on another, then diff the rewritten files and the
# reported offenses (with [Corrected] tags).
# usage: scripts/check-autocorrect.sh Cop1/Name[,Cop2/Name...] [corpus...]
#   env: RBCOP_CORPUS, RBCOP_ORACLE (as for check-cops.sh); MODE=-A (default) or -a;
#        LIMIT=n files per corpus (default all); OUT=scratch dir (default $TMPDIR/rbcop-autocorrect)
# Corpus files are only parsed, never executed. Exit 0 only if everything matches.
set -eu
cops=$1
shift
corpora=${*:-specs rails discourse}
root=$(cd "$(dirname "$0")/.." && pwd)
bin=$root/target/release/rbcop
corpus_root=${RBCOP_CORPUS:?set RBCOP_CORPUS to the corpus directory}
oracle_root=${RBCOP_ORACLE:?set RBCOP_ORACLE to the oracle directory}
mode=${MODE:--A}
out=${OUT:-${TMPDIR:-/tmp}/rbcop-autocorrect}
pattern=": [CWEFRI]: (\[Correctable\] )?($(echo "$cops" | tr , '|')): "

status=0
for c in $corpora; do
  dir=$out/$c
  rm -rf "$dir"
  mkdir -p "$dir/rubocop" "$dir/rbcop"
  # Files with at least one offense for these cops, relative to the corpus.
  grep -E "$pattern" "$oracle_root/$c.txt" | sed -E 's/:[0-9]+:[0-9]+: .*//' | sort -u \
    | sed "s|^$corpus_root/$c/||" | head -n "${LIMIT:-1000000}" > "$dir/files"
  # Keep the corpus's target Ruby (detected from these files at its root).
  for v in .ruby-version .tool-versions Gemfile.lock gems.locked; do
    if [ -f "$corpus_root/$c/$v" ]; then
      cp "$corpus_root/$c/$v" "$dir/rubocop/$v"
      cp "$corpus_root/$c/$v" "$dir/rbcop/$v"
    fi
  done
  (cd "$corpus_root/$c" && while IFS= read -r f; do
    for side in rubocop rbcop; do
      mkdir -p "$dir/$side/$(dirname "$f")"
      cp "$f" "$dir/$side/$f"
    done
  done) < "$dir/files"
  (cd "$dir/rubocop" && RBENV_VERSION=4.0.6 rbenv exec rubocop "$mode" --only "$cops" --force-default-config \
    --cache false --format emacs . > ../rubocop.out 2> ../rubocop.err) || true
  (cd "$dir/rbcop" && "$bin" "$mode" --only "$cops" --force-default-config --format emacs . > ../rbcop.out 2> ../rbcop.err) || true
  sed "s|^$dir/rubocop/||" "$dir/rubocop.out" | LC_ALL=C sort > "$dir/rubocop.norm"
  sed "s|^$dir/rbcop/||" "$dir/rbcop.out" | LC_ALL=C sort > "$dir/rbcop.norm"
  diff "$dir/rubocop.norm" "$dir/rbcop.norm" > "$dir/report.diff" || true
  diff -r "$dir/rubocop" "$dir/rbcop" > "$dir/files.diff" || true
  files=$(wc -l < "$dir/files" | tr -d ' ')
  rd=$(grep -c '^[<>]' "$dir/report.diff" || true)
  fd=$(grep -c '^diff ' "$dir/files.diff" || true)
  printf '%-10s files %5s  offenses %6s  report diff %4s  files differing %4s\n' \
    "$c" "$files" "$(wc -l < "$dir/rubocop.norm" | tr -d ' ')" "$rd" "$fd"
  if [ "$rd" != 0 ] || [ "$fd" != 0 ]; then status=1; fi
done
[ $status = 0 ] && echo "autocorrect matches" || echo "diffs in $out/<corpus>/{report,files}.diff (< rubocop, > rbcop)"
exit $status

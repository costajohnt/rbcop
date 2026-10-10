#!/bin/sh
# Gate for "cold" cops (too few offenses on the big corpora): RuboCop's own spec examples for them,
# one directory per cop and config variant (TargetRubyVersion, cop options), built by
# scripts/extract_cold_corpus.rb into $RBCOP_CORPUS/cold/<Dept_Cop>/<variant>/ (each with its .rubocop.yml).
# usage: scripts/check-cold.sh oracle     record $RBCOP_ORACLE/cold.txt (+ .crashes) with RuboCop, every cop on
#        scripts/check-cold.sh [Dept_Cop...]  compare rbcop offenses with the oracle and `-A` results with a live
#                                              `rubocop -A --only <cop>` run on copies of each variant (default: all)
# env: RBCOP_CORPUS, RBCOP_ORACLE; OUT=scratch dir (default $TMPDIR/rbcop-cold-<worktree name>).
# Corpus files are only parsed, never executed. Exit 0 only if everything matches.
set -eu
. "$(dirname "$0")/rubocop.sh"
root=$(cd "$(dirname "$0")/.." && pwd)
bin=$root/target/release/rbcop
corpus=${RBCOP_CORPUS:?set RBCOP_CORPUS to the corpus directory}/cold
oracle=${RBCOP_ORACLE:?set RBCOP_ORACLE to the oracle directory}/cold.txt
out=${OUT:-${TMPDIR:-/tmp}/rbcop-cold-$(basename "$root")}
variants() { find "$corpus${1:+/$1}" -name .rubocop.yml | sed 's|/\.rubocop\.yml$||' | sort; }
crashes() { sed -n 's/^An error occurred while \([^ ]*\) cop was inspecting \(.*\):[0-9]*:[0-9]*\.$/\1	\2/p' "$@"; }

if [ "${1:-}" = oracle ]; then
  : > "$oracle.tmp"
  : > "$oracle.err"
  for v in $(variants); do
    cache="$oracle.cache"
    rm -rf "$cache"
    (cd "$v" && rubocop --config .rubocop.yml --cache true --cache-root "$cache" --parallel \
      -r "$root/scripts/oracle_formatter.rb" -f RuboCop::Formatter::RbcopOracleFormatter . >> "$oracle.tmp" 2>> "$oracle.err") || true
    rm -rf "$cache"
  done
  LC_ALL=C sort "$oracle.tmp" > "$oracle"
  rm -f "$oracle.tmp"
  crashes "$oracle.err" | sort -u > "$oracle.crashes"
  echo "$(wc -l < "$oracle" | tr -d ' ') offenses, $(wc -l < "$oracle.crashes" | tr -d ' ') crashes recorded in $oracle"
  exit 0
fi

(cd "$root" && cargo build --release -q)
cops=${*:-$(cd "$corpus" && ls)}
list=$(echo $cops | tr ' ' '\n' | sed 's|_|/|' | paste -sd, -)
pattern=": [CWEFRI]: (\[Correctable\] )?($(echo "$list" | tr , '|')): "
rm -rf "$out"
mkdir -p "$out"
# Files hitting a documented gap (docs/known-gaps.md), relative to the cold corpus.
grep -v '^#' "$root/scripts/cold-skip.txt" > "$out/skip.rel" || true
sed "s|^|$corpus/|" "$out/skip.rel" > "$out/skip"

# Offenses: every cop on (like the oracle), compared for the cold cops only.
: > "$out/rbcop.raw"
for v in $(variants); do
  (cd "$v" && "$bin" --config .rubocop.yml --format rbcop-oracle . >> "$out/rbcop.raw" 2>> "$out/rbcop.err") || true
done
awk -F'\t' 'FILENAME == ARGV[1] { skip[$1 "\t" $2] = 1; next }
    { path = $0; sub(/:[0-9]+:[0-9]+:[0-9]+:[0-9]+: .*/, "", path)
      cop = $0; sub(/^.*: [CWEFRI]: (\[Correctable\] )?/, "", cop); sub(/: .*/, "", cop)
      if (!((cop "\t" path) in skip)) print }' "$oracle.crashes" "$out/rbcop.raw" \
  | grep -E "$pattern" | grep -vF -f "$out/skip" | LC_ALL=C sort > "$out/rbcop" || true
grep -E "$pattern" "$oracle" | grep -vF -f "$out/skip" | LC_ALL=C sort > "$out/rubocop" || true
LC_ALL=C comm -23 "$out/rubocop" "$out/rbcop" | sed 's/^/< /' > "$out/offenses.diff"
LC_ALL=C comm -13 "$out/rubocop" "$out/rbcop" | sed 's/^/> /' >> "$out/offenses.diff"
status=0
n=$(wc -l < "$out/offenses.diff" | tr -d ' ')
printf 'offenses   rubocop %6s  rbcop %6s  diff %5s\n' "$(wc -l < "$out/rubocop" | tr -d ' ')" "$(wc -l < "$out/rbcop" | tr -d ' ')" "$n"
[ "$n" = 0 ] || status=1

# Autocorrect: each variant with only its own cop, both tools on copies.
for c in $cops; do
  cop=$(echo "$c" | sed 's|_|/|')
  for v in $(variants "$c"); do
    rel=${v#"$corpus"/}
    for side in rubocop rbcop; do
      mkdir -p "$out/ac/$side/$rel"
      cp -R "$v/" "$out/ac/$side/$rel/"
      (cd "$out/ac/$side" && xargs rm -f < "$out/skip.rel")
    done
    (cd "$out/ac/rubocop/$rel" && rubocop -A --only "$cop" --config .rubocop.yml --cache false --format emacs . \
      2>/dev/null | sed "s|$(pwd -P)/||g" | LC_ALL=C sort > "$out/ac/rubocop.$(echo "$rel" | tr / _).out") || true
    (cd "$out/ac/rbcop/$rel" && "$bin" -A --only "$cop" --config .rubocop.yml --format emacs . \
      2>/dev/null | sed "s|$(pwd -P)/||g" | LC_ALL=C sort > "$out/ac/rbcop.$(echo "$rel" | tr / _).out") || true
  done
done
(cd "$out/ac" && for f in rubocop.*.out; do diff "$f" "rbcop.${f#rubocop.}" | sed "s|^|$f: |"; done) > "$out/ac-report.diff" || true
diff -r "$out/ac/rubocop" "$out/ac/rbcop" > "$out/ac-files.diff" || true
rd=$(grep -c ': [<>]' "$out/ac-report.diff" || true)
fd=$(grep -c '^diff ' "$out/ac-files.diff" || true)
printf 'autocorrect report diff %4s  files differing %4s\n' "$rd" "$fd"
if [ "$rd" != 0 ] || [ "$fd" != 0 ]; then status=1; fi
[ $status = 0 ] && echo "cold corpus matches" || echo "diffs in $out/{offenses,ac-report,ac-files}.diff (< rubocop, > rbcop)"
exit $status

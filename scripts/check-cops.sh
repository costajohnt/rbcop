#!/bin/sh
# Gate for porting cops: compare rbcop with recorded RuboCop oracles (scripts/oracle.sh) for the given cops.
# usage: scripts/check-cops.sh Cop1/Name[,Cop2/Name...] [corpus...]
#   corpora default to specs rails discourse rubocop-src jekyll chef fastlane brew mastodon; each needs $RBCOP_CORPUS/<name> and $RBCOP_ORACLE/<name>.txt
#   env CONFIG: the config both the oracle and rbcop ran with (default config/all-cops.yml)
# Exit 0 only if every corpus matches exactly. Diffs: $TMPDIR/rbcop-check/<pid>/<corpus>.diff (kept and printed on failure)
set -eu
cops=$1
shift
corpora=${*:-specs rails discourse rubocop-src jekyll chef fastlane brew mastodon}
root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/rbcop.sh"
corpus_root=${RBCOP_CORPUS:?set RBCOP_CORPUS to the corpus directory}
oracle_root=${RBCOP_ORACLE:?set RBCOP_ORACLE to the oracle directory}
# Per run, so concurrent gates (parallel porting agents) never share diff files.
out=${TMPDIR:-/tmp}/rbcop-check/$$
mkdir -p "$out"
all=
if [ "$cops" = all ]; then
  all=1
  cops=$(rbcop_cops)
fi
[ -n "$cops" ] || { echo "check-cops: empty cop list" >&2; exit 1; }
pattern=": [CWEFRI]: (\[Correctable\] )?($(echo "$cops" | tr , '|')): "
# Lint/RedundantCopDisableDirective needs every cop's offenses and is rejected by --only (as in
# RuboCop), so such runs inspect with all cops (like the oracles) and rely on the pattern filter.
only="--only $cops"
case ",$cops," in *,Lint/RedundantCopDisableDirective,*) only= ;; esac
norm() { grep -E "$pattern" | LC_ALL=C sort; }
# Files whose differences are documented gaps (scripts/check-skip.txt) are left out on both sides.
unskipped() { if [ -s "$1" ]; then grep -vF -f "$1"; else cat; fi; }

status=0
for c in $corpora; do
  # Where a RuboCop cop crashed it reported nothing, so drop rbcop's offenses for that (cop, file).
  crashes="$oracle_root/$c.txt.crashes"
  [ -f "$crashes" ] || crashes=/dev/null
  [ -s "$oracle_root/$c.txt" ] || { echo "check-cops: oracle $oracle_root/$c.txt is missing or empty" >&2; exit 1; }
  sed -n "s|^$c/\(.*\)|$corpus_root/$c/\1:|p" "$root/scripts/check-skip.txt" > "$out/$c.skip"
  # rbcop-oracle = emacs plus range end (path:line:col:last_line:last_col), like scripts/oracle_formatter.rb.
  (cd "$corpus_root/$c" && "$bin" --config "${CONFIG:-$root/config/all-cops.yml}" --format rbcop-oracle $only . 2>"$out/$c.err") \
    | awk -F'\t' 'FILENAME == ARGV[1] { skip[$1 "\t" $2] = 1; next }
        { path = $0; sub(/:[0-9]+:[0-9]+:[0-9]+:[0-9]+: .*/, "", path)
          cop = $0; sub(/^.*: [CWEFRI]: (\[Correctable\] )?/, "", cop); sub(/: .*/, "", cop)
          if (!((cop "\t" path) in skip)) print }' "$crashes" - \
    | unskipped "$out/$c.skip" | norm > "$out/$c.rbcop" || true
  unskipped "$out/$c.skip" < "$oracle_root/$c.txt" | norm > "$out/$c.rubocop" || true
  LC_ALL=C comm -23 "$out/$c.rubocop" "$out/$c.rbcop" | sed 's/^/< /' > "$out/$c.diff"
  LC_ALL=C comm -13 "$out/$c.rubocop" "$out/$c.rbcop" | sed 's/^/> /' >> "$out/$c.diff"
  n=$(wc -l < "$out/$c.diff" | tr -d ' ')
  n_rubocop=$(wc -l < "$out/$c.rubocop" | tr -d ' ')
  printf '%-10s rubocop %6s  rbcop %6s  diff %5s\n' "$c" "$n_rubocop" "$(wc -l < "$out/$c.rbcop" | tr -d ' ')" "$n"
  # With every cop on, a non-empty oracle filtered to nothing means the pattern is broken, not a match.
  if [ -n "$all" ] && [ "$n_rubocop" = 0 ]; then
    echo "check-cops: no oracle offenses for $c match the cop list" >&2
    exit 1
  fi
  if [ "$n" != 0 ]; then
    status=1
    sed -E 's/^([<>]).*: [CWEFRI]: (\[Correctable\] )?([A-Za-z]+\/[A-Za-z0-9]+):.*/   \1 \3/' "$out/$c.diff" | sort | uniq -c | sort -rn | head -20
  fi
done
if [ $status = 0 ]; then
  # Only failing runs keep their files: each run's dir is up to ~1 GB with every cop on.
  rm -rf "$out"
  echo "all corpora match"
else
  echo "diffs in $out/<corpus>.diff (< only RuboCop, > only rbcop)"
fi
exit $status

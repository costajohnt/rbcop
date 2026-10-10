#!/bin/sh
# Gate for porting cops: compare rbcop with recorded RuboCop oracles (scripts/oracle.sh) for the given cops.
# usage: scripts/check-cops.sh Cop1/Name[,Cop2/Name...] [corpus...]
#   corpora default to specs rails discourse rubocop-src jekyll chef fastlane brew mastodon; each needs $RBCOP_CORPUS/<name> and $RBCOP_ORACLE/<name>.txt
# Exit 0 only if every corpus matches exactly. Diffs: $TMPDIR/rbcop-check/<pid>/<corpus>.diff (kept and printed on failure)
set -eu
cops=$1
shift
corpora=${*:-specs rails discourse rubocop-src jekyll chef fastlane brew mastodon}
root=$(cd "$(dirname "$0")/.." && pwd)
bin=$root/target/release/rbcop
# A stale binary reports phantom diffs (e.g. right after a merge).
(cd "$root" && cargo build --release -q)
corpus_root=${RBCOP_CORPUS:?set RBCOP_CORPUS to the corpus directory}
oracle_root=${RBCOP_ORACLE:?set RBCOP_ORACLE to the oracle directory}
# Per run, so concurrent gates (parallel porting agents) never share diff files.
out=${TMPDIR:-/tmp}/rbcop-check/$$
mkdir -p "$out"
[ "$cops" = all ] && cops=$("$bin" --show-cops | sed -n 's/^\([^ #].*\):$/\1/p' | paste -sd, -)
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
  sed -n "s|^$c/\(.*\)|$corpus_root/$c/\1:|p" "$root/scripts/check-skip.txt" > "$out/$c.skip"
  # rbcop-oracle = emacs plus range end (path:line:col:last_line:last_col), like scripts/oracle_formatter.rb.
  (cd "$corpus_root/$c" && "$bin" --config "$root/config/all-cops.yml" --format rbcop-oracle $only . 2>"$out/$c.err") \
    | awk -F'\t' 'FILENAME == ARGV[1] { skip[$1 "\t" $2] = 1; next }
        { path = $0; sub(/:[0-9]+:[0-9]+:[0-9]+:[0-9]+: .*/, "", path)
          cop = $0; sub(/^.*: [CWEFRI]: (\[Correctable\] )?/, "", cop); sub(/: .*/, "", cop)
          if (!((cop "\t" path) in skip)) print }' "$crashes" - \
    | unskipped "$out/$c.skip" | norm > "$out/$c.rbcop" || true
  unskipped "$out/$c.skip" < "$oracle_root/$c.txt" | norm > "$out/$c.rubocop" || true
  LC_ALL=C comm -23 "$out/$c.rubocop" "$out/$c.rbcop" | sed 's/^/< /' > "$out/$c.diff"
  LC_ALL=C comm -13 "$out/$c.rubocop" "$out/$c.rbcop" | sed 's/^/> /' >> "$out/$c.diff"
  n=$(wc -l < "$out/$c.diff" | tr -d ' ')
  printf '%-10s rubocop %6s  rbcop %6s  diff %5s\n' "$c" "$(wc -l < "$out/$c.rubocop" | tr -d ' ')" "$(wc -l < "$out/$c.rbcop" | tr -d ' ')" "$n"
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

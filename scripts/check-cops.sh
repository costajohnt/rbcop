#!/bin/sh
# Gate for porting cops: compare rbcop with recorded RuboCop oracles (scripts/oracle.sh) for the given cops.
# usage: scripts/check-cops.sh Cop1/Name[,Cop2/Name...] [corpus...]
#   corpora default to specs rails discourse; each needs $RBCOP_CORPUS/<name> and $RBCOP_ORACLE/<name>.txt
# Exit 0 only if every corpus matches exactly. Diffs: $TMPDIR/rbcop-check/<corpus>.diff
set -eu
cops=$1
shift
corpora=${*:-specs rails discourse}
root=$(cd "$(dirname "$0")/.." && pwd)
bin=$root/target/release/rbcop
corpus_root=${RBCOP_CORPUS:?set RBCOP_CORPUS to the corpus directory}
oracle_root=${RBCOP_ORACLE:?set RBCOP_ORACLE to the oracle directory}
out=${TMPDIR:-/tmp}/rbcop-check
mkdir -p "$out"
pattern=": [CWEFRI]: (\[Correctable\] )?($(echo "$cops" | tr , '|')): "
# LineLength's [Correctable] tag depends on CheckLineBreakable, which rbcop does not model yet.
norm() { grep -E "$pattern" | sed -E 's/\[Correctable\] (Layout\/LineLength)/\1/' | LC_ALL=C sort; }

status=0
for c in $corpora; do
  (cd "$corpus_root/$c" && "$bin" -c "$root/config/all-cops.yml" --format emacs --only "$cops" . 2>"$out/$c.err") | norm > "$out/$c.rbcop" || true
  norm < "$oracle_root/$c.txt" > "$out/$c.rubocop" || true
  LC_ALL=C comm -23 "$out/$c.rubocop" "$out/$c.rbcop" | sed 's/^/< /' > "$out/$c.diff"
  LC_ALL=C comm -13 "$out/$c.rubocop" "$out/$c.rbcop" | sed 's/^/> /' >> "$out/$c.diff"
  n=$(wc -l < "$out/$c.diff" | tr -d ' ')
  printf '%-10s rubocop %6s  rbcop %6s  diff %5s\n' "$c" "$(wc -l < "$out/$c.rubocop" | tr -d ' ')" "$(wc -l < "$out/$c.rbcop" | tr -d ' ')" "$n"
  if [ "$n" != 0 ]; then
    status=1
    sed -E 's/^([<>]).*: [CWEFRI]: (\[Correctable\] )?([A-Za-z]+\/[A-Za-z0-9]+):.*/   \1 \3/' "$out/$c.diff" | sort | uniq -c | sort -rn | head -20
  fi
done
[ $status = 0 ] && echo "all corpora match" || echo "diffs in $out/<corpus>.diff (< only RuboCop, > only rbcop)"
exit $status

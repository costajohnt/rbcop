#!/bin/bash
# CI-shaped benchmark (docs/benchmarks.md): RuboCop with a cold cache vs rbcop, pinned to the CPUs
# of an emulated GitHub-hosted runner. Linux only (taskset).
# usage: scripts/bench-ci.sh <out.tsv> <reps> <tools> <dir>...
#   tools: comma-separated from rubocop-single, rubocop-par, rbcop
#   CPUS_2 / CPUS_4: CPU lists for the 2- and 4-vCPU runs (default 1,7 and 1,7,2,8: one and two
#   physical cores with both hyperthreads, like Azure's vCPUs); CPUS_1 for single-process RuboCop.
#   RUBOCOP: the RuboCop command (default `rubocop`).
# Rows appended to out.tsv: rep, corpus, vcpus, tool, wall s, user s, sys s, exit, load1, contaminated.
# A run is redone (up to 3 tries) when a gate (ci-run.sh) ran meanwhile; other benchmarks on the
# box serialize on ~/scratch/BENCH.lock.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/rbcop.sh"
out=$1 reps=$2 tools=$3
shift 3
read -ra rubocop <<< "${RUBOCOP:-rubocop}"
declare -A cpuset=([1]=${CPUS_1:-3} [2]=${CPUS_2:-1,7} [4]=${CPUS_4:-1,7,2,8})
lock=${BENCH_LOCK:-$HOME/scratch/BENCH.lock}
mkdir -p "$(dirname "$lock")"
quiet() { while pgrep -f ci-run.sh >/dev/null; do sleep 20; done; }
# time_one DIR VCPUS CMD...: wall/user/sys of CMD pinned to VCPUS.
time_one() {
  local dir=$1 cpus=$2
  shift 2
  for _ in 1 2 3; do
    exec 9>"$lock"
    flock 9
    quiet
    local flag
    flag=$(mktemp -u)
    (while :; do pgrep -f ci-run.sh >/dev/null && touch "$flag"; sleep 5; done) &
    local mon=$! load t0 t1 times st cont=0
    load=$(cut -d' ' -f1 /proc/loadavg)
    t0=$(date +%s.%N)
    times=$({ cd "$dir" && TIMEFORMAT='%U %S' && time taskset -c "${cpuset[$cpus]}" "$@" >/dev/null 2>&1; } 2>&1)
    st=$?
    t1=$(date +%s.%N)
    kill $mon 2>/dev/null
    wait $mon 2>/dev/null
    [ -e "$flag" ] && cont=1
    rm -f "$flag"
    exec 9>&-
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$rep" "$(basename "$dir")" "$cpus" "$tool" \
      "$(python3 -c "print(f'{$t1 - $t0:.2f}')")" "${times// /$'\t'}" "$st" "$load" "$cont" >> "$out"
    [ "$cont" = 0 ] && return
  done
}
for rep in $(seq 1 "$reps"); do
  for dir in "$@"; do
    for tool in ${tools//,/ }; do
      case $tool in
        rubocop-single) time_one "$dir" 1 "${rubocop[@]}" --force-default-config --cache false --format quiet . ;;
        rubocop-par)
          for n in 2 4; do
            cache=$(mktemp -d)
            time_one "$dir" $n "${rubocop[@]}" --force-default-config --cache true --cache-root "$cache" --parallel --format quiet .
            rm -rf "$cache"
          done ;;
        rbcop) for n in 2 4; do time_one "$dir" $n "$bin" --force-default-config --format quiet .; done ;;
      esac
    done
  done
done

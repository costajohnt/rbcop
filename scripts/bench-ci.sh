#!/bin/bash
# CI-shaped benchmark (docs/benchmarks.md): RuboCop with a cold cache vs rbcop, pinned to the CPUs
# of an emulated GitHub-hosted runner. Linux only (taskset).
# usage: scripts/bench-ci.sh <out.tsv> <reps> <tools> <dir>...
#   tools: comma-separated from rubocop-single, rubocop-par, rbcop, rbcop-cache, boot, file
#     rubocop-single: RuboCop without --parallel, 2 vCPUs; rubocop-par, rbcop: 2 and 4 vCPUs;
#     rubocop-par is a cold-cache run then a warm one (rubocop-par-warm); rbcop runs with
#     --cache false; rbcop-cache is rbcop with a cold cache (rbcop-cold) then warm (rbcop-warm);
#     boot: `rubocop --version`, `bundle exec rubocop --version` (Gemfile in <dir>), `rbcop --version`;
#     file: RuboCop and rbcop on the one file $FILE (relative to <dir>), 2 vCPUs.
#   PROJECT_CONFIG=1: use each directory's own .rubocop.yml (plugins must be installed for
#     RuboCop); default --force-default-config.
#   CPUS_2 / CPUS_4: CPU lists for the 2- and 4-vCPU runs (default 1,7 and 1,7,2,8: one and two
#   physical cores with both hyperthreads, like Azure's vCPUs).
#   RUBOCOP: the RuboCop command (default `rubocop`).
# Rows appended to out.tsv: rep, corpus, vcpus, tool, wall s, user s, sys s, exit, load1, contaminated,
# start (epoch s, to line runs up with host-side CPU logs).
# A run is redone (up to 3 tries) when a gate (ci-run.sh) ran meanwhile; other benchmarks on the
# box serialize on ~/scratch/BENCH.lock.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/rbcop.sh"
out=$1 reps=$2 tools=$3
shift 3
read -ra rubocop <<< "${RUBOCOP:-rubocop}"
declare -A cpuset=([2]=${CPUS_2:-1,7} [4]=${CPUS_4:-1,7,2,8})
cfg=--force-default-config
[ "${PROJECT_CONFIG:-0}" = 1 ] && cfg=
lock=${BENCH_LOCK:-$HOME/scratch/BENCH.lock}
mkdir -p "$(dirname "$lock")"
quiet() { while pgrep -f ci-run.sh >/dev/null; do sleep 20; done; }
# time_one DIR VCPUS LABEL CMD...: wall/user/sys of CMD pinned to VCPUS, recorded as LABEL.
time_one() {
  local dir=$1 cpus=$2 label=$3
  shift 3
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
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$rep" "$(basename "$dir")" "$cpus" "$label" \
      "$(python3 -c "print(f'{$t1 - $t0:.2f}')")" "${times// /$'\t'}" "$st" "$load" "$cont" "${t0%.*}" >> "$out"
    [ "$cont" = 0 ] && return
  done
}
for rep in $(seq 1 "$reps"); do
  for dir in "$@"; do
    for tool in ${tools//,/ }; do
      case $tool in
        rubocop-single) time_one "$dir" 2 "$tool" "${rubocop[@]}" $cfg --cache false --format quiet . ;;
        rubocop-par)
          for n in 2 4; do
            cache=$(mktemp -d)
            time_one "$dir" $n "$tool" "${rubocop[@]}" $cfg --cache true --cache-root "$cache" --parallel --format quiet .
            time_one "$dir" $n rubocop-par-warm "${rubocop[@]}" $cfg --cache true --cache-root "$cache" --parallel --format quiet .
            rm -rf "$cache"
          done ;;
        rbcop) for n in 2 4; do time_one "$dir" $n "$tool" "$bin" $cfg --cache false --format quiet .; done ;;
        rbcop-cache)
          for n in 2 4; do
            cache=$(mktemp -d)
            time_one "$dir" $n rbcop-cold "$bin" $cfg --cache-root "$cache" --format quiet .
            time_one "$dir" $n rbcop-warm "$bin" $cfg --cache-root "$cache" --format quiet .
            rm -rf "$cache"
          done ;;
        boot)
          time_one "$dir" 2 rubocop-version "${rubocop[@]}" --version
          time_one "$dir" 2 bundle-rubocop-version bundle exec rubocop --version
          time_one "$dir" 2 rbcop-version "$bin" --version ;;
        file)
          cache=$(mktemp -d)
          time_one "$dir" 2 rubocop-file "${rubocop[@]}" $cfg --cache true --cache-root "$cache" --format quiet "$FILE"
          rm -rf "$cache"
          time_one "$dir" 2 rbcop-file "$bin" $cfg --cache false --format quiet "$FILE" ;;
      esac
    done
  done
done

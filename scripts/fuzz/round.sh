#!/bin/sh
# One differential-fuzz round: <batches> batches of <size> variants, each mutated and compared.
# usage: RBCOP_CORPUS=... scripts/fuzz/round.sh <out-base-dir> <name> <rng-seed> [batches] [size]
# Batch i goes to <out-base-dir>/<name>-<i> (rng seed <rng-seed>+i); summaries to <out-base-dir>/<name>.log.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
base=$1
name=$2
seed=$3
batches=${4:-4}
size=${5:-500}
export RBENV_VERSION=4.0.6
log=$base/$name.log
: > "$log"
i=1
while [ "$i" -le "$batches" ]; do
  dir=$base/$name-$i
  rm -rf "$dir" "$dir.cache"
  rbenv exec ruby "$here/mutate.rb" "$dir" "$size" $((seed + i)) >> "$log"
  rbenv exec ruby "$here/run.rb" "$dir" >> "$log" 2>&1
  i=$((i + 1))
done
echo "round $name done" >> "$log"

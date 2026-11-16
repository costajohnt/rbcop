#!/usr/bin/env bash
# Scaling check (#789): rbcop's time on one large file must grow roughly linearly with its size.
#
# usage: scripts/check-scaling.sh [rbcop-binary]   (default target/release/rbcop)
#
# Generates files of 4,000 and 16,000 lines for several shapes (top-level calls with omitted hash
# values under omit_parentheses, documented empty defs and calls, File.open calls, string
# interpolations, a long hash, a class with many methods), runs every enabled cop on each
# (--cache false, best user+sys CPU time of RUNS runs, default 3) and fails when a shape's 16k
# time is more than MAX_RATIO (default 8; linear is 4) times its 4k time. A quadratic path shows
# up as about 16.
# Style/SoleNestedConditional is quadratic in RuboCop too (it reparses the whole file per offense),
# so deep nesting is not one of the shapes. Timing is noisy on a loaded host: run it on an idle one.
set -euo pipefail

bin=$(cd "$(dirname "${1:-target/release/rbcop}")" && pwd)/$(basename "${1:-target/release/rbcop}")
runs=${RUNS:-3}
max_ratio=${MAX_RATIO:-8}
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
export LC_ALL=C.UTF-8 LANG=C.UTF-8

printf 'AllCops:\n  TargetRubyVersion: 3.4\n  NewCops: enable\nStyle/MethodCallWithArgsParentheses:\n  Enabled: true\n  EnforcedStyle: omit_parentheses\n' >"$dir/.rubocop.yml"

gen() { # gen <shape> <lines>
  awk -v shape="$1" -v n="$2" 'BEGIN {
    print "# frozen_string_literal: true"; print ""
    if (shape == "calls") { print "a = 1"; for (i = 1; i < n; i++) print (i % 2 ? "foo(b: " i ", a:)" : "bar(x" i % 7 ", \"y\", z: 1)") }
    if (shape == "comments") { for (i = 0; i < n / 5; i++) { print "# doc " i; print "def m" i; print "end"; print "# note " i; print "foo(b: " i ", a:)" } }
    if (shape == "interp") { for (i = 0; i < n; i++) print "puts \"a #{b} c #{d" i % 5 "}\"" }
    if (shape == "fileopen") { for (i = 0; i < n; i++) print "File.open(\"f" i "\")" }
    if (shape == "hash") { print "H = {"; for (i = 0; i < n - 2; i++) print "  k" i ": \"v" i "\","; print "}" }
    if (shape == "methods") { print "class Foo"; for (i = 0; i < n / 5; i++) { print "  def m" i "(a, b = 1)"; print "    return a if b.nil?"; print "    a + (b * " i ")"; print "  end"; print "" } print "end" }
  }'
}

cpu() { # best user+sys seconds of $runs runs on $1
  local best='' t
  for _ in $(seq "$runs"); do
    t=$( { TIMEFORMAT='%U %S'; time "$bin" --cache false --format quiet "$1" >/dev/null 2>&1 || true; } 2>&1 | awk '{ print $1 + $2 }')
    best=$(awk -v a="$t" -v b="$best" 'BEGIN { print (b == "" || a < b) ? a : b }')
  done
  echo "$best"
}

status=0
printf '%-10s %8s %8s %6s\n' shape 4k-cpu 16k-cpu ratio
for shape in calls comments fileopen interp hash methods; do
  mkdir -p "$dir/$shape"
  gen "$shape" 4000 >"$dir/$shape/small.rb"
  gen "$shape" 16000 >"$dir/$shape/large.rb"
  small=$(cpu "$dir/$shape/small.rb")
  large=$(cpu "$dir/$shape/large.rb")
  ratio=$(awk -v s="$small" -v l="$large" 'BEGIN { printf "%.1f", l / (s > 0.01 ? s : 0.01) }')
  verdict=ok
  if awk -v r="$ratio" -v m="$max_ratio" 'BEGIN { exit !(r > m) }'; then verdict=SUPERLINEAR status=1; fi
  printf '%-10s %8s %8s %6s %s\n' "$shape" "$small" "$large" "$ratio" "$verdict"
done
[ "$status" = 0 ] && echo "scaling: ok" || echo "scaling: FAILED (16k/4k cpu ratio above $max_ratio)"
exit "$status"

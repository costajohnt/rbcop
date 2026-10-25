#!/bin/sh
# Gate for the result cache (#281): on a copy of each project, a cold run, a warm run, and a warm
# run after editing a few files and the top .rubocop.yml must all print what `--cache false`
# prints (stdout, stderr, exit status) in every formatter; then -a and -A on a warm cache must
# report and rewrite exactly as with `--cache false`.
# usage: scripts/check-cache.sh PROJECT_DIR...   (the directories are copied, never changed)
# env: OUT=scratch dir (default $TMPDIR/rbcop-cache-<worktree name>). Exit 0 only if all match.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/rbcop.sh"
out=${OUT:-${TMPDIR:-/tmp}/rbcop-cache-$(basename "$root")}
formats="progress clang emacs simple quiet files json junit html offenses worst tap pacman github markdown"
status=0

# run DIR NAME ARGS...: NAME.out, NAME.err and NAME.status in $out.
run() {
  run_dir=$1 run_name=$2
  shift 2
  (cd "$run_dir" && "$bin" "$@" > "$out/$run_name.out" 2> "$out/$run_name.err"; echo $? > "$out/$run_name.status") || true
}
same() {
  for x in out err status; do
    if ! cmp -s "$out/$1.$x" "$out/$2.$x"; then
      echo "DIFF $3: $1.$x vs $2.$x"
      diff "$out/$1.$x" "$out/$2.$x" | head -5
      status=1
      return
    fi
  done
}
# Every formatter, against a --cache false run: $1 = project copy, $2 = label.
formats() {
  for f in $formats; do
    run "$1" base --cache false -f "$f"
    run "$1" warm --cache-root "$out/cache" -f "$f"
    same base warm "$2 -f $f"
  done
}

for p in "$@"; do
  name=$(basename "$p")
  rm -rf "$out"
  mkdir -p "$out"
  cp -R "$p" "$out/a"
  # Cold, then warm (twice: the first formatter fills the cache).
  run "$out/a" base --cache false
  run "$out/a" cold --cache-root "$out/cache"
  same base cold "$name cold"
  formats "$out/a" "$name warm"
  # A few files and the top config change.
  for f in $(cd "$out/a" && find . -name '*.rb' | LC_ALL=C sort | awk 'NR % 97 == 1' | head -5); do
    printf '\nputs(1)  \n' >> "$out/a/$f"
  done
  if [ -f "$out/a/.rubocop.yml" ]; then
    for cop in Style/MethodCalledOnDoEndBlock Style/ImplicitRuntimeError Lint/NumberConversion; do
      if ! grep -q "^$cop:" "$out/a/.rubocop.yml"; then
        printf '\n%s:\n  Enabled: true\n' "$cop" >> "$out/a/.rubocop.yml"
        break
      fi
    done
  fi
  formats "$out/a" "$name edited"
  # -a / -A on a warm cache rewrite as without one (both in the same directory, $out/w).
  for mode in -a -A; do
    rm -rf "$out/b" "$out/w"
    cp -R "$out/a" "$out/w"
    run "$out/w" fixed-base --cache false "$mode" -f emacs
    mv "$out/w" "$out/b"
    cp -R "$out/a" "$out/w"
    run "$out/w" fixed-warm --cache-root "$out/cache" "$mode" -f emacs
    same fixed-base fixed-warm "$name $mode"
    diff -r --no-dereference "$out/b" "$out/w" > /dev/null || { echo "DIFF $name $mode: rewritten files differ"; status=1; }
    # And the cache those runs left behind still reports the corrected tree right.
    run "$out/w" after-base --cache false
    run "$out/w" after-warm --cache-root "$out/cache"
    same after-base after-warm "$name after $mode"
  done
  echo "$name: $(find "$out/cache" -type f | wc -l | tr -d ' ') cache entries checked"
done
[ "$status" = 0 ] && echo "cache: all match"
exit "$status"

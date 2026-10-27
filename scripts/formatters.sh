#!/bin/sh
# Record RuboCop's output for every case in tests/formatters/cases.txt into tests/formatters/expected/
# (<name>.txt is stdout, <name>.status the exit status); tests/formatters.rs replays them with rbcop.
# A case line is `name|directory (from the repo root)|arguments[|stdin file (from the repo root)]`.
# RuboCop runs with --cache false --force-default-config --only <tests/formatters/cops.txt>, so the
# goldens do not move when rbcop gains cops; cfg-* cases read their directory's config (no
# --force-default-config). Machine-specific text is normalised (see normalise()).
# usage: scripts/formatters.sh [name...]   (RuboCop from $RUBOCOP, see scripts/rubocop.sh)
set -eu
. "$(dirname "$0")/rubocop.sh"
root=$(cd "$(dirname "$0")/.." && pwd -P)
dir=$root/tests/formatters
cops=$(paste -sd, "$dir/cops.txt")
mkdir -p "$dir/expected"
# Keep in sync with normalise() in tests/formatters.rs.
normalise() {
  sed -e "s|$root|<root>|g" \
      -e 's|Finished in [0-9.e-]* seconds|Finished in <time> seconds|' \
      -e 's|"ruby_platform":"[^"]*"|"ruby_platform":"<platform>"|' \
      -e 's|) \[[^]]*\]$|) [<platform>]|'
}
only=" $* "
while IFS='|' read -r name cwd args input; do
  case "$name" in '' | '#'*) continue ;; esac
  [ "$only" = "  " ] || case "$only" in *" $name "*) ;; *) continue ;; esac
  stdin=/dev/null
  [ -n "${input:-}" ] && stdin=$root/$input
  # fix-* cases autocorrect, so they run on a fresh copy of their directory.
  run_dir=$root/$cwd
  case "$name" in fix-*)
    run_dir=${TMPDIR:-/tmp}/rbcop-formatters-fix
    rm -rf "$run_dir" && mkdir -p "$run_dir" && cp -R "$root/$cwd/." "$run_dir"
    run_dir=$(cd "$run_dir" && pwd -P) ;;
  esac
  force=--force-default-config
  case "$name" in cfg-*) force= ;; esac
  set +e
  (cd "$run_dir" && eval "RUBY_PROGRESS_BAR_LENGTH=80 rubocop --cache false $force --only $cops $args" \
    < "$stdin" 2>/dev/null) > "$dir/expected/$name.raw"
  echo $? > "$dir/expected/$name.status"
  set -e
  normalise < "$dir/expected/$name.raw" > "$dir/expected/$name.txt"
  rm "$dir/expected/$name.raw"
  echo "$name: exit $(cat "$dir/expected/$name.status")"
done < "$dir/cases.txt"

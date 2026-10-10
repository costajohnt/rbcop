#!/bin/sh
# Record RuboCop's output for every case in tests/formatters/cases.txt into tests/formatters/expected/
# (<name>.txt is stdout, <name>.status the exit status); tests/formatters.rs replays them with rbcop.
# A case line is `name|directory (from the repo root)|arguments[|stdin file (from the repo root)]`.
# RuboCop runs with --cache false --force-default-config --only <tests/formatters/cops.txt>, so the
# goldens do not move when rbcop gains cops. Machine-specific text is normalised (see normalise()).
# usage: scripts/formatters.sh [name...]   (needs `rubocop` on PATH; with rbenv set RBENV_VERSION)
set -eu
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
  set +e
  (cd "$root/$cwd" && eval "RUBY_PROGRESS_BAR_LENGTH=80 rubocop --cache false --force-default-config --only $cops $args" \
    < "$stdin" 2>/dev/null) > "$dir/expected/$name.raw"
  echo $? > "$dir/expected/$name.status"
  set -e
  normalise < "$dir/expected/$name.raw" > "$dir/expected/$name.txt"
  rm "$dir/expected/$name.raw"
  echo "$name: exit $(cat "$dir/expected/$name.status")"
done < "$dir/cases.txt"

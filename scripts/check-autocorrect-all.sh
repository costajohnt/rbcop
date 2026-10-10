#!/bin/sh
# Autocorrect gate with every cop rbcop implements enabled together, so corrections that interact
# across cops (one cop's -A output feeding another) are compared with RuboCop too.
# usage: scripts/check-autocorrect-all.sh [corpus...]   (env as for check-autocorrect.sh)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
(cd "$root" && cargo build --release -q)
# --show-cops prints RuboCop's YAML; implemented cop names are the unindented keys.
# Lint/RedundantCopDisableDirective cannot be used with --only (RuboCop aborts), so it is left out
# here; check-autocorrect.sh with that cop in its list runs it without --only.
cops=$("$root/target/release/rbcop" --show-cops | sed -n 's/^\([A-Z][A-Za-z]*\/[A-Za-z0-9]*\):$/\1/p' \
  | grep -vx 'Lint/RedundantCopDisableDirective' | paste -sd, -)
exec sh "$root/scripts/check-autocorrect.sh" "$cops" "$@"

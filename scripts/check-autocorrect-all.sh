#!/bin/sh
# Autocorrect gate with every cop rbcop implements enabled together, so corrections that interact
# across cops (one cop's -A output feeding another) are compared with RuboCop too.
# usage: scripts/check-autocorrect-all.sh [corpus...]   (env as for check-autocorrect.sh)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/rbcop.sh"
# Lint/RedundantCopDisableDirective cannot be used with --only (RuboCop aborts), so it is left out
# here; check-autocorrect.sh with that cop in its list runs it without --only.
cops=$(rbcop_only_cops)
exec sh "$root/scripts/check-autocorrect.sh" "$cops" "$@"

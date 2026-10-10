# shellcheck shell=sh
# Sourced by every script that runs RuboCop. Defines `rubocop` as $RUBOCOP (default: `rubocop` on
# PATH), word-split so a wrapper works, and exits loudly when it does not run, so a missing RuboCop
# never turns into empty oracles or "every file differs". With rbenv, for example:
#   RUBOCOP="rbenv exec rubocop" RBENV_VERSION=4.0.6 scripts/<script>.sh ...
rubocop_cmd=${RUBOCOP:-rubocop}
# `command` skips this function, so the default `rubocop` does not recurse.
# shellcheck disable=SC2086
rubocop() { command $rubocop_cmd "$@"; }
if ! rubocop --version > /dev/null 2>&1; then
  echo "$0: RuboCop does not run as '$rubocop_cmd'; set RUBOCOP (e.g. RUBOCOP=\"rbenv exec rubocop\" RBENV_VERSION=4.0.6)" >&2
  exit 1
fi

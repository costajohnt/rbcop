# shellcheck shell=sh
# Sourced by every script that runs RuboCop. Defines `rubocop` as $RUBOCOP (default: `rubocop` on
# PATH), word-split so a wrapper works, and exits loudly when it does not run, so a missing RuboCop
# never turns into empty oracles or "every file differs". With rbenv, for example:
#   RUBOCOP="rbenv exec rubocop" RBENV_VERSION=4.0.6 scripts/<script>.sh ...
rubocop_cmd=${RUBOCOP:-rubocop}
# Scripts run RuboCop from temp dirs, where rbenv would not see the repo's .ruby-version: pin it.
# No effect without rbenv.
rubocop_root=$(cd "$(dirname "$0")/.." && pwd)
if [ -z "${RBENV_VERSION:-}" ] && [ -f "$rubocop_root/.ruby-version" ]; then
  RBENV_VERSION=$(cat "$rubocop_root/.ruby-version")
  export RBENV_VERSION
fi
# `command` skips this function, so the default `rubocop` does not recurse.
# shellcheck disable=SC2086
rubocop() { command $rubocop_cmd "$@"; }
# Checked from outside the repo, the way the scripts call it.
if ! (cd / && rubocop --version) > /dev/null 2>&1; then
  echo "$0: RuboCop does not run as '$rubocop_cmd'; set RUBOCOP (e.g. RUBOCOP=\"rbenv exec rubocop\" RBENV_VERSION=4.0.6)" >&2
  exit 1
fi

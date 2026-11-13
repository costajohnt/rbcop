# shellcheck shell=sh
# Sourced by every script that runs RuboCop. Defines `rubocop` as $RUBOCOP (default: `rubocop` on
# PATH), word-split so a wrapper works, and exits loudly when it does not run, so a missing RuboCop
# never turns into empty oracles or "every file differs". With rbenv, for example:
#   RUBOCOP="rbenv exec rubocop" RBENV_VERSION=4.0.6 scripts/<script>.sh ...
rubocop_cmd=${RUBOCOP:-rubocop}
# Scripts run RuboCop from temp dirs, where rbenv would not see the repo's .ruby-version: pin it.
# No effect without rbenv.
# The repo root is the nearest directory above the sourcing script that holds Cargo.toml: scripts/
# and tests/<dir>/ both source this file, so `$(dirname "$0")/..` is not always the root.
rubocop_root=$(cd "$(dirname "$0")" && pwd)
while [ ! -f "$rubocop_root/Cargo.toml" ]; do
  if [ "$rubocop_root" = / ]; then
    echo "$0: no Cargo.toml above $(dirname "$0"); cannot find the rbcop repo root" >&2
    exit 1
  fi
  rubocop_root=$(dirname "$rubocop_root")
done
if [ -z "${RBENV_VERSION:-}" ] && [ -f "$rubocop_root/.ruby-version" ]; then
  RBENV_VERSION=$(cat "$rubocop_root/.ruby-version")
  export RBENV_VERSION
fi
# `command` skips this function, so the default `rubocop` does not recurse.
# shellcheck disable=SC2086
rubocop() { command $rubocop_cmd "$@"; }
# Checked from outside the repo, the way the scripts call it.
if ! rubocop_version=$(cd / && rubocop --version 2> /dev/null); then
  echo "$0: RuboCop does not run as '$rubocop_cmd'; set RUBOCOP (e.g. RUBOCOP=~/rubocop-1.92.2/bin/rubocop)" >&2
  exit 1
fi
# And it is the release rbcop targets (src/config/mod.rs RUBOCOP_VERSION), so an older RuboCop
# never records oracles or passes a live gate. Moving to a new release
# (docs/upgrading-rubocop.md), set RUBOCOP_EXPECTED to it.
rubocop_expected=${RUBOCOP_EXPECTED:-$(sed -n 's/^pub const RUBOCOP_VERSION: &str = "\(.*\)";$/\1/p' "$rubocop_root/src/config/mod.rs")}
rubocop_version=$(echo "$rubocop_version" | head -n 1)
if [ "$rubocop_version" != "$rubocop_expected" ]; then
  echo "$0: '$rubocop_cmd' is RuboCop $rubocop_version, not $rubocop_expected (set RUBOCOP, or RUBOCOP_EXPECTED when upgrading)" >&2
  exit 1
fi
# Same for the gems `-V` names: Parser, Prism, rubocop-ast and ruby (the constants beside
# RUBOCOP_VERSION) and each native plugin (the docs/plugins.md pins), so a host with another
# rubocop-rails or prism never records or compares oracles. `-V` lists plugins only from a config
# that loads them, and Prism only for a target of 3.3 or newer. When an upgrade moves one, set
# RUBOCOP_EXPECTED_DEPS to the new pins as gem:version words, e.g. "rubocop-ast:1.51.0 prism:2.1.0".
rubocop_const() { sed -n "s/^pub const $1: &str = \"\(.*\)\";\$/\1/p" "$rubocop_root/src/config/mod.rs"; }
rubocop_plugins=$(sed -n 's/^| \(rubocop-[a-z_]*\) | \([0-9][^ ]*\) |.*/\1:\2/p' "$rubocop_root/docs/plugins.md")
rubocop_pins=$(
  for rubocop_d in "parser:$(rubocop_const PARSER_VERSION)" "prism:$(rubocop_const PRISM_VERSION)" \
    "rubocop-ast:$(rubocop_const RUBOCOP_AST_VERSION)" "ruby:$(rubocop_const RUBY_VERSION)" $rubocop_plugins; do
    case " ${RUBOCOP_EXPECTED_DEPS:-} " in *" ${rubocop_d%%:*}:"*) ;; *) echo "$rubocop_d" ;; esac
  done
  for rubocop_d in ${RUBOCOP_EXPECTED_DEPS:-}; do echo "$rubocop_d"; done
)
rubocop_tmp=$(mktemp -d)
{
  echo "AllCops:"
  echo "  TargetRubyVersion: 3.4"
  echo "plugins:"
  for rubocop_d in $rubocop_plugins; do echo "  - ${rubocop_d%%:*}"; done
} > "$rubocop_tmp/.rubocop.yml"
if ! rubocop_v=$(cd "$rubocop_tmp" && rubocop -V 2>&1); then
  rm -rf "$rubocop_tmp"
  printf '%s: %s -V fails with the docs/plugins.md plugins loaded:\n%s\n' "$0" "$rubocop_cmd" "$rubocop_v" >&2
  exit 1
fi
rm -rf "$rubocop_tmp"
rubocop_deps=$(echo "$rubocop_v" | sed -n \
  -e '1s/.*using Parser \([^,]*\), Prism \([^,]*\),.* rubocop-ast \([^,]*\),.* running on ruby \([^)]*\)).*/parser:\1 prism:\2 rubocop-ast:\3 ruby:\4/p' \
  -e 's/^  - \([^ ]*\) \([^ ]*\)$/\1:\2/p')
# shellcheck disable=SC2086 # word lists
if [ "$(printf '%s\n' $rubocop_deps | sort)" != "$(printf '%s\n' $rubocop_pins | sort)" ]; then
  echo "$0: '$rubocop_cmd' -V does not match the pinned gems (src/config/mod.rs, docs/plugins.md; set RUBOCOP, or RUBOCOP_EXPECTED_DEPS when upgrading)" >&2
  echo "expected: $(echo $rubocop_pins)" >&2
  echo "found:    $(echo $rubocop_deps)" >&2
  exit 1
fi

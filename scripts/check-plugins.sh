#!/bin/sh
# Plugin parity gate (docs/plugins.md): for each native plugin, every cop of it rbcop implements
# against RuboCop oracles recorded with that plugin (scripts/oracle.sh with
# CONFIG=config/all-cops-<plugin>.yml), on the nine corpora and the corpus2 projects, then -A
# against live RuboCop (scripts/check-autocorrect.sh) on the corpora where the plugin's cops fire.
# usage: scripts/check-plugins.sh
# env:   RBCOP_CORPUS, RBCOP_ORACLE (as for check-cops.sh); RBCOP_PLUGIN_ORACLE (default
#        $RBCOP_ORACLE/../oracle-<plugin>), RBCOP_CORPUS2 (default $RBCOP_CORPUS/../corpus2);
#        PLUGIN_AC_CORPORA (corpora for the -A pass, default specs rails discourse mastodon brew)
# Exit 0 only if every plugin matches everywhere.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/rbcop.sh"
corpus=${RBCOP_CORPUS:?set RBCOP_CORPUS}
corpus2=${RBCOP_CORPUS2:-$corpus/../corpus2}
status=0
for spec in performance:Performance; do
  plugin=${spec%%:*} dept=${spec#*:}
  oracle=${RBCOP_PLUGIN_ORACLE:-${RBCOP_ORACLE:?set RBCOP_ORACLE}/../oracle-$plugin}
  [ -d "$oracle" ] || { echo "check-plugins: no oracles for rubocop-$plugin in $oracle" >&2; exit 1; }
  cops=$(CONFIG="$root/config/all-cops-$plugin.yml" rbcop_cops | tr , '\n' | grep "^$dept/" | paste -sd, -)
  [ -n "$cops" ] || { echo "check-plugins: no $dept cops in --show-cops" >&2; exit 1; }
  echo "rubocop-$plugin: $(echo "$cops" | tr , '\n' | wc -l | tr -d ' ') cops"
  export CONFIG="$root/config/all-cops-$plugin.yml"
  RBCOP_ORACLE=$oracle sh "$root/scripts/check-cops.sh" "$cops" || status=1
  RBCOP_ORACLE=$oracle RBCOP_CORPUS=$corpus2 sh "$root/scripts/check-cops.sh" "$cops" \
    sidekiq puma devise rspec graphql-ruby solidus || status=1
  CONFIG="$root/config/plugin-$plugin.yml" RBCOP_ORACLE=$oracle \
    sh "$root/scripts/check-autocorrect.sh" "$cops" ${PLUGIN_AC_CORPORA:-specs rails discourse mastodon brew} || status=1
  unset CONFIG
done
[ $status = 0 ] && echo "plugins: all corpora match"
exit $status

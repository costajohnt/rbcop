#!/bin/sh
# Plugin parity gate (docs/plugins.md): for each native plugin, every cop of it rbcop implements
# against RuboCop oracles recorded with that plugin (scripts/oracle.sh with
# CONFIG=config/all-cops-<plugin>.yml), on the nine corpora and the corpus2 projects, then -A
# against live RuboCop (scripts/check-autocorrect.sh) on the corpora where the plugin's cops fire.
# usage: scripts/check-plugins.sh
# env:   RBCOP_CORPUS, RBCOP_ORACLE (as for check-cops.sh); RBCOP_PLUGIN_ORACLE (default
#        $RBCOP_ORACLE/../oracle-<plugin>), RBCOP_CORPUS2 (default $RBCOP_CORPUS/../corpus2);
#        PLUGIN_AC_CORPORA (corpora for the -A pass, default specs rails discourse mastodon brew;
#        the rspec family adds the corpus2 projects rspec and graphql-ruby; not solidus, where
#        corrections RuboCop and rbcop share leave intermediate syntax errors in 8 Ruby 3.2 files
#        whose Lint/Syntax wording differs, a core gap in docs/known-gaps.md; final files match)
# rubocop-rails also changes some of RuboCop's own cops (its default.yml settings, Style/HashExcept's
# minimum Ruby, MigratedSchemaVersion), so those are checked against its oracles too, and mastodon is
# checked once more with its own .rubocop.yml (oracle-rspec/mastodon-project.txt, recorded with
# CONFIG=$RBCOP_CORPUS/mastodon/.rubocop.yml and the plugin gems docs/plugins.md pins; mastodon
# loads rails, performance and the rspec family, so this runs with the last of them, rspec).
# The rspec family (rubocop-rspec, rubocop-rspec_rails, rubocop-factory_bot, rubocop-capybara)
# shares one oracle set (config/all-cops-rspec.yml loads all four) and also changes
# Metrics/BlockLength's Exclude.
# Exit 0 only if every plugin matches everywhere.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/rbcop.sh"
corpus=${RBCOP_CORPUS:?set RBCOP_CORPUS}
corpus2=${RBCOP_CORPUS2:-$corpus/../corpus2}
status=0
for spec in performance:Performance rails:Rails minitest:Minitest rspec:RSpec,RSpecRails,FactoryBot,Capybara; do
  plugin=${spec%%:*} dept=${spec#*:}
  oracle=${RBCOP_PLUGIN_ORACLE:-${RBCOP_ORACLE:?set RBCOP_ORACLE}/../oracle-$plugin}
  [ -d "$oracle" ] || { echo "check-plugins: no oracles for rubocop-$plugin in $oracle" >&2; exit 1; }
  cops=$(CONFIG="$root/config/all-cops-$plugin.yml" rbcop_cops | tr , '\n' | grep -E "^($(echo "$dept" | tr , '|'))/" | paste -sd, -)
  [ -n "$cops" ] || { echo "check-plugins: no $dept cops in --show-cops" >&2; exit 1; }
  [ "$plugin" = rspec ] && cops="$cops,Metrics/BlockLength"
  [ "$plugin" = rails ] && cops="$cops,Lint/NumberConversion,Lint/RedundantSafeNavigation,Lint/SafeNavigationChain,Lint/UselessAccessModifier,Lint/UselessMethodDefinition,Style/AndOr,Style/CollectionCompact,Style/FormatStringToken,Style/HashExcept,Style/InvertibleUnlessCondition,Style/SymbolProc"
  echo "rubocop-$plugin: $(echo "$cops" | tr , '\n' | wc -l | tr -d ' ') cops"
  export CONFIG="$root/config/all-cops-$plugin.yml"
  RBCOP_ORACLE=$oracle sh "$root/scripts/check-cops.sh" "$cops" || status=1
  RBCOP_ORACLE=$oracle RBCOP_CORPUS=$corpus2 sh "$root/scripts/check-cops.sh" "$cops" \
    sidekiq puma devise rspec graphql-ruby solidus || status=1
  CONFIG="$root/config/plugin-$plugin.yml" RBCOP_ORACLE=$oracle \
    sh "$root/scripts/check-autocorrect.sh" "$cops" ${PLUGIN_AC_CORPORA:-specs rails discourse mastodon brew} || status=1
  if [ "$plugin" = rspec ]; then
    CONFIG="$root/config/plugin-$plugin.yml" RBCOP_ORACLE=$oracle RBCOP_CORPUS=$corpus2 \
      sh "$root/scripts/check-autocorrect.sh" "$cops" rspec graphql-ruby || status=1
  fi
  unset CONFIG
  if [ "$plugin" = rspec ]; then
    # A real Rails app's own config: every cop rbcop implements, as mastodon configures them.
    project="$corpus/mastodon/.rubocop.yml"
    mcops=$(CONFIG=$project rbcop_cops)
    mkdir -p "${TMPDIR:-/tmp}/rbcop-mastodon-project"
    ln -sf "$oracle/mastodon-project.txt" "${TMPDIR:-/tmp}/rbcop-mastodon-project/mastodon.txt"
    ln -sf "$oracle/mastodon-project.txt.crashes" "${TMPDIR:-/tmp}/rbcop-mastodon-project/mastodon.txt.crashes"
    # Under its own config mastodon has a handful of offenses (rspec_rails cops newer than its lock).
    ALLOW_EMPTY_ORACLE=1 CONFIG=$project RBCOP_ORACLE="${TMPDIR:-/tmp}/rbcop-mastodon-project" sh "$root/scripts/check-cops.sh" "$mcops" mastodon || status=1
  fi
done
[ $status = 0 ] && echo "plugins: all corpora match"
exit $status

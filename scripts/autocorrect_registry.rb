# Dumps RuboCop's cop registry order with each cop's `autocorrect_incompatible_with` list.
# Team#each_corrector merges correctors in this order; rbcop reads the output from
# src/autocorrect/registry.tsv. Regenerate when upgrading RuboCop:
#   RBENV_VERSION=4.0.6 rbenv exec ruby scripts/autocorrect_registry.rb > src/autocorrect/registry.tsv
require 'rubocop'

RuboCop::Cop::Registry.global.filter_by_badge { true }.cops.each do |cop|
  incompatible = cop.autocorrect_incompatible_with.map(&:cop_name)
  puts [cop.cop_name, *incompatible].join("\t")
end

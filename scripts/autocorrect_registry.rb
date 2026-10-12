# Dumps RuboCop's cop registry order with each cop's `autocorrect_incompatible_with` list.
# Team#each_corrector merges correctors in this order; rbcop reads the output from
# src/autocorrect/registry.tsv. Regenerate when upgrading RuboCop:
#   RBENV_VERSION=4.0.6 rbenv exec ruby scripts/autocorrect_registry.rb > src/autocorrect/registry.tsv
require 'rubocop'
# Native plugins (src/helpers/cop_registry.rs NATIVE_PLUGINS): their cops follow RuboCop's, and
# rubocop-performance adds itself to two core cops' autocorrect_incompatible_with.
require 'rubocop-performance'
require 'rubocop-rails'
require 'rubocop-rspec'
require 'rubocop-rspec_rails'
require 'rubocop-factory_bot'
require 'rubocop-capybara'

RuboCop::Cop::Registry.global.filter_by_badge { true }.cops.each do |cop|
  incompatible = cop.autocorrect_incompatible_with.map(&:cop_name)
  puts [cop.cop_name, *incompatible].join("\t")
end

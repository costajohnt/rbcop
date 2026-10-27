# frozen_string_literal: true

# Writes the documentation sections `rubocop --explain` shows (Cop::CopDocumentation#sections, read
# from the comment above each cop class) for RuboCop's cops and the native plugins' cops, as
# src/cop_docs.txt for src/explain.rs. Run with the target RuboCop release and the plugin pins of
# docs/plugins.md on the load path:
#   RBENV_VERSION=4.0.6 ruby scripts/dump-cop-docs.rb > src/cop_docs.txt
# Format, one record per line: `C name` starts a cop, `S title` a section, `L text` a body line.
require 'rubocop'
%w[rubocop-performance rubocop-rails rubocop-minitest rubocop-rspec rubocop-rspec_rails
   rubocop-factory_bot rubocop-capybara].each { |gem| require gem }

RuboCop::Cop::Registry.global.sort_by(&:cop_name).each do |cop|
  sections = RuboCop::Cop::CopDocumentation.new(cop).sections
  next if sections.empty?

  puts "C #{cop.cop_name}"
  sections.each do |title, body|
    puts "S #{title}"
    body.each { |line| puts "L #{line}" }
  end
end

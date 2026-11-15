# frozen_string_literal: true

# Trimmed from standard 1.57.0's lib/standard.rb (MIT, see LICENSE.txt) for rbcop's scenarios: the
# require that changes RuboCop. The rest (CLI, formatter, LSP, lint_roller plugin) does nothing in
# a `rubocop` run.
require "rubocop"

require "standard/rubocop/ext"

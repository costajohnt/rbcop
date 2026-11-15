# frozen_string_literal: true

def foo
  # standard:disable Metrics/AbcSize
  bar
end
x = "a" # standard:disable Style/StringLiterals
puts x # rubocop:disable Style/StringLiterals

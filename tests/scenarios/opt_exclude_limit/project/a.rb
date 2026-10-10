# frozen_string_literal: true

def foo(unused)
  x = 1
  y = "a" # rubocop:disable Style/StringLiterals
  raise Exception if x == x
end

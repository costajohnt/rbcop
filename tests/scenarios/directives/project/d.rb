# frozen_string_literal: true

x = 1 
y = 2 # rubocop:disable Layout/TrailingWhitespace 
# rubocop:disable Layout
z = 3 
# rubocop:enable Layout
a = 4 
# rubocop:disable all
binding.pry
# rubocop:enable all
binding.pry
b = x == nil # rubocop:todo Style/NilComparison
c = y == nil # rubocop:disable NilComparison -- with a reason
d = z == nil ## rubocop:disable Style/NilComparison
# rubocop:disable Lint/Debugger, Style/NilComparison
binding.pry
e = a == nil
# rubocop:enable Lint/Debugger
binding.pry
f = b == nil
# rubocop:enable Style/NilComparison
puts x, y, z, a, b, c, d, e, f

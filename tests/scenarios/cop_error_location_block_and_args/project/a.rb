# frozen_string_literal: true

# rubocop:disable Output
foo(lambda { |a | a })
bar { |b | b }
def m(a, b, c, d, e, f); end
baz do |a, b, c, d, e, f|
  a
end
puts 1

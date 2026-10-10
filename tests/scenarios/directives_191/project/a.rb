# frozen_string_literal: true

# rubocop:disable-next Style/NilComparison
a = x == nil
b = x == nil
# rubocop:todo-next Lint/Debugger
foo(1,
    binding.pry,
    2)
binding.pry
# rubocop:disable-next Style/NilComparison

c = x == nil
# rubocop:disable-next Layout/TrailingWhitespace
s = <<~TXT 
  body 
TXT
t = 1 
# rubocop:next +Style/Not -Style/NilComparison
d = not(x == nil)
e = not(x)
# rubocop:disable Style/NilComparison
f = x == nil
# rubocop:enable-next Style/NilComparison
g = x == nil
h = x == nil
# rubocop:enable Style/NilComparison
# rubocop:push -Lint/Debugger -Layout
binding.pry 
# rubocop:push +Lint/Debugger
binding.pry
# rubocop:pop
binding.pry 
# rubocop:pop
binding.pry 
# rubocop:enable LineLength
z = 'qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq'
# rubocop:enable Layout/LineLength
w = 'qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq'
foo(a, b, c, d, e, f, g, h, s, t, z, w)

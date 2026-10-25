# frozen_string_literal: true

def method_with_a_very_long_name_that_needs_a_todo_comment_appended_to_it(argument_one, b)
  argument_one + b
end
x = 1
y = <<~TEXT
  #{x} heredoc text
TEXT
z = %w[
  one two
]
eval("1 + 1")
w = "a" \
  "b"
v = "interp #{x}
second line"
def m(a, b, c, d, e, f)
  a + b + c + d + e + f
end
puts y, z, w, v
if x then puts 1 end

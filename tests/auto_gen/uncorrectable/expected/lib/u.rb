# frozen_string_literal: true

# rubocop:todo Naming/MethodParameterName
def method_with_a_very_long_name_that_needs_a_todo_comment_appended_to_it(
  argument_one, b
)
  # rubocop:enable Naming/MethodParameterName
  argument_one + b
end
x = 1
y = <<~TEXT
  #{x} heredoc text
TEXT
z = %w[
  one two
]
eval('1 + 1') # rubocop:todo Style/EvalWithLocation
w = 'a' \
    'b'
v = "interp #{x}
second line"
# rubocop:todo Naming/MethodParameterName
def m(a, b, c, d, e, f) # rubocop:todo Metrics/ParameterLists, Naming/MethodParameterName
  # rubocop:enable Naming/MethodParameterName
  a + b + c + d + e + f
end
puts y, z, w, v
puts 1 if x

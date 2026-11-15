# frozen_string_literal: true

x = 1
if (a = x)
  a
end
if b = x # rubocop:disable Lint/AssignmentInCondition
  b
end
# rubocop:disable-next Style/StringLiterals
y = "one"
z = 'two'
# rubocop:todo Style/StringLiterals
w = "three"
# rubocop:enable all
v = 'four' if x == 1
# rubocop: disable-next Style/StringLiterals, Layout/SpaceAroundOperators
u ="five"
puts y, z, w, v, u

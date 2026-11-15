# frozen_string_literal: true

x = 1
if (a = x) # rubocop:disable Lint/UselessAssignment
  a
end
if b = x # rubocop:disable Lint/AssignmentInCondition
  b
end
# rubocop:disable Style/StringLiterals
y = "one"
# rubocop:enable Style/StringLiterals
z = "two"
# rubocop:todo Style/StringLiterals
w = "three"
# rubocop:enable all
# rubocop:disable Lint
v = "four" if x == 1
# rubocop:enable Lint
#rubocop: disable Style/StringLiterals, Layout/SpaceAroundOperators
u ="five"
# rubocop:enable Style/StringLiterals, Layout/SpaceAroundOperators
t = "six" # rubocop:disable Style/StringLiterals
s = "seven" # rubocop:todo Style/StringLiterals
puts y, z, w, v, u

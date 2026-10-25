# frozen_string_literal: true

x = "a" # rubocop:disable Style/StringLiterals -- because reasons
y = "b" # rubocop:todo Style/StringLiterals
# rubocop:disable Style/StringLiterals
z = "c"
# rubocop:enable all
w = 'd'
puts x, y, z, w
eval(w)

# frozen_string_literal: true

x = 1
if b = x # standard:disable Lint/AssignmentInCondition
  puts "b" # rubocop:disable Style/StringLiterals
end
if c = x
  puts c
end

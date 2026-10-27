# frozen_string_literal: true

# rubocop:push +StringLiterals
puts "x"
# rubocop:pop
# rubocop:enable-next Style
x=1;
# rubocop:next +Lint
binding.irb
puts "y"

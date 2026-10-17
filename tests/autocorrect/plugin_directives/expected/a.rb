# frozen_string_literal: true

x = [1, 2].select { |y| y > 1 }.first # rubocop:disable Performance/Detect
puts x
w = [1, 2].detect { |y| y > 1 }
puts w
foo # rubocop:disable Custom/NoFoo
z = 1 # rubocop:disable Performance/Bogus
puts z
# rubocop:disable Performance
v = [3].select(&:odd?).first
# rubocop:enable Performance
puts v

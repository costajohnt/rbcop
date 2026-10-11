# frozen_string_literal: true

# rubocop:disable-next Security/IoMethods
IO.read('x')
# rubocop:disable Security/IoMethods
IO.read('y')
# rubocop:enable Security/IoMethods
IO.read('z') # rubocop:disable Security/IoMethods

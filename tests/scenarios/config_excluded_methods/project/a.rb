# frozen_string_literal: true

included do
  a
  b
end
foo { |x| x.bar }
baz { |x| x.bar }
long_call_with_many_arguments(1, 2, 3)
other_call_with_many_arguments(1, 2, 3)

# frozen_string_literal: true

def threshold
  Time.current - months.months
  Time.current - foo.days
  Time.current - 3.months
  Time.current - 1.5.hours
  Time.current - nil.foo.days
  Time.current - nil.days
  Time.current - nil.foo(1).days
end

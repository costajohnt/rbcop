# frozen_string_literal: true

# A class.
class Foo
  def bar(arg)
    return 1 if arg

    x = arg ? 2 : 3
    x + 1
  end

  def self.baz
    a = 1
    a + 2
  end
end

# A module.
module Mod
  X = 1
  Y = 2
end

items = [1, 2]
items.each do |i|
  puts i
  puts i
end
handler = lambda do |v|
  puts v
  puts v
end
handler.call(1)
define_method(:qux) do |arg|
  arg ? 1 : 2
  arg && 3
end

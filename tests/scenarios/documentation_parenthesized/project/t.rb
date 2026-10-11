# frozen_string_literal: true

# Doc
(class Foo
  def x; end
end)

# Doc
(module Bar
  def self.x; end
end)

# Doc
x = (class Baz
  def x; end
end)
puts x

(y
 # Doc
)
class Qux
  def x; end
end

# Doc
((class Quux
  def x; end
end))

(class Corge # :nodoc:
  def x; end
end)

# Doc
(def y; end)

(def z; end)

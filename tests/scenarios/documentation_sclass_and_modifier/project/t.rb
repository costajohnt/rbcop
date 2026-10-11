module P
  class << self
    include X
  end
end

class Q
  class << self
    include X
    extend Y
  end
end

module R
  class << self
  end
end

module S
  class << foo
    include X
  end
end

module T
  class << self
    include X
    def x; end
  end
end

# Doc.
class A
  def foo; end
end if x

# Doc.
module B
  def bar; end
end unless y

# Doc.
class C
  # Doc.
  def baz
  end while z

  # Doc.
  def qux
  end until z

  # Doc.
  def quux
  end if z
end

# Doc.
class D
  # Doc.
  private def foo
  end
end

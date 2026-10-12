def Item.by_name(name) where(name) end
def Item.by_name(name) where(name) end

def Item.a; 1; end
def Item.a; 2; end

def Item.b; end
def Item.b; end

describe "x" do
  it "a" do
    def Item.c(x, y = 1, *r, k:, o: 2.5, **kw, &blk)
      @z = x&.foo(y) + -3
      return [r, k, { a: :b, "c" => nil }], Foo::Bar, ::Baz
    end
  end
  it "b" do
    def Item.c(x, y = 1, *r, k:, o: 2.5, **kw, &blk)
      @z = x&.foo(y) + -3
      return [r, k, { a: :b, "c" => nil }], Foo::Bar, ::Baz
    end
  end
end

class Foo
  def Item.d
    v = "a\tbé"
    !v.empty? && self.class && $g && @@c && true || false
  end

  def Item.d
    v = "a\tbé"
    !v.empty? && self.class && $g && @@c && true || false
  end
end

def Item.e() = :sym
def Item.e() = :sym

def Item.f(a) = a.to_s
def Item.f(b) = b.to_s

module Item
  def Item.g; end
  def Item.g; end
end

class Bar
  def Item.h(x) [x, x.size, 1.0e20, 10**30] end
  def Item.h(x) [x, x.size, 1.0e20, 10**30] end
end

class A
  def self&.drop!
    2
  end

  def self &.foo(a) a end
end

def (x)&.bar; end
def obj&.baz = 1

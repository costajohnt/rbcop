class C
  def a(x); self.[](x); end
  def f(x); self.[]=(x, 1); end
  def b(x); self.+(x); end
  def c(x); self[x]; end
  def d(x); self[x] = 1; end
  def e; self.foo; end
end

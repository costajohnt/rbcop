def foo
  x = 
  @bar ||= calc(x)
end
def baz
  puts @qux ||= 1
end
def q
  A = @z ||= 2
end
def r
  a, b = @w ||= 3
end

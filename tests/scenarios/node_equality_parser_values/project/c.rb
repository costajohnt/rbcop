t = (__LINE__) ==
  (__LINE__)
if c
  foo
  bar("#{__LINE__}")
else
  baz
  bar("#{__LINE__}")
end
if c
  v = x
else
  x = 1
  v = x
end
u = (__LINE__) == (__LINE__)

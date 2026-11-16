foo(a, b,)
foo(a, b)
foo(
  a,
  b,
)
foo(
  a,
  b
)
foo(bar(1, 2), 3)
foo(a, bar(1,))
foo(<<~A, b,)
  x, y
A
foo(<<~A, b)
  x, y
A
foo(a, "#{b}, c",)
foo(a, [1, 2])
x = 1
foo(a, b, # c
)

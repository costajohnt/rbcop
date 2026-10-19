h = { <<~K => 1, <<~L => 2 }
  k
K
  k
L
h2 = { <<~K => 1, <<~L => 2 }
  k
K
    k
L
h3 = { <<~K => 1, "k\nj\n" => 2 }
  k
  j
K
h4 = { <<~K => 1, "k\n#{x}\n" => 2 }
  k
  #{x}
K
if c
  foo(<<~B)
    y
  B
else
  foo(<<~C)
    y
  C
end
s = Set[<<~S, "s\n"]
	s
S
p(<<~A == "x\n")
  x
A
p h, h2, h3, h4, s

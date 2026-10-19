if c
  foo(<<~B)
    y
  B
else
  foo("y\n")
end
case z
when <<~W then 1
  w
W
when "w\n" then 2
end

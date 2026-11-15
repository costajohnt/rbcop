# Nine branches, see a.rb (#785).
case c
when 1 then foo("y\n")
when 2 then foo("y\n")
when 3 then foo("y\n")
when 4 then foo("y\n")
when 5 then foo("y\n")
when 6 then foo("y\n")
when 7 then foo("y\n")
when 8 then foo("y\n")
else
  foo(<<~B)
    y
  B
end
case z
when <<~W then 1
  w
W
when "w\n" then 2
end

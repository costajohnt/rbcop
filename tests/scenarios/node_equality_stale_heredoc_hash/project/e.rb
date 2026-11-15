if c
  v = <<~'A'
  x\n
A

else
  v = <<~B
  x\\n
B

end
# Nine branches, see a.rb (#785).
case c
when 1 then v = <<~`A`
  ls
A
when 2 then v = <<~`A`
  ls
A
when 3 then v = <<~`A`
  ls
A
when 4 then v = <<~`A`
  ls
A
when 5 then v = <<~`A`
  ls
A
when 6 then v = <<~`A`
  ls
A
when 7 then v = <<~`A`
  ls
A
when 8 then v = <<~`A`
  ls
A
else
  v = <<~`B`
    ls
B
end
# Nine branches, see a.rb (#785).
case c
when 1 then v = <<~A
  #{a}
  x
A
when 2 then v = <<~A
  #{a}
  x
A
when 3 then v = <<~A
  #{a}
  x
A
when 4 then v = <<~A
  #{a}
  x
A
when 5 then v = <<~A
  #{a}
  x
A
when 6 then v = <<~A
  #{a}
  x
A
when 7 then v = <<~A
  #{a}
  x
A
when 8 then v = <<~A
  #{a}
  x
A
else
  v = <<~B
    #{a}
    x
B
end
if c
  v = <<~A
  \tx
A

else
  v = <<~B
  \tx
B

end
if c
  v = <<~A
    \x41
  y
A

else
  v = <<~B
    A
  y
B

end

if c
  v = <<~'A'
  x\n
A

else
  v = <<~B
  x\\n
B

end
if c
  v = <<~`A`
  ls
A

else
  v = <<~`B`
    ls
B

end
if c
  v = <<~A
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

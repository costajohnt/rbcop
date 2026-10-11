x = "a \
#{ b }c"
puts x
x = "a \
#{ b + c }d"
puts x
x = "a \
#{ b }c #{ d }"
puts x
x = `a \
#{ b }c`
y = %x(a \
#{ b + c })
z = %W[a \
#{ b }]
w = <<~EOS
  a \
  #{ b }
EOS
v = "a \\
#{ b }"
u = "a \\\
#{ b + 1 }"
t = 'a' "b \
#{c}d"
puts x, y, z, w, v, u, t

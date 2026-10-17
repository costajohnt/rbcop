a = (proc { _1 })
b = (proc { it })
c = (lambda { it })
d = (x.lambda { 1 })
e = (Proc.new { it })
f = (proc { |v| v })
g = (-> { _1 })
h = (proc(1) { 2 })
foo x.lambda { 1 }
foo proc(1) { 2 }
foo y.proc { 1 }
foo Proc.new(1) { 2 }
foo ::Proc.new { it }
x.each { (_1) }
x.each { (it) }
x.each do
  (it.foo)
end
x.each do
  (_1.foo)
end
-> { (_1.foo) }
-> { (it.foo) }
x.each { (foo) { 1 } }
x.each { (it.foo) { 1 } }
x.each { (foo) { _1 } }
x.each { (foo) { it } }
puts a, b, c, d, e, f, g, h

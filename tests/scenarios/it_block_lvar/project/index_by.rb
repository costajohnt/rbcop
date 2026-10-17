Hash[x.map { [it, it] }]
x.map { [it, it] }.to_h
x.to_h { [it, it] }
x.to_h { [it, it.foo] }
x.to_h { [it.foo, it] }
x.each_with_object({}) { |e, h| h[e] = e }
x.map { [_1, _1] }.to_h
x.to_h { |e| [e, e] }

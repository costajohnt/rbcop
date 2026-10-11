arr.map &:to_s..x
arr.flatten[arr.flatten.map &:to_s.length - 2]
arr.map &x
arr.map &:to_s
arr.map &"s"
arr.map &-x
arr.map &method(:p)
x.foo &y
foo.map a, &x
foo(&x)
foo & x
foo &&x

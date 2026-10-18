/(?<_1>a)/ =~ s
_1

def two(str)
  /(?<a>x)(?<_2>y)/ =~ str
  bar { _2 + a }
end

foo { /(?<_3>a)/ =~ s; _3 }

case x
in {_4:}
  _4
in [*_5]
  _5
in {**_6}
  _6
in [*_8, 1, *]
  _8
end

baz { case y
in {_9:} then _9
end }

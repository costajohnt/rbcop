foo { /(?<_1>a)/ =~ s; bar { _1 }; _2 }
x = -> { /(?<_1>a)/ =~ s; bar { baz { _1 } }; _3 }
x.call
foo { case y; in {_1:} then bar { _1 }; end; _2 }
foo { /(?<_1>a)/ =~ s; bar { _1 }; _1 }

def m
  foo { _1 }
end

def n
  foo { _1&.bar(_1) || _2 }
end

def o
  foo { _1.x + _1.x }
end

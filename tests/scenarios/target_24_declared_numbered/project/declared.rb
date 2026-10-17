def m(xs)
  xs.map do |_1|
    _1 + _1
  end
end

def n(xs)
  xs.map { |_1, _2| _2 }
end

def o(xs)
  xs.map { _1 }
end

def q
  _1 = 3
  _1
end

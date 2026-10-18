def rest(*_1)
  _1
end

def kwrest(**_1)
  _1
end

def blk(&_1)
  _1
end

def kw(_1:)
  _1
end

def kwopt(_1: 2)
  _1
end

def self.srest(*_1)
  bar { _1 }
end

foo { |*_1| _1 }
foo { |**_1| _1 }
foo { |&_1| _1 }
foo { |_1:| _1 }
foo { |_1: 1| _1 }
->(*_1) { _1 }
-> **_1 { _1 }

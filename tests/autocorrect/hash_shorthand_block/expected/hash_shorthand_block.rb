def m(key, amount)
  instrument(:increment, key, amount:) do
    1
  end
end

def n(key, amount)
  instrument :increment, key, amount: amount { 1 }
end

def o(amount)
  foo amount:
end

def p(amount)
  super(amount:) do
  end
end

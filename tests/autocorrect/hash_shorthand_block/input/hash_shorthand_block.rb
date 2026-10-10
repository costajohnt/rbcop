def m(key, amount)
  instrument :increment, key, amount: amount do
    1
  end
end

def n(key, amount)
  instrument :increment, key, amount: amount { 1 }
end

def o(amount)
  foo amount: amount
end

def p(amount)
  super amount: amount do
  end
end

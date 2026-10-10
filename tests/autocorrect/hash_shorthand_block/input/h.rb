def m(key, amount)
  instrument :increment, key, amount: amount do
    1
  end
end

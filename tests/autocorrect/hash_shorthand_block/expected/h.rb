def m(key, amount)
  instrument(:increment, key, amount:) do
    1
  end
end

names = items.map { _1.name }
items.each_with_index { _1 }
sorted = items.sort_by { _1 }
items.each do
  _1
  42
end
x do
  (_1; bar)
end
a, b = 1, 2 rescue nil
puts names, sorted, a, b

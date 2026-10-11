if a.any? { |d| d }
  true
else
  false
end
x = a.any? { |d| d } ? true : false
x = a.any?(&:b) ? true : false
x = a.any? { _1 } ? true : false
x = !a.any? { _1 } ? true : false
x = (a.any? { _1 }) ? true : false

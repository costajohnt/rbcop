def f
  x = nil
  while true
    x = nil
    x = g
    break if x
  end
ensure
  false
end

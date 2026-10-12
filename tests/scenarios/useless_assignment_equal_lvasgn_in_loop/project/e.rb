begin
  x = nil
  until false
    x = nil
    x = g
    break if x
  end
rescue F
  false
end

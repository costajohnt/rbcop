begin
  values.reduce(false) do |acc, el|
     el == acc
  end
rescue => e
  e
ensure
  z
end

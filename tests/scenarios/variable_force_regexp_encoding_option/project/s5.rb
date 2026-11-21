def f(v)
  v =~ /(?<a>x)/s
  q = 1
  /#{v}(?<b>x)/s =~ v
end

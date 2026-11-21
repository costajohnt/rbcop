def f(v)
  if /(?<a>x)/s =~ v
    p $1, a
  end
end

def f(v)
  if /(?<OS>Sol)(?<major>\d)/ =~ v
    major
  end
end

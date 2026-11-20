def f(v)
  /(?<OS>Sol)(?<b>x)/ =~ v
  /(?'OS'a)(?<c>x)/ =~ v
  [b, c, OS]
end

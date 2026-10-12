if /(?<n>a)/ =~ x && n
  1
end
if foo && /(?<m>a)/ =~ x
  2
end
if /(?<k>a)/ =~ x
  3
end
unless !(/(?<j>a)/ =~ x)
  4
end
while foo && /(?<w>a)/ =~ x
  5
end
if foo && (y = /(?<v>a)/ =~ x)
  6
end

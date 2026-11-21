if res =~ /(.*)\=[aa][a]%c(?:a+)+[A-z]](?<n>x)(y)/e
  p $1
end
if x =~ /#{y}(.*)\=/me
  p $1
end
if x =~ %r{(.)\=}iex
  p $1
end
case
when /(?<a>x)/e =~ z then 1
when /(?<b>x)/e =~ z then 2
end

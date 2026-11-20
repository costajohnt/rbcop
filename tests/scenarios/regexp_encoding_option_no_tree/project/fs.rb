if res =~ /(.*)\=[aa][a]%c(?:a+)+[A-z]](?<n>x)(y)/s
  p $1
end
if x =~ /#{y}(.*)\=/ms
  p $1
end
if x =~ %r{(.)\=}isx
  p $1
end
case
when /(?<a>x)/s =~ z then 1
when /(?<b>x)/s =~ z then 2
end

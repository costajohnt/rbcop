if res =~ /(.*)\=[aa][a]%c(?:a+)+[A-z]](?<n>x)(y)/u
  p $1
end
if x =~ /#{y}(.*)\=/mu
  p $1
end
if x =~ %r{(.)\=}iux
  p $1
end
case
when /(?<a>x)/u =~ z then 1
when /(?<b>x)/u =~ z then 2
end

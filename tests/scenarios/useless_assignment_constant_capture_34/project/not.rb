def f(v)
  /#{v}(?<OS>Sol)/ =~ v
  /(?<OS>Sol)/.=~(v)
  /(?<OS>Sol) # (?<X>y)
  /x =~ v
end

items.each do |t|
  rank = t.rank_for(x) or next
  puts rank
end
value = foo(1) || bar

begin
  foo
rescue (logger).error
  bar
end
begin
  foo
rescue a((b))
  bar
end
begin
  foo
rescue (a).b(&c)
  bar
end
begin
  foo
rescue *(a)
  bar
end
begin
  foo
rescue Foo, (a).b
  bar
end
begin
  foo
rescue (Foo), (a)
  bar
end
begin
  foo
rescue Foo, *(a), (b)
  bar
end
begin
  foo
rescue [(a)].first
  (bar)
  baz
end
begin
  foo
rescue (a) => e
  (e).bar
end

begin
  foo
rescue
  x ? a : (b ? c : (foo(e)))
end

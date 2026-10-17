def m
  x
rescue Foo => e
  (logger).error { e.message }
end
def n
  x
rescue Foo
  (logger).error { 1 }
end
begin
  x
rescue => e
  (logger).error { e }
end
def o
  x
rescue Foo => e
  (logger).error(e)
end
def p
  x
rescue Foo => e
  (logger).error { _1 }
end
def q
  x
rescue Foo => e
  (logger).error do it end
end
def r
  x
rescue Foo => e
  (logger).error(&e)
end

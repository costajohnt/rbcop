def bar
  do_something
rescue => e
  raise
ensure
  log(e)
end

def baz
  do_something
rescue => e
  raise
ensure
  log(x:)
end

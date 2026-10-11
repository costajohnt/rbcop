def foo
  do_something
rescue => e
  raise
ensure
  log(e:)
end

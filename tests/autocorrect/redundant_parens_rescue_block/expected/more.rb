x rescue logger.error { 1 }
x rescue (logger).error(1)
def m
  x
rescue Foo => e
  logger.error { e }.foo
end
def n
  x
rescue Foo => e
  y(logger.error { e })
end
def o
  x
rescue Foo => e
  (logger.error { e })
end
def q
  x
rescue Foo => e
  logger.error { e }
end
def r
  x
rescue Foo => e
  y = 1
  logger.error { e }
end
[1].reduce(0) do |acc, el|
  next acc if el.zero?

  el.update(acc) { 1 }
end
[1].reduce(0) do |acc, el|
  next acc if el.zero?

  el.merge(acc) { _1 }.compact
end
[1].reduce(0) do |acc, el|
  next acc if el.zero?

  foo(el, acc) { 1 }
end
[1].reduce(0) do |acc, el|
  next acc if el.zero?

  el.update(@x) { 1 }
end
[1].inject(0) do |acc, el|
  next acc if el.zero?

  el.foo(bar) do
    1
  end
end
# Long line so an offense remains: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

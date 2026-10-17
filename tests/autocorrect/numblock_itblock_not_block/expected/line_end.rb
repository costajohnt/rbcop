foo do
  "expected #{it} to be " \
  "x"
end
foo do
  "expected #{_1} to be " \
  "x"
end
foo do |a|
  "expected #{a} to be " \
    "x"
end
foo do
  "expected #{it} to be " \
    "x"
  1
end
foo -> {
  "expected to be " \
    "x"
}
# Long line so an offense remains: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

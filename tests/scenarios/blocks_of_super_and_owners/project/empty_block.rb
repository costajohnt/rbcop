foo {}.to bar do
end
foo {}.bar {}.baz {}
super {}.to bar do
end
x = -> {}
foo(-> {}) {}
foo {}.to(bar) { _1 }

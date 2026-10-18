# frozen_string_literal: true

case x
in { "ab": }
  ab
in { "a_1": }
  a_1
in { a_1:, b: }
  a_1 + b
in { "true": }
  self
in { "foo_bar":, "b": }
  foo_bar + b
in { 'cd': }
  cd
in Foo("ef":)
  ef
in { "g": { "h": } }
  h
in { "ab": 1 }
  1
in { "ab": Integer => z }
  z
end

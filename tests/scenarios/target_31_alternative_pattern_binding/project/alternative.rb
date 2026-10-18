# frozen_string_literal: true

case x
in a | 1
  1
end

case y
in {a:} | [b]
  2
end

z in 1 | c

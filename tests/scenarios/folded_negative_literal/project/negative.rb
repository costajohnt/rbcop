# frozen_string_literal: true

def sort(input)
  - 1
end

def compare(a)
  return - 1 if a.nil?
  x = - 1.5
  y = [- 1, - 2r, - 3i, - 1e5]
  z = { a: - 1 }
  w = a > - 1 ? - 1 : + 1
  v = - 1 <=> a
  u = (- 1) + a
  t = - 1.0 == a
  case a
  when - 1 then 1
  end
  foo(- 1)
  bar - 1
  [x, y, z, w, v, u, t]
end

def literal_cmp
  - 1 == - 1
end

def unparenthesized
  if - 1 < 0
    1
  end
  - 1 .abs
  -1
  -(1)
end

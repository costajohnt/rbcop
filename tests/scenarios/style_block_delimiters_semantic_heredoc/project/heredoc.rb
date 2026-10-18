# frozen_string_literal: true

def f
  a(<<~SQL).map { |r| r }
    x
  SQL
end

def g
  <<~SQL.lines.map { |r| r }
    x
  SQL
end

def h
  a([<<~SQL.squish, 1]).map { |r| r }
    x
  SQL
end

def i
  a(<<~SQL).map { |r| r }
    x
  SQL
  1
end

def j
  a.map { |r| r + <<~X }
    y
  X
end

def k
  a.map { |r| r }.join(<<~X)
    y
  X
end

# frozen_string_literal: true

def f(keys)
  keys.reduce(nil) do |merged, buffer|
    next buffer if !merged

    buffer.each_with_object(merged) { |x, result| result[x] = 1 }
  end
end

def g(keys)
  keys.reduce(nil) do |merged, buffer|
    next buffer if !merged

    buffer.each_with_object(merged) do |x, result|
      result[x] = 1
    end
  end
end

def h(keys)
  keys.reduce(nil) do |merged, buffer|
    next buffer if !merged

    buffer.each_with_object(merged) { it[0] = 1 }
  end
end

def i(keys)
  keys.reduce(nil) do |merged, buffer|
    next buffer if !merged

    buffer.concat(merged) do 1 end
  end
end

def j(keys)
  keys.reduce(nil) do |merged, buffer|
    next buffer if !merged

    x = buffer.each_with_object(merged) { _1 }
    x
  end
end

def k(keys)
  keys.reduce(nil) do |merged, buffer|
    next buffer if !merged

    foo(buffer, merged) { 1 }
  end
end

def l(keys)
  keys.reduce(nil) do |merged, buffer|
    next buffer if !merged

    buffer&.concat(merged) { 1 }
  end
end

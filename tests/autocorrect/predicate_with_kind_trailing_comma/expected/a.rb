a.all?(Symbol)
a.all?(Symbol)
a.any?(String)
a.all? { |k, v| k.is_a?(Symbol) }
a.all? { |(k)| k.is_a?(Symbol) }
a.all? { |k, *| k.is_a?(Symbol) }

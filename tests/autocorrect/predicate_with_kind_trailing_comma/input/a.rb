a.all? { |k| k.is_a?(Symbol) }
a.all? { |k,| k.is_a?(Symbol) }
a.any? {|k,| k.kind_of?(String)}
a.all? { |k, v| k.is_a?(Symbol) }
a.all? { |(k)| k.is_a?(Symbol) }
a.all? { |k, *| k.is_a?(Symbol) }

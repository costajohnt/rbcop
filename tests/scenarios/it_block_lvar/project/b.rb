x.each { eval(code, binding, __FILE__, it) }
x.each { y.class == it }
x.each { it.first(it).uniq }
x.each { [1, 2].include?(it.size) }
x.each { puts it % 1 }
x.each { format(it, 1) }
x.map { it if it.foo }.compact
x.map { next if it.foo; it }.compact
x.each { foo(y: it) }
x.each { |y| foo(y: y) }
x.each { it.reject { |e| e.blank? } }
x.each { it.reject { _1.blank? } }
x.each { it.reject(&:blank?) }
x.each { it.select { |k, v| v.present? } }
x.each { it.each_with_object({}) { |e, h| h[e] = e } }
x.each { it.map { |k, v| [k, v.to_s] }.to_h }
x.each { it.to_h { |k, v| [k.to_s, v] } }
x.each { it.transform_values { |v| v } }
x.each { it.map { |k, v| [k, v] }.to_h }
x.each { it.to_h { [it, it.to_s] } }
x.each { it.to_h { [it.to_s, it] } }
x.each { it.each_with_object({}) { |(k, v), h| h[k] = it } }
x.each { Hash[it.map { |k, v| [k.to_sym, v] }] }
def m(&block) = x.each { foo(&block) }
def n
  x.each do
    return unless it
    foo
  end
end
def o
  x.each { self.a = it }
end
x.each { it.foo = it.foo }
x.each { y = it; y = y }
x.map { it.select(&:a).first }.compact
x.map { it.to_h { [_1, 1] } }
x.to_h { [it, 1] }
x.to_h { [1, it] }
x.map { [it, 1] }.to_h
x.each_with_object({}) { |e, h| h[e] = it }
x.to_h { [it.to_sym, it] }
x.to_h { [it, it.to_sym] }
x.map { [it.to_sym, it] }.to_h
x.each { it.x ||= 1 }
x.each { it.x += 1 }
x.each { it[0] }
x.each { it.y.z }
x.map { it[:a] }
x.map { it.foo(1) }
x.select { it.even? }.map { it * 2 }
x.select { it =~ /a/ }
x.reject { it.nil? }
x.find_all { it.zero? }
x.sort_by { it }
x.max_by { it }
x.sum { it }
x.map { it.to_s }
x.flat_map { it }
x.group_by { it }
x.sort { it }

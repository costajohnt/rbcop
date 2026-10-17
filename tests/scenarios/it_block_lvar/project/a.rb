x.each { it + 0 }
x.each { Set[it, it] }
x.each { foo((it)) }
x.each { (it) ? 1 : 2 }
x.each { puts "#{it}" }
x.each { puts format('%s', it) }
x.each { eval(it) }
x.each { it.class == Foo }
x.each { it.class.name == 'Foo' }
x.map { next it if it.foo }.compact
x.each { [1, 2].include?(it) }
x.each { it.map(&:x).compact.uniq }
x.reject { it.blank? }
x.each { it.errors[:name] << 'x' }
x.each { it.errors.keys }
x.each { OpenSSL::Cipher::AES.new(128, it) }
x.each { it.each_with_object([]) { |e, a| a << e } }
x.each { it == 1 || it == 2 }
x.each { y(it: it) }
x.each { it.select { it.foo }.first }
x.each { it.map { |e| e }.flatten }
x.each { it.sort.first }
x.each { it.gsub('a', 'b') }
x.each { it.to_s.to_s }
x.each { it&.foo&.bar }
x.each { it.nil? ? nil : it.foo }
x.each { it ? it : 1 }
x.each { it.present? ? it : nil }
x.each { it.blank? ? nil : it }
x.each { return it if it }
x.each { it.where(a: 1).first }
x.each { it.pluck(:a) }
x.each { it.map { it[:a] } }
x.each { it.select { it > 1 }.map { it * 2 } }
x.each { it.select { it =~ /a/ } }
x.each { it.reject { it.nil? } }
x.each { it.inject(0) { |s, e| s + e } }
x.each { it.each_with_index.map { |e, i| e * i } }
x.each { puts(it.to_s) unless it.nil? }
x.each { if it then it end }
x.each { foo(it) if it.present? }
x.each { it.present? && it.foo }
x.each { it && it.foo }
x.each { it.foo if it }
x.each { it = 1 if false; it }
x.each { [it].flatten }
x.each { it.start_with?('a') || it.start_with?('b') }
x.each { it.is_a?(A) || it.is_a?(B) }
x.each { it.length.zero? }
x.each { it.size > 0 }
x.each { it.count > 0 }
x.each { it.keys.each { it } }
x.each { it.casecmp('a').zero? }
x.each { it.downcase == 'a' }
x.each { Time.now - it }
x.each { it.freeze }
x.each { it.dup.freeze }
x.each { "a" + it }
x.each { it + "a" }
x.each { it.to_sym.to_proc }
x.each { it.method(:x) }
x.each { send(it) }
x.each { it.send(:x) }
x.each { raise it }
x.each { it.try(:a) }
x.each { it.try!(:a) }
x.each { it.delegate :a }
x.each { it.first(1) }
x.each { [it, 1].max }
x.each { it.include?(1) ? 1 : 2 }

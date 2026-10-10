# frozen_string_literal: true

foo {}
puts 1 if (x = 1)
puts x
l = -> { nil }
puts File.open(%q(a)).read
puts File.exist?(%q(/dev/null))
puts l

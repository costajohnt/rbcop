before_validation(proc { it.history << 1 })
before_validation(proc { _1.history << 1 })
before_validation proc { |m| m.history << 1 }
foo(Proc.new { it })
foo lambda { it.x }
foo lambda { _1.x }
foo -> { it.x }
foo -> { _1.x }
foo(Proc.new { _1 })

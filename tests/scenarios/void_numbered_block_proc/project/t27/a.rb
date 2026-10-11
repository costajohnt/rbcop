Proc.new { _1 }
Proc.new { it.method }
::Proc.new { _1.method }
proc { _1 }
proc { |x| x }
Proc.new { |x| x.method }
lambda { _1 }
lambda { it }
Proc.new
Proc.new &blk
Proc.new(1) { |x| x }
lambda(1) { |x| x }
proc { |x; y| x }
proc { }
Proc.new { }
Proc&.new { |x| x }
x = 1

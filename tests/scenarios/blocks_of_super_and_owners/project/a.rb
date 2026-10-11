def m(a)
  super(a) { _1 }
  super { _1 }
  foo { _1 }
  super { xxxxxx }
  super() { xxxxxx }
  super(a) { xxxxxx }
  super do xxxxxx end
  super { |x| xxxxxx }
end

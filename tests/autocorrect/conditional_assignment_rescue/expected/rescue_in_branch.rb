def m(sig, rc, meth)
  if sig
    definition = \
    sig
  elsif meth
    definition = \
    "arg"
  else
    obj = if rc
    begin
    rc.foo(meth)
    rescue NameError
    raise
    rescue ArgumentError, TypeError => e
    e
        else
    3
        ensure
    4
        end
      end
    definition = \
    obj
  end
  definition
end

def n(a)
  if a
    x = begin
    1
    rescue
    2
        end
  else
    x = 3
  end
  x
end

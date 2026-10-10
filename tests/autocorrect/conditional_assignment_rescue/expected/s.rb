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
        end
      end
    definition = \
    obj
  end
  definition
end

def m(sig, rc, meth)
  definition = \
    if sig
      sig
    elsif meth
      "arg"
    else
      obj = if rc
        begin
          rc.foo(meth)
        rescue NameError
          raise
        end
      end
      obj
    end
  definition
end

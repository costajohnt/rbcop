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
        rescue ArgumentError, TypeError => e
          e
        else
          3
        ensure
          4
        end
      end
      obj
    end
  definition
end

def n(a)
  x = if a
        begin
          1
        rescue
          2
        end
      else
        3
      end
  x
end

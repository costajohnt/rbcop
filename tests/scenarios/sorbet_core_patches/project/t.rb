class Foo
  A = T.let([1, 2], T::Array[Integer])
  B = T.let("x", String)
  C = ::T.let([1], T::Array[Integer])
  D = T.let([1], T::Array[Integer], 3)
  E = type_member
  F = type_template { { fixed: Integer } }
  G = T.type_alias { T.any(String, Integer) }
  H = ::T.type_alias { [1] }
  I = T.let(T.let([1], T::Array[Integer]), T::Array[Integer])
  J = [1, 2]
  K ||= T.let([3], T::Array[Integer])
end

def foo(x)
  case x
  when String
  when Integer
  else
    x #: absurd
  end
  case x
  when 1
    x #:absurd
  when 2
    x # absurd
  when 3
    x  #:  absurd   
  else
    x #: absurd!
  end
  y = 1
end

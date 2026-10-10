begin
  # Helps
  class A
    def x; end
  end
ensure
end
foo do
  # Doc
  class B
    def y; end
  end
  x
rescue
end
begin
  x
  # Doc
  class C
    def z; end
  end
rescue
end

def m
  Class&.new do
    def x; end
  end
end

class A
  private

  B = Module&.new
  C = Data&.define(:a)
end

X = Struct&.new(:a) do
  private

  def y; end
end

Y = Class&.new do
 private
  def z; end
end

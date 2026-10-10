module M
  include E
  private :a, :b
end

module N
  include E
  private
  def x; end
end

class C
  include E
  protected(&blk)
end

class D
  include E
  public
end

class F
  private :a
  include E
end

class G
  private do
    1
  end
end

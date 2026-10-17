class BarTest < ActiveSupport::TestCase
  def test_one
    refute File.exist?(path)
    refute x.match?(y)
    refute_equal nil, x
    refute_includes x, y
  end
end

class BarTest < ActiveSupport::TestCase
  def test_one
    refute_path_exists path
    assert_no_match x, y
    assert_not_nil x
    assert_not_includes x, y
  end
end

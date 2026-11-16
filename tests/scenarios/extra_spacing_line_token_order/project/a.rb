def test_to_date
  # 1 sec before midnight Jan 1 EST
  assert_equal Date.new(1999, 12, 31), ActiveSupport::TimeWithZone.new(Time.utc(2000, 1, 1, 4, 59, 59), ActiveSupport::TimeZone["Eastern Time (US & Canada)"]).to_date
  # midnight Jan 1 EST
  assert_equal Date.new(2000,  1,  1), ActiveSupport::TimeWithZone.new(Time.utc(2000, 1, 1, 5,  0,  0), ActiveSupport::TimeZone["Eastern Time (US & Canada)"]).to_date
  # 1 sec before midnight Jan 2 EST
  assert_equal Date.new(2000,  1,  1), ActiveSupport::TimeWithZone.new(Time.utc(2000, 1, 2, 4, 59, 59), ActiveSupport::TimeZone["Eastern Time (US & Canada)"]).to_date
  # midnight Jan 2 EST
  assert_equal Date.new(2000,  1,  2), ActiveSupport::TimeWithZone.new(Time.utc(2000, 1, 2, 5,  0,  0), ActiveSupport::TimeZone["Eastern Time (US & Canada)"]).to_date
end

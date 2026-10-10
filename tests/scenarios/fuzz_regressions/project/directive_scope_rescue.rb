def foo
  # rubocop:disable Layout/SpaceAroundOperators
  x =   0
  # rubocop:enable Layout/SpaceAroundOperators
rescue StandardError
  y =   1
end

def bar
  # rubocop:disable-next Layout/SpaceAroundOperators
  x =   0
  y =   2
ensure
  z =   3
end

[1].each do |i|
  # rubocop:disable Layout/SpaceAroundOperators
  i =   0
  # rubocop:enable Layout/SpaceAroundOperators
rescue
else
  w =   9
end

begin
  # rubocop:disable Layout/SpaceAroundOperators
  q =   1
  # rubocop:enable Layout/SpaceAroundOperators
end

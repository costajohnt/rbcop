# frozen_string_literal: true

begin
  x
rescue => e
  message = if e
    1
  else
    2
  end
end

begin
  x
rescue StandardError => e
  message =
    case e
    when A
      1
    else
      2
    end
end

begin
  x
rescue => e
  y
  message = if e
    1
  else
    2
  end
end

begin
  x
rescue
  y
else
  message = if z
    1
  else
    2
  end
ensure
  message = if z
    1
  else
    2
  end
end

def m
  x
rescue
  self.message = if z
    1
  else
    2
  end
end

message = (if z then 1 else 2 end) rescue nil
puts message

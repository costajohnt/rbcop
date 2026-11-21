# frozen_string_literal: true

begin
  x
rescue RuntimeError, ::Timeout::Error
  y
end

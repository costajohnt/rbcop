# frozen_string_literal: true

begin
  x
rescue RuntimeError, ArgumentError, StandardError
  y
end

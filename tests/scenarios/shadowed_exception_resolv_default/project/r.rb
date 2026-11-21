# frozen_string_literal: true

begin
  x
rescue StandardError, Resolv::ResolvError
  y
end

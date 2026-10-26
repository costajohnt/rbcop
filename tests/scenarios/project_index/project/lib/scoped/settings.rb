# frozen_string_literal: true

SETTING = 2

module Outer
  SETTING_TWO = 3

  # Resolves Base inside Outer.
  class Reader
    def run
      Base
      Simple
    end
  end
end

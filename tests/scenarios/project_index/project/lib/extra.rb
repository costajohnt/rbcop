# frozen_string_literal: true

class Base
  def get_name
    :again
  end
end

class Proxy
  def respond_to_missing?(name, include_private = false)
    super
  end
end

# Only class methods, but subclassed in utils.rb.
class Util
  def self.call
    1
  end
end

# Only class methods.
class Helpers
  def self.call
    2
  end
end

# Errors.
class AppError < Exception
end

# An error through AppError.
class SubError < AppError
end

module Outer
  # Shadows the top-level Base.
  class Base
  end

  # Not shadowed.
  class Thing
    def run
      Base
      ::Simple
      ::Base
    end
  end
end

class Outer::Inner
end

class Base::Nested
end

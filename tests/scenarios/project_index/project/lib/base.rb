# frozen_string_literal: true

# The base class.
class Base
  LIMIT = 1

  def initialize(value)
    @value = value
  end

  def get_name
    @value
  end

  def is_ready
    true
  end

  def self.build(one, two = nil)
    new(one || two)
  end

  # @deprecated Use {#fresh} instead
  def stale
    1
  end

  # @deprecated
  OLD = 1

  # @deprecated
  def older
    stale
  end

  def fresh
    used_helper
  end

  private

  def used_helper
    2
  end

  def unused_helper
    3
  end
end

# No initialize anywhere up the chain.
class Simple
  def value
    1
  end
end

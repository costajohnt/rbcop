# frozen_string_literal: true

class Child < Base
  LIMIT = 2

  def initialize(value)
    @child = value
  end

  def get_name
    super(1)
  end

  def is_ready
    false
  end

  def get_other
    1
  end

  def method_missing(name, *args)
    super
  end

  def run
    Base.build(1, 2, 3)
    Base.biuld(1)
    stale
    older
    Base::OLD
    Base::LIMTI
  end
end

# Plain subclass.
class Plain < Simple
  def initialize
    @plain = true
  end
end

# Another subclass, whose respond_to_missing? lives in extra.rb.
class Proxy < Simple
  def method_missing(name, *args)
    super
  end
end

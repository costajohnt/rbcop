# frozen_string_literal: true

class Foo < ApplicationRecord
  def m
    where()
  end

  def n
    Bar.or()
  end
end

# frozen_string_literal: true

class Foo < ApplicationRecord
  def m
    where(all)
  end

  def n
    Bar.or(all)
  end
end

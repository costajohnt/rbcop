# frozen_string_literal: true

class Foo < ApplicationRecord
  def m
    all.where(all)
  end

  def n
    Bar.all.or(all)
  end
end

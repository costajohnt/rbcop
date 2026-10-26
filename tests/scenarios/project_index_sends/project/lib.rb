# frozen_string_literal: true

# Foo
class Foo
  def self.[](a); end

  def self.[]=(a, b); end
end

# Bar
class Bar
  # @deprecated Use Foo.
  def self.[](a); end

  # @deprecated Use Foo.
  def self.[]=(a, b); end
end

# Baz
class Baz
  def self.fetch(a); end

  def self.[]=(a, b); end
end

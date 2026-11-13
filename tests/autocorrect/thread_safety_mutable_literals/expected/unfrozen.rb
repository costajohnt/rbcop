# frozen_string_literal: false

class Test
  @var = [1, 2, 3].freeze
  @var ||= [1, 2, 3].freeze
  var ||= [1, 2, 3]
  @var = %w(a b c).freeze
  @var ||= %w(a b c).freeze
  var ||= %w(a b c)
  @var = { a: 1, b: 2 }.freeze
  @var ||= { a: 1, b: 2 }.freeze
  var ||= { a: 1, b: 2 }
  @var = 'str'.freeze
  @var ||= 'str'.freeze
  var ||= 'str'
  @var = "#{30 + 12}ok".freeze
  @var ||= "#{30 + 12}ok".freeze
  var ||= "#{30 + 12}ok"
  @var = Something.new
  @var ||= Something.new
  var ||= Something.new
  @var = "#{a}".freeze
  @var ||= "#{a}".freeze
  var ||= "#{a}"
  @var = /re/.freeze
  @var ||= /re/.freeze
  var ||= /re/
  @var = (1..2).freeze
  @var ||= (1..2).freeze
  var ||= 1..2
  @var = [Queue.new].freeze
  @var ||= [Queue.new].freeze
  var ||= [Queue.new]
  @var = [ThreadSafe::Hash.new { false }].freeze
  @var ||= [ThreadSafe::Hash.new { false }].freeze
  var ||= [ThreadSafe::Hash.new { false }]
  @var = [Concurrent::ThreadSafe::Util::Adder.new].freeze
  @var ||= [Concurrent::ThreadSafe::Util::Adder.new].freeze
  var ||= [Concurrent::ThreadSafe::Util::Adder.new]
  @var = `ls`.freeze
  @var ||= `ls`.freeze
  var ||= `ls`
  @var = foo&.bar
  @var ||= foo&.bar
  var ||= foo&.bar
  @var = Foo.new { 1 }
  @var ||= Foo.new { 1 }
  var ||= Foo.new { 1 }
  @var = ClassA.new foo: 'bar'
  @var ||= ClassA.new foo: 'bar'
  var ||= ClassA.new foo: 'bar'
  @var = FOO + 'bar'
  @var ||= FOO + 'bar'
  var ||= FOO + 'bar'
  @var = 'foo' + 'bar' + 'baz'
  @var ||= 'foo' + 'bar' + 'baz'
  var ||= 'foo' + 'bar' + 'baz'
  @var = @a + @b + @c
  @var ||= @a + @b + @c
  var ||= @a + @b + @c
  @var = FOO % BAR
  @var ||= FOO % BAR
  var ||= FOO % BAR
  @var = FOO ** BAR
  @var ||= FOO ** BAR
  var ||= FOO ** BAR
  @var = -> { 1 }
  @var ||= -> { 1 }
  var ||= -> { 1 }
  @var = foo(1).bar
  @var ||= foo(1).bar
  var ||= foo(1).bar
  @var = x = []
  @var ||= x = []
  var ||= x = []
  @var = 1
  @var ||= 1
  var ||= 1
  @var = 2.1
  @var ||= 2.1
  var ||= 2.1
  @var = :sym
  @var ||= :sym
  var ||= :sym
  @var = CONST
  @var ||= CONST
  var ||= CONST
  @var = ::CONST
  @var ||= ::CONST
  var ||= ::CONST
  @var = Namespace::CONST
  @var ||= Namespace::CONST
  var ||= Namespace::CONST
  @var = FOO + BAR
  @var ||= FOO + BAR
  var ||= FOO + BAR
  @var = FOO - BAR
  @var ||= FOO - BAR
  var ||= FOO - BAR
  @var = 'foo' + BAR
  @var ||= 'foo' + BAR
  var ||= 'foo' + BAR
  @var = ENV['foo']
  @var ||= ENV['foo']
  var ||= ENV['foo']
  @var = ENV['foo'] || 'bar'
  @var ||= ENV['foo'] || 'bar'
  var ||= ENV['foo'] || 'bar'
  @var = [1, 2].freeze
  @var ||= [1, 2].freeze
  var ||= [1, 2].freeze
  @var = Struct.new
  @var ||= Struct.new
  var ||= Struct.new
  @var = ::Struct.new(:a, :b)
  @var ||= ::Struct.new(:a, :b)
  var ||= ::Struct.new(:a, :b)
  @var = Struct.new(:node) do
    def assignment?
      true
    end
  end
  @var ||= Struct.new(:node) do
    def assignment?
      true
    end
  end
  var ||= Struct.new(:node) do
    def assignment?
      true
    end
  end
  @var = Something.new.freeze
  @var ||= Something.new.freeze
  var ||= Something.new.freeze
  @var = Queue.new
  @var ||= Queue.new
  var ||= Queue.new
  @var = ::Queue.new
  @var ||= ::Queue.new
  var ||= ::Queue.new
  @var = ThreadSafe::Array.new
  @var ||= ThreadSafe::Array.new
  var ||= ThreadSafe::Array.new
  @var = ::ThreadSafe::Hash.new
  @var ||= ::ThreadSafe::Hash.new
  var ||= ::ThreadSafe::Hash.new
  @var = ThreadSafe::Hash.new { false }
  @var ||= ThreadSafe::Hash.new { false }
  var ||= ThreadSafe::Hash.new { false }
  @var = Concurrent::Array.new
  @var ||= Concurrent::Array.new
  var ||= Concurrent::Array.new
  @var = ::Concurrent::Map.new
  @var ||= ::Concurrent::Map.new
  var ||= ::Concurrent::Map.new
  @var = Concurrent::Map.new(initial_capacity: 4)
  @var ||= Concurrent::Map.new(initial_capacity: 4)
  var ||= Concurrent::Map.new(initial_capacity: 4)
  @var = Concurrent::Map.new do |h, key|
    h.fetch_or_store(key, Concurrent::Map.new)
  end
  @var ||= Concurrent::Map.new do |h, key|
    h.fetch_or_store(key, Concurrent::Map.new)
  end
  var ||= Concurrent::Map.new do |h, key|
    h.fetch_or_store(key, Concurrent::Map.new)
  end
  @var = Concurrent::ThreadPoolExecutor.new(options)
  @var ||= Concurrent::ThreadPoolExecutor.new(options)
  var ||= Concurrent::ThreadPoolExecutor.new(options)
  @var = Concurrent::ThreadSafe::Util::Adder.new
  @var ||= Concurrent::ThreadSafe::Util::Adder.new
  var ||= Concurrent::ThreadSafe::Util::Adder.new
  @var = FOO + 2
  @var ||= FOO + 2
  var ||= FOO + 2
  @var = 1 + 2
  @var ||= 1 + 2
  var ||= 1 + 2
  @var = FOO + 2.1
  @var ||= FOO + 2.1
  var ||= FOO + 2.1
  @var = 1.2 + 3.4
  @var ||= 1.2 + 3.4
  var ||= 1.2 + 3.4
  @var = FOO == BAR
  @var ||= FOO == BAR
  var ||= FOO == BAR
  @var = 'foo'.count
  @var ||= 'foo'.count
  var ||= 'foo'.count
  @var = 'foo'.count('f')
  @var ||= 'foo'.count('f')
  var ||= 'foo'.count('f')
  @var = [1, 2, 3].count { |n| n > 2 }
  @var ||= [1, 2, 3].count { |n| n > 2 }
  var ||= [1, 2, 3].count { |n| n > 2 }
  @var = 'foo'.length
  @var ||= 'foo'.length
  var ||= 'foo'.length
  @var = 'foo'.size
  @var ||= 'foo'.size
  var ||= 'foo'.size
  @var = nil
  @var ||= nil
  var ||= nil
  @var = true
  @var ||= true
  var ||= true
  @var = -1
  @var ||= -1
  var ||= -1
  @var = 1r
  @var ||= 1r
  var ||= 1r
  @var = 2i
  @var ||= 2i
  var ||= 2i
  @var = :"a#{b}"
  @var ||= :"a#{b}"
  var ||= :"a#{b}"
  @var = (1..2).freeze
  @var ||= (1..2).freeze
  var ||= (1..2)
  @var = (1)
  @var ||= (1)
  var ||= (1)
  @var = freeze
  @var ||= freeze
  var ||= freeze
  @var = 1 << 2
  @var ||= 1 << 2
  var ||= 1 << 2
  @var = Data.define(:a)
  @var ||= Data.define(:a)
  var ||= Data.define(:a)
  @var = Concurrent::A::B::C::D.new
  @var ||= Concurrent::A::B::C::D.new
  var ||= Concurrent::A::B::C::D.new
  @var = [1].count { _1 }
  @var ||= [1].count { _1 }
  var ||= [1].count { _1 }
  @var = Struct.new { _1 }
  @var ||= Struct.new { _1 }
  var ||= Struct.new { _1 }
  @@list = [1, 2]
  if something
    @var = [1, 2, 3].freeze
  end
  @var = (1..10).to_a.freeze
  @var = (1..10).to_a.freeze
  @var = [YYY, ZZZ].freeze
  @var = [@a, @b].freeze
  @var = %w(YYY ZZZ).freeze
  @var = (1..99).freeze
  @var = (1..99).freeze
  @var = (1...99).freeze
  @var = (1...99).freeze
  @var = (1..).freeze
  @a, @b = [1].freeze, 1
  @a, @b, @c = [1, { a: 1 }.freeze, [3].freeze]
  @a, _, @c = 1, [2].freeze, 'foo'.freeze
  @a, @b, @c = 'foo'.freeze, [2].freeze, 3
  @a, @b = 1
  @a, *@b = [1].freeze, [2], [3]
  *@a, @b = [1], [2].freeze
  @a, = [1].freeze, [2]
  @a, (@b, @c) = [1].freeze, [2]
  @a, @b = *x, [].freeze
  @var = <<~HERE.freeze
    content
  HERE
  @var = <<~HERE.freeze
    a
    b
  HERE
  @var = 'a' \
    'b'.freeze
  @var += [1]
  @var &&= [1]
  def some_method
    @var = [1]
  end
  def self.some_method
    @var = [1]
  end
  class << self
    @var = [1].freeze
    def x
      @var = [1]
    end
  end
  define_singleton_method(:name) do
    @var = [1, 2]
  end
  define_method(:name) do
    @var = [1, 2]
  end
  define_method(:name) { _1; @var = [1].freeze }
  foo do
    @var = [1].freeze
  end
  x = lambda { @var = [1].freeze }
  setup do
    @something = { a: "A" }.freeze
  end
  teardown do
    @something = { a: "A" }.freeze
  end
  set_up do
    @something = { a: "A" }.freeze
  end
end

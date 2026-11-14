# typed: strict
class Foo
  extend T::Sig

  DEFAULT_PATH = T.let(Pathname.new("/usr/local").freeze, Pathname)
  BUILT = T.let(::Foo::Bar.new { _1.enabled = true }, Foo::Bar)
  LICENSES = T.let(Set.new(["mit"]).freeze, T::Set[String])
  OTHER = T.let(Pathname.new("x"), String)
  QUERY = T.let(Query.new(<<~SQL), Query) # the query
    SELECT 1
  SQL
  MAX_RETRIES = T.let(3, Integer)
  GREETING = T.let("hello", String)
  RATE = T.let(1.5, Float)
  PATTERN = T.let(/foo/, Regexp)
  FROZEN_PATTERN = T.let(/foo/.freeze, Regexp)
  STATUS = T.let(:active, Symbol)
  SHELLS = T.let([:bash, :zsh].freeze, T::Array[Symbol])
  NAMES = T.let(["alice", "bob"], T::Array[String])
  WIDE = T.let(["alice", "bob"], T::Array[T.nilable(String)])
  TUPLES = T.let([[1, "one"]].freeze, T::Array[[Integer, String]])
  FROZEN_GREETING = T.let("hello".freeze, String)
  OPTIONS = T.let({ verbose: true }, T::Hash[Symbol, T::Boolean])
  HEREDOC = T.let(<<~TEXT, String)
    body
  TEXT
  RBS_GREETING = "hello" #: String
  RBS_NAMES = ["alice", "bob"] #: Array[String]
  RBS_FROZEN = [:a, :b].freeze #: Array[Symbol]
  RBS_TUPLE = [[1, "one"]].freeze #: Array[[Integer, String]]
  RBS_INT = 1 #: ::Integer
  RBS_WRONG = 1 #: String

  sig { params(a: Integer, b: T::Array[String], c: String).void }
  def initialize(a, *b, c:)
    @a = T.let(a, Integer)
    @b = T.let(b, T::Array[T::Array[String]])
    @c = T.let(c, T.nilable(String))
  end

  sig { returns(Foo) }
  def foo
    @foo = T.let(@foo, T.nilable(Foo))
    @foo ||= Foo.new
  end

  sig { returns(Foo) }
  def bar
    @bar = T.let(nil, T.nilable(Foo))
    @bar ||= compute_a_very_long_value_that_needs_wrapping(with_some_arguments, and_more_arguments_here, and_more)
  end

  foo do
    CONST = T.let(1, Integer)
  end
  (PAREN = T.let(1, Integer))
end

LITERAL = T.let(1, Integer)

# Differential-fuzz input generator: mutate corpus seed files into odd variants.
# usage: RBENV_VERSION=4.0.6 rbenv exec ruby scripts/fuzz/mutate.rb <out-dir> <count> <rng-seed> <corpus-dir>...
# Seeds are only read (lexed with Prism for token mutations), never executed.
# Writes <out-dir>/vNNNNN.rb plus <out-dir>/manifest.tsv (variant, seed file, mutations).
require 'prism'
require 'fileutils'

out, count, seed, *corpora = ARGV
abort 'usage: mutate.rb <out-dir> <count> <rng-seed> [corpus-dir...]' unless seed
if corpora.empty?
  base = ENV.fetch('RBCOP_CORPUS') { abort 'pass corpus dirs or set RBCOP_CORPUS' }
  corpora = %w[specs rails discourse cold].map { File.join(base, _1) }
end
RNG = Random.new(Integer(seed))
MAX_SEED_BYTES = 4000

seeds = corpora.flat_map { |c| Dir.glob(File.join(c, '**', '*.rb')) }
               .select { |f| File.file?(f) && File.size(f).between?(1, MAX_SEED_BYTES) }
               # Invalid UTF-8 seeds only exercise documented encoding gaps (docs/known-gaps.md).
               .select { |f| File.binread(f).force_encoding('UTF-8').valid_encoding? }
abort 'no seeds' if seeds.empty?

SNIPPETS = [
  "# comment\n", "#\n", "# rubocop:disable all\n", "# rubocop:enable all\n",
  "# rubocop:disable Style/StringLiterals\n", "# frozen_string_literal: true\n",
  "# encoding: utf-8\n", "=begin\nblock comment\n=end\n", "x = <<~EOS\n  foo \#{bar}\n  baz\nEOS\n",
  "foo(<<-A, <<~B)\n  a\n  A\n  b\nB\n", "__END__\n", "p(1)\n", "BEGIN { x }\n", "end\n", "begin\n",
  "  \t\n", "\n\n\n", "def é; end\n", "ñ = 1\n", "变量 = 2\nputs 变量\n", "x = 'ü'\n",
  "a ? b : c\n", "foo&.bar\n", "-> { _1 }\n", "[1, 2].map { it }\n", "case x\nin [a, *] then a\nend\n",
  "{ a:, b: }\n", "foo(...)\n", "def m(*, **, &) = bar(*, **, &)\n", "\"\\u{1F600}\"\n",
  "%w[a b]\n", "%i(a b)\n", "/(?<n>x)/ =~ s\n", "1r + 2i\n", "?a\n", ":\"sym\#{x}\"\n",
  "x = y rescue nil\n", "while true do break end\n", "if x then y end\n", "return\n", "redo\n",
  "class Foo < Bar; end\n", "module M; private; def x; end; end\n", "alias foo bar\n", "undef foo\n",
  "\f\n", "\v\n", "a = 1;;\n", ";\n", "\\\n", "`ls`\n", "$stdout.puts $1\n", "@@x ||= 1\n",
  "puts(<<~E.strip)\n  q\nE\n", "foo do |a, (b, c)|\nend\n", "x = *a, b\n", "a, b = b, a\n",
  "\"a\" \"b\"\n", "'a' \\\n  'b'\n", "{ \"a\": 1 }\n", "foo bar, baz do end\n", "not x\n", "!!x\n"
].freeze
NON_ASCII = %w[é ñ 变量 ü café π ö Ж].freeze
WRAPS = [
  ["begin\n", "end\n", 2], ["foo do\n", "end\n", 2], ["class Foo\n", "end\n", 2], ["module M\n", "end\n", 0],
  ["def m\n", "end\n", 2], ["if x\n", "end\n", 2], ["[1].each { |i|\n", "}\n", 2], ["x = (\n", ")\n", 0],
  ["-> do\n", "end\n", 4], ["begin\n", "rescue => e\n  e\nensure\n  z\nend\n", 2]
].freeze

def lines(s) = s.lines

def char_boundary(s, i)
  i -= 1 while i.positive? && i < s.bytesize && (s.getbyte(i) & 0xC0) == 0x80
  i
end

def tokens(s)
  Prism.lex(s).value.map(&:first).reject { |t| t.type == :EOF || t.location.length.zero? }
rescue StandardError
  []
end

def pick(a) = a[RNG.rand(a.size)]

def mutate(src)
  ls = lines(src)
  i = ls.empty? ? 0 : RNG.rand(ls.size)
  case RNG.rand(29)
  when 0
    ls.delete_at(i)
    ['delete_line', ls.join]
  when 1 then ['dup_line', ls.insert(i, ls[i] || "\n").join]
  when 2
    j = [i + 1, ls.size - 1].min
    ls[i], ls[j] = ls[j], ls[i] if ls[i] && ls[j]
    ['swap_lines', ls.join]
  when 3
    l = ls[i] or return ['noop', src]
    d = RNG.rand(1..4)
    ls[i] = RNG.rand(2).zero? ? (' ' * d) + l : l.sub(/\A {1,#{d}}/, '')
    ['indent', ls.join]
  when 4 then ['insert_snippet', ls.insert(i, pick(SNIPPETS)).join]
  when 5 then ['insert_end_marker', ls.insert(i, "__END__\n").join]
  when 6 then ['bom', "\uFEFF#{src}"]
  when 7 then ['crlf_all', src.gsub(/\r?\n/, "\r\n")]
  when 8
    l = ls[i] or return ['noop', src]
    ls[i] = l.sub(/\n\z/, "\r\n")
    ['crlf_line', ls.join]
  when 9
    ['tabs', ls.map { |l| RNG.rand(3).zero? ? l.sub(/\A( {2})+/) { "\t" * (_1.size / 2) } : l }.join]
  when 10 then ['trailing_ws', ls.map { |l| RNG.rand(3).zero? ? l.sub(/(\r?\n)?\z/) { "#{pick([' ', "\t", '  '])}#{_1}" } : l }.join]
  when 11
    toks = tokens(src).select { |t| t.type == :IDENTIFIER }
    return ['noop', src] if toks.empty?
    name = toks.sample(random: RNG).value
    to = pick(NON_ASCII) + (RNG.rand(2).zero? ? '' : name)
    ['non_ascii_ident', src.b.gsub(/\b#{Regexp.escape(name)}\b/n, to.b).force_encoding('UTF-8')]
  when 12
    pre, post, ind = pick(WRAPS)
    ['wrap', pre + ls.map { |l| l.strip.empty? ? l : (' ' * ind) + l }.join + (src.end_with?("\n") || src.empty? ? '' : "\n") + post]
  when 13
    cut = char_boundary(src, RNG.rand(src.bytesize + 1))
    ['truncate', src.byteslice(0, cut)]
  when 14
    toks = tokens(src)
    return ['noop', src] if toks.empty?
    t = toks.sample(random: RNG).location
    ['delete_token', src.byteslice(0, t.start_offset) + src.byteslice(t.end_offset..)]
  when 15
    toks = tokens(src)
    return ['noop', src] if toks.empty?
    t = toks.sample(random: RNG).location
    ['dup_token', src.byteslice(0, t.end_offset) + ' ' + src.byteslice(t.start_offset..)]
  when 16
    ends = ls.each_index.select { |k| ls[k] =~ /\A\s*end\b/ }
    return ['noop', src] if ends.empty?
    ls.delete_at(pick(ends))
    ['delete_end', ls.join]
  when 17
    l = ls[i] or return ['noop', src]
    ls[i] = l.sub(/\n\z/, ';')
    ['join_semicolon', ls.join]
  when 18
    l = ls[i] or return ['noop', src]
    ls[i] = l.chomp
    ['join_lines', ls.join]
  when 19 then ['blank_lines', ls.insert(i, "\n" * RNG.rand(1..3)).join]
  when 20 then ['strip_blank', ls.reject { |l| l.strip.empty? && RNG.rand(2).zero? }.join]
  when 21
    c = pick(["\r", "\0", "\v", "\f", "\t", "\u00A0", "\u3000", "\xFF".b])
    pos = char_boundary(src, RNG.rand(src.bytesize + 1))
    ['insert_char', (src.byteslice(0, pos).b + c.b + src.byteslice(pos..).b).force_encoding('UTF-8')]
  when 22 then ['no_final_newline', src.sub(/\s+\z/, '')]
  when 23
    toks = tokens(src).select { |t| %i[INTEGER STRING_CONTENT IDENTIFIER CONSTANT].include?(t.type) }
    return ['noop', src] if toks.empty?
    t = toks.sample(random: RNG).location
    rep = pick(['0', 'nil', 'x', 'Foo', '""', '[]', '{}', '(a)', 'a.b', '__method__', '1.0', 'é'])
    ['replace_token', src.byteslice(0, t.start_offset) + rep + src.byteslice(t.end_offset..)]
  when 24
    toks = tokens(src).select { |t| %i[PARENTHESIS_LEFT PARENTHESIS_RIGHT BRACE_LEFT BRACE_RIGHT KEYWORD_DO KEYWORD_END BRACKET_LEFT BRACKET_RIGHT COMMA].include?(t.type) }
    return ['noop', src] if toks.empty?
    t = toks.sample(random: RNG).location
    ['delete_bracket', src.byteslice(0, t.start_offset) + src.byteslice(t.end_offset..)]
  when 25
    l = ls[i] or return ['noop', src]
    ls[i] = l.sub(/\S/) { "#{_1} " }
    ['space_after_first', ls.join]
  when 26
    # A non-ASCII character next to punctuation (Ruby reads it as an identifier there).
    toks = tokens(src).reject { |t| %i[IDENTIFIER CONSTANT STRING_CONTENT COMMENT].include?(t.type) }
    return ['noop', src] if toks.empty?
    t = toks.sample(random: RNG).location
    at = RNG.rand(2).zero? ? t.start_offset : t.end_offset
    ['non_ascii_by_punct', src.byteslice(0, at) + pick(["　", " ", 'é', 'Ж']) + src.byteslice(at..)]
  else
    other = File.read(pick($seeds), mode: 'rb').force_encoding('UTF-8').lines
    ['foreign_line', ls.insert(i, other.empty? ? "\n" : pick(other)).join]
  end
end

$seeds = seeds
FileUtils.mkdir_p(out)
File.open(File.join(out, 'manifest.tsv'), 'w') do |m|
  Integer(count).times do |n|
    file = pick(seeds)
    src = File.read(file, mode: 'rb').force_encoding('UTF-8')
    applied = []
    RNG.rand(1..3).times do
      name, src = begin
        mutate(src)
      rescue ArgumentError, Encoding::CompatibilityError, RegexpError # invalid UTF-8 seed, non-ASCII name
        ['error', src]
      end
      applied << name
    end
    name = format('v%05d.rb', n)
    File.binwrite(File.join(out, name), src)
    m.puts [name, file, applied.join(',')].join("\t")
  end
end
puts "#{count} variants in #{out}"

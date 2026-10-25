# Generate src/encodings.bin: Ruby's transcoding of each source encoding a magic comment can name
# to UTF-8 (Parser::Source::Buffer.reencode_string), as RuboCop's Ruby (4.0.6) does it, for
# src/encoding.rs. Every character of each encoding is found by probing String#encode byte by byte.
# Tables cover the single-byte encodings and the Japanese multibyte ones (MULTIBYTE); the others
# get kind 0 (see docs/known-gaps.md).
# usage: ruby scripts/encodings.rb > src/encodings.bin
#
# Format (integers big-endian):
#   u16 alias count; per alias: u8 length, lowercase name, u8 encoding index
#   u8 encoding count; per encoding: u8 length, Encoding#name, u8 kind, then for kinds 1 and 2:
#   u32 entry count; per entry (sorted): u24 key (the bytes; a multibyte one starts above 0x7f),
#   u24 code point (0xFFFFFF: an undefined conversion). ASCII bytes mapping to themselves are left out.
# Kinds: 0 no table (inspected as before: binary, UTF-8, dummy, not tabled), 1 table whose undefined
# conversions read `X from E to UTF-8`, 2 one whose read `X to UTF-8 in conversion from E to UTF-8`,
# 3 no converter (`code converter not found (E to UTF-8)`).
MULTIBYTE = %w[Shift_JIS Windows-31J EUC-JP eucJP-ms CP51932].freeze

# String#encode returns an ASCII-only string as it is, so a lone ASCII byte goes through a Converter.
def probe(enc, bytes)
  str = (bytes.pack('C*') + "\n").force_encoding(enc)
  return str.encode('UTF-8') if bytes.any? { |b| b > 0x7f }

  ec = Encoding::Converter.new(enc, Encoding::UTF_8)
  ec.convert(str) + ec.finish
rescue EncodingError => e
  e
end

# [kind, {key => code point}] or nil when Ruby's behaviour is not a plain byte trie of characters.
def table(enc)
  return [3, {}] if probe(enc, [0x80]).is_a?(Encoding::ConverterNotFoundError)

  # ASCII bytes are left out when they map to themselves.

  entries = {}
  kind = nil
  todo = [[]]
  until todo.empty?
    prefix = todo.shift
    256.times do |b|
      seq = prefix + [b]
      r = probe(enc, seq)
      return nil if seq.size == 1 && b < 0x80 && !r.is_a?(String)

      case r
      when String
        chars = r.delete_suffix("\n").codepoints
        return nil unless chars.size == 1

        entries[seq] = chars[0] unless seq == chars && b < 0x80
      when Encoding::UndefinedConversionError
        dump = seq.pack('C*').dump
        k = [nil, "#{dump} from #{enc.name} to UTF-8", "#{dump} to UTF-8 in conversion from #{enc.name} to UTF-8"]
            .index(r.message)
        return nil unless k
        return nil if kind && kind != k

        kind = k
        entries[seq] = 0xFFFFFFFF
      when Encoding::InvalidByteSequenceError
        eb = r.error_bytes.bytes
        ra = r.readagain_bytes.to_s.bytes
        if eb == seq && ra == [0x0a]
          return nil if seq.size >= 3

          todo << seq
        elsif !(eb == prefix && ra == [b]) && !(prefix.empty? && eb == [b] && ra.empty?)
          return nil
        end
      else
        return nil
      end
      return nil if entries.size > 256 && !MULTIBYTE.include?(enc.name)
    end
  end
  [kind || 1, entries]
end

out = +''.b
names = Encoding.name_list.map(&:downcase).uniq.sort
encs = []
aliases = []
names.each do |n|
  e = Encoding.find(n)
  next unless e

  encs << e unless encs.include?(e)
  aliases << [n, encs.index(e)]
end
out << [aliases.size].pack('n')
aliases.each { |n, i| out << [n.bytesize].pack('C') << n << [i].pack('C') }
out << [encs.size].pack('C')
encs.each do |e|
  t = nil
  t = table(e) if e.ascii_compatible? && !e.dummy? && ![Encoding::UTF_8, Encoding::BINARY].include?(e)
  kind, entries = t || [0, {}]
  out << [e.name.bytesize].pack('C') << e.name << [kind].pack('C')
  warn "#{e.name}: kind #{kind}, #{entries.size} entries"
  next unless [1, 2].include?(kind)

  keys = entries.map { |seq, cp| [seq.inject(0) { |a, b| (a << 8) | b }, cp & 0xFFFFFF] }.sort
  out << [keys.size].pack('N')
  keys.each { |k, cp| out << [k, cp].pack('NN').bytes.values_at(1, 2, 3, 5, 6, 7).pack('C*') }
end
$stdout.binmode
$stdout.write(out)

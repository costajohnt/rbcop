# Regenerate src/helpers/alignment.rs WIDTHS (RBENV_VERSION=4.0.6 rbenv exec ruby scripts/display_widths.rb):
# ranges of code points whose Unicode::DisplayWidth.of(char) is not 1 (the installed gem), as
# rbcop's WIDTHS table rows, 5 per line.
require "unicode/display_width"
rows = []
cur = nil
(0..0x10FFFF).each do |u|
  next if (0xD800..0xDFFF).cover?(u)
  w = Unicode::DisplayWidth.of(u.chr(Encoding::UTF_8))
  if w != 1
    if cur && cur[1] == u - 1 && cur[2] == w
      cur[1] = u
    else
      rows << cur if cur
      cur = [u, u, w]
    end
  else
    rows << cur if cur
    cur = nil
  end
end
rows << cur if cur
warn "#{Unicode::DisplayWidth::VERSION} #{rows.size} rows"
rows.each_slice(5) { |s| puts "    " + s.map { |a, b, w| format("(0x%X, 0x%X, %d)", a, b, w) }.join(", ") + "," }

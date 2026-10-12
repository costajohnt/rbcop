# Loaded by `rubocop --require` when rbcop runs RuboCop for the cops it does not implement
# (RBCOP_FALLBACK=1, src/fallback.rs). One line per offense, tab-separated:
#   path  begin_byte  end_byte  severity  correctable  corrected  cop  message
# with `\`, tab and newline escaped in path and message. rbcop annotates messages itself (cop
# name, details, style guide URL), so RuboCop's annotation is turned off here.

RuboCop::Cop::MessageAnnotator.prepend(Module.new { def annotate(message) = message })

class RbcopFallbackFormatter < RuboCop::Formatter::BaseFormatter
  def file_finished(file, offenses)
    offenses.each do |o|
      # rbcop reports syntax errors itself (and Lint/Syntax cannot be excluded).
      next if o.cop_name == 'Lint/Syntax'

      loc = o.location
      if loc.is_a?(Parser::Source::Range)
        source = loc.source_buffer.source
        b = source[0, loc.begin_pos].bytesize
        e = source[0, loc.end_pos].bytesize
        global = 0
      else
        b = e = 0
        global = 1
      end
      output.puts [esc(file), b, e, o.severity.name, o.correctable? ? 1 : 0, o.corrected? ? 1 : 0, global,
                   o.cop_name, esc(o.message)].join("\t")
    end
  end

  private

  ESCAPES = { '\\' => '\\\\', "\t" => '\\t', "\n" => '\\n' }.freeze

  def esc(s) = s.gsub(/[\\\t\n]/, ESCAPES)
end

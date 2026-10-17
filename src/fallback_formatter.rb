# Loaded by `rubocop --require` when rbcop runs RuboCop for the cops it does not implement
# (RBCOP_FALLBACK=1, src/fallback.rs). One line per offense, tab-separated:
#   path  begin_byte  end_byte  severity  correctable  corrected  cop  message
# with `\`, tab and newline escaped in path and message. rbcop annotates messages itself (cop
# name, details, style guide URL), so RuboCop's annotation is turned off here.

require 'set'
require 'stringio'

RuboCop::Cop::MessageAnnotator.prepend(Module.new { def annotate(message) = message })

# rbcop loads the same configuration and has printed its warnings, the pending-cops banner and the
# missing-department warnings for its own cops in directives already, so the child keeps them to
# itself. A config error still reaches stderr: it is raised, and printed after the config is
# loaded.
module RbcopQuiet
  def self.call
    err = $stderr
    $stderr = StringIO.new
    yield
  ensure
    $stderr = err
  end
end

RuboCop::ConfigStore.prepend(Module.new do
  %i[apply_options! options_config= force_default_config! for_dir].each do |m|
    define_method(m) { |*args| RbcopQuiet.call { super(*args) } }
  end
end)
RuboCop::PendingCopsReporter.singleton_class.prepend(Module.new { def warn_if_needed(_config) = nil })
# rbcop warns about bare directive names of the cops in its registry (RBCOP_REGISTRY: the ones it
# implements, a plugin's lib/ classes, a `require:`d file's cop classes).
RbcopQuiet::IMPLEMENTED = (ENV.fetch('RBCOP_REGISTRY', '').split(',') + ['Lint/Syntax'])
                          .to_set { |name| name.split('/').last }
RuboCop::Cop::Registry.prepend(Module.new do
  private def print_department_missing_warning(name, path)
    super unless RbcopQuiet::IMPLEMENTED.include?(name.strip)
  end
end)

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

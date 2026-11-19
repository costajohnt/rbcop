# Loaded by `rubocop --require` when rbcop runs RuboCop for the cops it does not implement
# (src/fallback.rs). One line per offense (and `rbcop-fallback-uncached<TAB>path` after a file a cop
# raised an error or a warning on), what the cache needs (see `finished`), then
# `rbcop-fallback-done`. Offenses are tab-separated:
#   path  begin_byte  end_byte  severity  correctable  corrected  global  cop  message  disabled  justification
#   corrections
# (justification empty when there is none; disabled offenses come only under --display-suppressed)
# with `\`, tab and newline escaped in path and message. rbcop annotates messages itself (cop
# name, details, style guide URL), so RuboCop's annotation is turned off here.

require 'set'
require 'stringio'

RuboCop::Cop::MessageAnnotator.prepend(Module.new { def annotate(message) = message })

# rbcop loads the same configuration and has printed its warnings, the pending-cops banner and the
# missing-department warnings for its own cops in directives already, so the child keeps them to
# itself. A config error still reaches stderr: it is raised, and printed after the config is
# loaded. What the Ruby code the config requires prints while it loads (Ruby's warning about a
# constant it redefines) reaches stderr, as from RuboCop: rbcop runs no Ruby (#804).
module RbcopQuiet
  ERR = $stderr

  def self.loud
    quiet = $stderr
    $stderr = ERR
    yield
  ensure
    $stderr = quiet
  end

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
RuboCop::FeatureLoader.prepend(Module.new { def load = RbcopQuiet.loud { super } })
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
  class << self
    # Whether a cop raised an error or a warning on the file just inspected (see below).
    attr_accessor :uncached
  end

  def file_finished(file, offenses)
    offenses.each do |o|
      # rbcop reports syntax errors itself (and Lint/Syntax cannot be excluded), except in the
      # files it leaves wholly to RuboCop.
      next if o.cop_name == 'Lint/Syntax' && !ENV['RBCOP_FALLBACK_SYNTAX']

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
                   o.cop_name, esc(o.message), o.disabled? ? 1 : 0, esc(o.justification.to_s),
                   corrections(o)].join("\t")
    end
    output.puts "rbcop-fallback-uncached\t#{esc(file)}" if self.class.uncached
    # A cop that forks (rubocop-require_tools) would flush a copy of anything still buffered.
    output.flush
  end

  # rbcop checks for this line: RuboCop ran to the end (Runner calls it from an ensure). Before
  # it, what rbcop's cache of these offenses depends on (#805): the Ruby files loaded (but this
  # one, which lives in a fresh directory each run), whether the registry has a cop rbcop's does
  # not, and whether a cop it may run has an external dependency (ResultCache#context_checksum).
  def finished(_inspected_files)
    here = [File.dirname(File.expand_path(__FILE__)), File.dirname(File.realpath(__FILE__))]
    $LOADED_FEATURES.each { |f| output.puts "rbcop-fallback-loaded\t#{esc(f)}" unless f.start_with?(*here) }
    known = ENV.fetch('RBCOP_REGISTRY', '').split(',').to_set
    cops = RuboCop::Cop::Registry.global.cops
    output.puts 'rbcop-fallback-unknown-cops' unless cops.all? { |c| known.include?(c.cop_name) }
    except = Array(options[:except])
    output.puts 'rbcop-fallback-external-dependency' if cops.any? do |c|
      !except.include?(c.cop_name) &&
        c.instance_method(:external_dependency_checksum).owner != RuboCop::Cop::Base
    end
    output.puts 'rbcop-fallback-done'
  end

  private

  # Offense#corrections as `begin_byte,end_byte,hex(replacement)` joined by `;` (empty when none).
  def corrections(offense)
    return '' unless offense.respond_to?(:corrections) && offense.location.is_a?(Parser::Source::Range)

    source = offense.location.source_buffer.source
    offense.corrections.map do |c|
      [source[0, c.begin_pos].bytesize, source[0, c.end_pos].bytesize, c.replacement.unpack1('H*')].join(',')
    end.join(';')
  end

  ESCAPES = { '\\' => '\\\\', "\t" => '\\t', "\n" => '\\n' }.freeze

  def esc(s) = s.gsub(/[\\\t\n]/, ESCAPES)
end

# rbcop keeps the child's offenses in its result cache (#805), except for a file on which a cop
# raised an error or a warning, as Runner#save_in_cache (process_file sees only that file's).
RuboCop::Runner.prepend(Module.new do
  private def process_file(file)
    super.tap { RbcopFallbackFormatter.uncached = errors.any? || warnings.any? }
  end
end)

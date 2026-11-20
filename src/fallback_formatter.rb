# Loaded by `rubocop --require` when rbcop runs RuboCop for the cops it does not implement
# (src/fallback.rs). One line per offense (and `rbcop-fallback-uncached<TAB>path` after a file a cop
# raised an error or a warning on), whether a cop has an external dependency, then
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
    output.puts "rbcop-fallback-uncached\t#{esc(file)}" if RbcopFiles::UNCACHED.include?(file)
    # A cop that forks (rubocop-require_tools) would flush a copy of anything still buffered.
    output.flush
  end

  # With rbcop's result cache (#805, RBCOP_FALLBACK_HANDSHAKE: a directory), before any file is
  # inspected, where ResultCache.source_checksum would read them: the Ruby files loaded (but
  # this one, in a fresh directory each run) go to `loaded` there, a line to stderr says so, and
  # rbcop answers on fd 3 with the files it has offenses for, which are then skipped.
  def started(_target_files)
    dir = ENV.fetch('RBCOP_FALLBACK_HANDSHAKE', nil) or return

    here = [File.dirname(File.expand_path(__FILE__)), File.dirname(File.realpath(__FILE__))]
    File.write(File.join(dir, 'loaded'), $LOADED_FEATURES.reject { |f| f.start_with?(*here) }.join("\n"))
    STDERR.write("\x1Erbcop-fallback-loaded\n")
    STDERR.flush
    # A wrapper that closed fd 3 leaves nothing skipped (rbcop then takes the child's offenses).
    IO.new(3, 'r').each_line { |line| RbcopFiles::CACHED << File.expand_path(line.chomp) }
  rescue SystemCallError, ArgumentError
    nil
  end

  # rbcop checks for this line: RuboCop ran to the end (Runner calls it from an ensure). Before
  # it, whether a cop it may run has an external dependency (ResultCache#context_checksum),
  # which rbcop's cache cannot key on. Only loaded cop classes count: Registry#cops would load
  # every lazily registered cop (0.3 s), and a cop that was never loaded did not run.
  def finished(_inspected_files)
    except = Array(options[:except])
    output.puts 'rbcop-fallback-external-dependency' if ObjectSpace.each_object(Class).any? do |c|
      c < RuboCop::Cop::Base && c.name && !except.include?(c.cop_name) &&
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

# Which files the child inspects (#805). Most of its time goes to parsing, and a file no cop it
# runs may inspect has no offenses: such a file (Team#roundup_relevant_cops: enabled, not
# excluded_file?) is not parsed unless a directive in it may opt one of its cops in, nor is a
# file rbcop has cached offenses for (CACHED). The others are inspected, and rbcop keeps their
# offenses in its result cache, except for a file on which a cop raised an error or a warning,
# as Runner#save_in_cache: Runner#file_problems reports those before file_finished, also when the
# files are inspected in parallel worker processes.
module RbcopFiles
  CACHED = Set.new
  UNCACHED = Set.new
  # Any mode (CommentConfig#opted_in_by: enable, push, next, ...).
  DIRECTIVE = /(?:rubocop|standard)\s*:\s*[\w-]+.*/
end

RuboCop::Runner.prepend(Module.new do
  private def process_file(file)
    # Runner#file_offense_cache: with RuboCop's cache on (rbcop's here) the standby team is built
    # for every file's config, inspected or not.
    standby_team(@config_store.for_file(file)) if ENV['RBCOP_FALLBACK_HANDSHAKE']
    return [] if RbcopFiles::CACHED.include?(File.expand_path(file))
    return [] unless ENV['RBCOP_FALLBACK_SYNTAX'] || rbcop_inspects?(file)

    super
  end

  private def file_problems(file, errors, warnings)
    RbcopFiles::UNCACHED << file
    super
  end

  private def rbcop_inspects?(file)
    config = @config_store.for_file(file)
    # Runner#mobilize_team's own memo: the team RuboCop builds to inspect files of this config,
    # built now rather than when the first of them is parsed (no constructor runs more often).
    @inspection_team_by_config ||= {}.compare_by_identity
    team = @inspection_team_by_config[config] ||= assemble_team(config)
    # Asked within RuboCop's per-file cycle (Team#investigate_with_corrector): be_ready gives the
    # file fresh cop instances (the investigation then uses them), and a file not inspected
    # leaves the team not ready, as that method's ensure does, so the next file gets fresh ones.
    team.send(:be_ready)
    registry = mobilized_cop_classes(config)
    relevant = team.cops.any? do |cop|
      cop.cop_name != 'Lint/Syntax' && registry.enabled?(cop, config) && !cop.excluded_file?(file)
    rescue StandardError
      true
    end
    return true if relevant || rbcop_opted_in?(file)

    team.instance_variable_set(:@ready, false)
    false
  end

  # A directive in the file may opt one of the child's cops in (any mode, naming the cop or its
  # department).
  private def rbcop_opted_in?(file)
    lines = File.binread(file).scan(RbcopFiles::DIRECTIVE)
    return false if lines.empty?

    @rbcop_names ||= RuboCop::Cop::Registry.global.names - Array(@options[:except]) - ['Lint/Syntax']
    @rbcop_names.any? do |name|
      department, _, cop = name.rpartition('/')
      lines.any? { |l| l.include?(cop) || (!department.empty? && l.include?(department)) }
    end
  rescue SystemCallError
    true
  end
end)

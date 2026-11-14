# The spec-corpus extractor for plugins tested with Minitest (rubocop-minitest, rubocop-sorbet): runs
# the gem's own cop tests (test/rubocop/cop/<dept>/**/*_test.rb) and records every source an assert_offense /
# assert_no_offenses / inspect_source parses (annotations removed, templates and loops resolved by
# running them), grouped by the cop config and TargetRubyVersion each test uses. Sources that
# assert_correction re-parses are not recorded (the recorded `-A` output covers them).
#
# usage: ruby scripts/extract_minitest_corpus.rb <gem-src> <out-dir> <cops-file> <default.yml>
# Output: as scripts/extract_cold_corpus.rb: <out>/<Dept>_<Cop>/<variant>/{.rubocop.yml, files};
# variant = r<ruby>[-c<n>]. A file is test/NNN_test.rb, or test/NNN/<path> when the test names one,
# so the department's Include (`**/test/**/*`) matches it.
require "yaml"
require "fileutils"
require "tmpdir"

src, out_root, cops_file, _default_yml = ARGV
src = File.expand_path(src) if src
abort "usage: #{$0} <gem-src> <out-dir> <cops-file> <default.yml>" unless _default_yml

cops = File.readlines(cops_file).map { |l| l.split.first }.compact
plugin = ENV.fetch("PLUGINS", "rubocop-minitest").split(",").first
# The installed gem (its lib is the tag's), so integrate_plugins! finds it in Gem.loaded_specs.
gem plugin, ENV.fetch("PLUGIN_VERSION", ">= 0")
require plugin
# rubocop-minitest's AssertOffense, which rubocop-sorbet's tests use too.
require "rubocop/minitest/support"
if plugin == "rubocop-minitest"
  # The gem's test_helper minus minitest/autorun, pride and proveit (not needed to record sources).
  $LOADED_FEATURES << File.join(src, "test/test_helper.rb")
else
  # rubocop-sorbet's test_helper defines the cop_config(config) its tests build cops with and
  # injects the gem's default.yml; load it with minitest/autorun, debug and mocha stubbed out
  # (tests that stub with mocha fail after recording what they parsed).
  stubs = File.join(Dir.mktmpdir, "stubs")
  %w[minitest/autorun debug/prelude mocha/minitest].each do |f|
    FileUtils.mkdir_p(File.dirname(File.join(stubs, f)))
    File.write(File.join(stubs, "#{f}.rb"), "")
  end
  $LOAD_PATH.unshift(stubs, File.join(src, "test"))
  at_exit { FileUtils.rm_rf(File.dirname(stubs)) }
  require "test_helper"
  # Its cop_config needs a target_cop, which only some tests define; rubocop-minitest 0.41's
  # parse_source! calls cop_config for every test, so the others get the cop they test.
  Minitest::Test.prepend(Module.new do
    private

    def target_cop = @cop&.class || cop_class || super
  end)
end

RECORDED = Hash.new { |h, k| h[k] = [] }

module Record
  private

  def parse_source!(source, file = nil)
    unless caller_locations.any? { |l| l.label.end_with?("assert_correction") }
      path = file.respond_to?(:path) ? File.basename(file.path) : file
      key = [target_ruby_version.to_s, recorded_config, other_cops.dup]
      RECORDED[cop.class.cop_name] << [key, path, source.dup]
    end
    super
  end

  def recorded_config
    return cop_config.dup if method(:cop_config).arity.zero?

    # rubocop-sorbet builds its cops itself (`Cop.new(cop_config(...))`, its test_helper's): the
    # settings that differ from the defaults.
    default = RuboCop::ConfigLoader.default_configuration.for_badge(cop.class.badge)
    cop.cop_config.reject { |k, v| %w[Enabled AutoCorrect].include?(k) || default[k] == v }
  end
end
RuboCop::Minitest::AssertOffense.prepend(Record)

# Every test of the departments (a test file is not always named after its cop); sources are keyed
# by the cop under test, and only the listed cops are written.
cops.map { |c| c.split("/").first.downcase }.uniq.each do |dept|
  Dir[File.join(src, "test/rubocop/cop", dept, "**/*_test.rb")].sort.each { |t| require t }
end
Minitest.run(["--seed", "1"])

total = 0
cops.each do |cop|
  snips = RECORDED[cop]
  variants = snips.group_by(&:first)
  configs = variants.keys.map { |k| k.drop(1) }.uniq
  variants.each do |(ruby, config, others), list|
    n = configs.index([config, others])
    tag = n.zero? && config.empty? && others.empty? ? "" : "-c#{n}"
    dir = File.join(out_root, cop.tr("/", "_"), "r#{ruby}#{tag}")
    FileUtils.mkdir_p(dir)
    yml = { "plugins" => ENV.fetch("PLUGINS", "rubocop-minitest").split(","),
            "AllCops" => { "EnabledByDefault" => true, "NewCops" => "enable", "SuggestExtensions" => false,
                           "TargetRubyVersion" => ruby.to_f } }
    yml[cop] = config unless config.empty?
    File.write(File.join(dir, ".rubocop.yml"), yml.merge(others).to_yaml)
    list.uniq { |_, f, s| [f, s] }.each_with_index do |(_, f, s), i|
      num = format("%03d", i)
      path = File.join(dir, f ? File.join("test", num, f.delete_prefix("/")) : File.join("test", "#{num}_test.rb"))
      FileUtils.mkdir_p(File.dirname(path))
      File.write(path, s)
      total += 1
    end
  end
  warn "#{cop}: #{snips.size} snippets, #{variants.size} variants, 0 unresolved"
end
puts "#{total} files"

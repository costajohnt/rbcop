# Build the "cold" corpus: every expect_offense / expect_no_offenses / expect_correction snippet of
# the given cops' RuboCop specs, with `#{...}` interpolation, `%{key}` templates, `.each` loops,
# shared examples and `let`s resolved by a small static evaluator (the spec code is parsed with
# Prism, never executed), grouped by the TargetRubyVersion and cop_config each example runs with.
#
# usage: ruby scripts/extract_cold_corpus.rb <rubocop-src>/spec/rubocop/cop <out-dir> <cops-file> <default.yml>
#   <cops-file>: one `Dept/Cop` per line (extra columns ignored)
# Output: <out>/<Dept>_<Cop>/<variant>/{.rubocop.yml, NNN.rb | NNN/<filename>}; variant = r<ruby>[-c<n>].
# Untagged examples land in both r2.7 (RuboCop's spec default) and r3.4. Snippets the evaluator
# cannot resolve are listed on stderr; extend the evaluator for them.
require "prism"
require "yaml"
require "fileutils"

spec_root, out_root, cops_file, default_yml = ARGV
abort "usage: #{$0} <spec/rubocop/cop> <out-dir> <cops-file> <default.yml>" unless default_yml

DEFAULTS = YAML.safe_load_file(default_yml, permitted_classes: [Regexp, Symbol])
ANNOTATION = /\A\s*((?<!\\)\^+|\^{}) ?/
GROUPS = %i[describe context].freeze
EXAMPLES = %i[it specify example].freeze
EXPECT = %i[expect_offense expect_no_offenses expect_correction].freeze

LOOPS = {
  each: ->(c, _) { c }, each_key: ->(c, _) { c.map(&:first) }, each_value: ->(c, _) { c.map(&:last) },
  each_pair: ->(c, _) { c }, each_with_index: ->(c, _) { c.each_with_index.to_a },
  repeated_permutation: ->(c, a) { c.repeated_permutation(*a).to_a },
  permutation: ->(c, a) { c.permutation(*a).to_a }, combination: ->(c, a) { c.combination(*a).to_a },
  product: ->(c, a) { c.product(*a) }
}.freeze
# Stands in for an interpolation the evaluator cannot resolve; fine inside dropped annotation lines.
HOLE = "\u0000unresolved\u0000"

# SPEC=1 (tests/spec_corpus): untagged examples only at 2.7, the version RuboCop's specs default to,
# and no expect_correction snippets (the recorded `-A` output of the expect_offense source covers them).
SPEC_MODE = ENV["SPEC"] == "1"
UNTAGGED_RUBIES = SPEC_MODE ? ["2.7"] : ["2.7", "3.4"]

class Unresolved < StandardError; end

Ctx = Struct.new(:env, :lets, :shared, :ruby, :tagged, :rails) do
  def child
    Ctx.new(env.dup, lets.dup, shared.dup, ruby, tagged, rails)
  end
end

class Extractor
  attr_reader :snippets, :failures

  def initialize(cop, file)
    @cop = cop
    @file = file
    @snippets = []
    @failures = []
  end

  def run
    ctx = Ctx.new({}, {}, {}, nil, false, nil)
    # `RSpec.shared_examples` from spec/support, available to every spec.
    Dir[File.join(@file[%r{\A.*/spec/}], "support", "*.rb")].sort.each do |f|
      Prism.parse_file(f).value.statements.body.each { |n| prescan(n, ctx) }
    end
    walk(Prism.parse_file(@file).value.statements.body, ctx)
  end

  # For specs without expect_* examples (Lint/Syntax): every literal `let(:source)`.
  def harvest_sources(node = Prism.parse_file(@file).value)
    if node.is_a?(Prism::CallNode) && node.name == :let && node.arguments&.arguments&.first&.slice == ":source"
      stmts = body_of(node.block)
      text = begin
        stmts.size == 1 && value(stmts.first, nil)
      rescue Unresolved, NoMethodError
        nil
      end
      @snippets << { ruby: UNTAGGED_RUBIES.first, config: { @cop => {} }, filename: nil, text: text } if text.is_a?(String)
    end
    node.compact_child_nodes.each { |c| harvest_sources(c) }
  end

  private

  def walk(nodes, ctx)
    nodes.each { |n| prescan(n, ctx) }
    nodes.each { |n| visit(n, ctx) }
  end

  def body_of(block)
    block&.body.is_a?(Prism::StatementsNode) ? block.body.body : []
  end

  def prescan(node, ctx)
    return unless node.is_a?(Prism::CallNode) && (node.receiver.nil? || node.receiver.slice == "RSpec")

    case node.name
    when :let, :let!
      name = node.arguments&.arguments&.first
      ctx.lets[name.unescaped] = [node.block, ctx] if name.is_a?(Prism::SymbolNode)
    when :shared_examples, :shared_examples_for, :shared_context
      name = node.arguments.arguments.first
      ctx.shared[value(name, ctx)] = node.block
    end
  end

  def visit(node, ctx)
    case node
    when Prism::CallNode then visit_call(node, ctx)
    when Prism::LocalVariableWriteNode
      ctx.env[node.name.to_s] = begin
        value(node.value, ctx)
      rescue Unresolved => e
        e
      end
    end
  end

  def visit_call(node, ctx)
    if GROUPS.include?(node.name) || (EXAMPLES.include?(node.name) && node.receiver.nil?)
      return unless node.block

      c = ctx.child
      tags(node, c)
      walk(body_of(node.block), c)
    elsif %i[it_behaves_like include_examples include_context it_should_behave_like].include?(node.name)
      include_shared(node, ctx)
    elsif LOOPS.key?(node.name) && node.block.is_a?(Prism::BlockNode) && node.receiver
      each_loop(node, ctx)
    elsif EXPECT.include?(node.name) && node.receiver.nil?
      record(node, ctx)
    elsif node.block.is_a?(Prism::BlockNode) && !%i[let let! shared_examples shared_examples_for shared_context before after around subject].include?(node.name)
      walk(body_of(node.block), ctx.child)
    end
  end

  def tags(node, ctx)
    (node.arguments&.arguments || []).each do |a|
      next unless a.is_a?(Prism::SymbolNode)

      # rubocop-rails' `:railsNN` metadata: TargetRailsVersion N.N.
      ctx.rails = "#{$1}.#{$2}" if a.unescaped =~ /\Arails(\d)(\d)\z/
      next unless a.unescaped =~ /\Aruby(\d)(\d)\z/

      ctx.ruby = "#{$1}.#{$2}"
      ctx.tagged = true
    end
  end

  def include_shared(node, ctx)
    args = node.arguments.arguments
    name = value(args.first, ctx)
    block = ctx.shared[name] or return fail_at(node, "unknown shared example #{name.inspect}")
    c = ctx.child
    vals = args.drop(1).map { |a| value(a, c) }
    bind(block.parameters, vals, c)
    walk(body_of(node.block), c) if node.block
    walk(body_of(block), c)
  rescue Unresolved => e
    fail_at(node, e.message)
  end

  def each_loop(node, ctx)
    coll = value(node.receiver, ctx)
    args = (node.arguments&.arguments || []).map { |a| value(a, ctx) }
    coll = coll.to_a if coll.is_a?(Range) || coll.is_a?(Hash) || coll.is_a?(Set)
    raise Unresolved, "#{node.name} over #{coll.class}" unless coll.is_a?(Array)

    coll = LOOPS[node.name].call(coll, args)

    coll.each do |item|
      c = ctx.child
      params = node.block.parameters&.parameters
      names = param_names(params)
      vals = names.size > 1 && item.is_a?(Array) ? item : [item]
      bind(node.block.parameters, vals, c)
      walk(body_of(node.block), c)
    end
  rescue Unresolved => e
    fail_at(node, e.message)
  end

  def param_names(params)
    return [] unless params

    params.requireds.map { |p| p.respond_to?(:name) ? p.name.to_s : nil } +
      params.optionals.map { |p| p.name.to_s }
  end

  def bind(block_params, vals, ctx)
    params = block_params&.parameters
    return unless params

    params.requireds.each_with_index do |p, i|
      if p.is_a?(Prism::MultiTargetNode)
        p.lefts.each_with_index { |q, j| ctx.env[q.name.to_s] = vals[i]&.[](j) }
      else
        ctx.env[p.name.to_s] = vals[i]
      end
    end
    params.optionals.each_with_index do |p, i|
      idx = params.requireds.size + i
      ctx.env[p.name.to_s] = idx < vals.size ? vals[idx] : value(p.value, ctx)
    end
  end

  def record(node, ctx)
    return if SPEC_MODE && node.name == :expect_correction

    args = node.arguments&.arguments || []
    kw = {}
    pos = []
    args.each { |a| a.is_a?(Prism::KeywordHashNode) ? kw.merge!(value(a, ctx)) : pos << a }
    text = value(pos[0], ctx)
    raise Unresolved, "source is #{text.class}" unless text.is_a?(String)

    filename = pos[1] && value(pos[1], ctx)
    raise Unresolved, "unresolved interpolation in file name" if filename.is_a?(String) && filename.include?(HOLE)
    if node.name == :expect_offense
      kw.each do |k, v|
        next if k == :loop

        v = v.to_s
        text = text.gsub("%{#{k}}", v).gsub("^{#{k}}", "^" * v.size).gsub("_{#{k}}", " " * v.size)
      end
      text = text.lines.grep_v(ANNOTATION).join
    end
    return if text.strip.empty? && !SPEC_MODE # Lint/EmptyFile needs them
    raise Unresolved, "unresolved interpolation in source" if text.b.include?(HOLE.b)

    cop_config = let_value("cop_config", ctx) || {}
    other = let_value("other_cops", ctx) || {}
    # A config value that is not valid UTF-8 (`"\x82"`) cannot be written to .rubocop.yml.
    raise Unresolved, "invalid UTF-8 in cop_config" unless valid_utf8?([cop_config, other])
    ruby = ctx.ruby || (let_value("ruby_version", ctx)&.to_s)
    rubies = ruby ? [ruby] : UNTAGGED_RUBIES
    # rubocop-rails specs: `let(:rails_version)` (the 'with Rails N.N' contexts) and the
    # 'with SchemaLoader' context's `let(:schema)` (written to db/schema.rb).
    rails = ctx.rails || let_value("rails_version", ctx)&.to_s
    schema = let_value("schema", ctx) if ctx.lets.key?("schema")
    raise Unresolved, "schema is #{schema.class}" unless schema.nil? || schema.is_a?(String)

    # `let(:gem_versions)` stubs Config#gem_versions_in_target: written to a Gemfile.lock.
    gems = let_value("gem_versions", ctx) if ctx.lets.key?("gem_versions")
    raise Unresolved, "gem_versions is #{gems.class}" unless gems.nil? || gems.is_a?(Hash)

    rubies.each do |r|
      @snippets << { ruby: r, config: { @cop => cop_config }.merge(other), filename: filename, text: text,
                     rails: rails, schema: schema, gems: gems }
    end
  rescue Unresolved => e
    fail_at(node, e.message)
  end

  def valid_utf8?(v)
    case v
    when String then v.dup.force_encoding(Encoding::UTF_8).valid_encoding?
    when Array then v.all? { |e| valid_utf8?(e) }
    when Hash then v.all? { |k, e| valid_utf8?(k) && valid_utf8?(e) }
    else true
    end
  end

  def let_value(name, ctx)
    entry = ctx.lets[name] or return nil
    block, = entry
    stmts = body_of(block)
    raise Unresolved, "let(:#{name}) is empty" if stmts.empty?
    raise Unresolved, "let(:#{name}) has statements" if stmts.size > 1

    value(stmts.last, ctx)
  end

  # `described_class::NAME`: the literal value of that constant in the cop's own source.
  def cop_constant(name)
    lib = @file.sub(%r{/spec/rubocop/cop/}, "/lib/rubocop/cop/").sub(/_spec\.rb\z/, ".rb")
    queue = [Prism.parse_file(lib).value]
    until queue.empty?
      n = queue.shift
      return const_literal(n.value) if n.is_a?(Prism::ConstantWriteNode) && n.name.to_s == name

      queue.concat(n.compact_child_nodes)
    end
    raise Unresolved, "no constant #{name} in #{lib}"
  end

  def const_literal(node)
    if node.is_a?(Prism::CallNode) && node.name == :freeze
      const_literal(node.receiver)
    elsif node.is_a?(Prism::CallNode) && node.name == :[] && node.receiver&.slice == "Set"
      node.arguments.arguments.map { |a| value(a, nil) }
    else
      value(node, nil)
    end
  end

  def fail_at(node, msg)
    @failures << "#{@file}:#{node.location.start_line}: #{msg}"
    nil
  end

  def value(node, ctx)
    case node
    when Prism::StringNode, Prism::XStringNode then node.unescaped
    when Prism::SymbolNode then node.unescaped.to_sym
    when Prism::IntegerNode, Prism::FloatNode then node.value
    when Prism::RegularExpressionNode then Regexp.new(node.unescaped)
    when Prism::TrueNode then true
    when Prism::FalseNode then false
    when Prism::NilNode then nil
    when Prism::InterpolatedStringNode, Prism::InterpolatedSymbolNode
      s = node.parts.map do |p|
        case p
        when Prism::StringNode then p.unescaped
        when Prism::EmbeddedStatementsNode
          st = p.statements&.body || []
          raise Unresolved, "multi-statement interpolation" if st.size > 1

          begin
            st.empty? ? "" : value(st.first, ctx).to_s
          rescue Unresolved
            HOLE
          end
        else raise Unresolved, "interpolated #{p.class}"
        end
      end.join
      node.is_a?(Prism::InterpolatedSymbolNode) ? s.to_sym : s
    when Prism::ArrayNode then node.elements.map { |e| value(e, ctx) }
    when Prism::HashNode, Prism::KeywordHashNode
      node.elements.to_h do |a|
        raise Unresolved, "hash #{a.class}" unless a.is_a?(Prism::AssocNode)

        [value(a.key, ctx), value(a.value, ctx)]
      end
    when Prism::RangeNode
      a = value(node.left, ctx)
      b = value(node.right, ctx)
      node.exclude_end? ? (a...b) : (a..b)
    when Prism::ParenthesesNode
      value(node.body.body.last, ctx)
    when Prism::StatementsNode then value(node.body.last, ctx)
    when Prism::LocalVariableReadNode then lookup(node.name.to_s, ctx)
    when Prism::CallNode then call_value(node, ctx)
    when Prism::ConstantPathNode
      if node.slice == "Float::INFINITY"
        Float::INFINITY
      elsif node.parent&.slice == "described_class"
        cop_constant(node.name.to_s)
      else
        node.slice
      end
    when Prism::ConstantReadNode then node.slice
    when Prism::IfNode, Prism::UnlessNode
      truthy = value(node.predicate, ctx)
      truthy = !truthy if node.is_a?(Prism::UnlessNode)
      branch = truthy ? node.statements : (node.is_a?(Prism::IfNode) ? node.subsequent : node.else_clause)
      branch = branch.statements if branch.is_a?(Prism::ElseNode)
      branch ? value(branch, ctx) : nil
    else raise Unresolved, "cannot evaluate #{node.class}"
    end
  end

  def lookup(name, ctx)
    if ctx.env.key?(name)
      v = ctx.env[name]
      raise v if v.is_a?(Unresolved)

      return v
    end
    return let_value(name, ctx) if ctx.lets.key?(name)

    raise Unresolved, "unbound #{name}"
  end

  def call_value(node, ctx)
    if node.receiver.nil? && node.arguments.nil? && node.block.nil?
      return lookup(node.name.to_s, ctx)
    end

    recv = value(node.receiver, ctx) if node.receiver
    args = (node.arguments&.arguments || []).map { |a| value(a, ctx) }
    case node.name
    when :*, :+, :size, :length, :to_a, :to_s, :to_sym, :upcase, :downcase, :capitalize, :join,
         :sub, :gsub, :chomp, :strip, :first, :last, :flatten, :product, :zip, :keys, :values, :inspect,
         :freeze, :dup, :tr, :delete, :count, :reverse, :sort, :uniq, :lines, :chars, :split,
         :b, :==, :!=, :include?, :start_with?, :end_with?, :map, :to_h, :!, :-, :index, :match, :compact, :merge
      raise Unresolved, "#{node.name} on #{recv.class}" if recv.nil? && node.receiver

      blk = node.block
      if blk.is_a?(Prism::BlockArgumentNode) && blk.expression.is_a?(Prism::SymbolNode)
        recv.public_send(node.name, *args, &blk.expression.unescaped.to_sym)
      elsif blk
        raise Unresolved, "#{node.name} with a block"
      else
        recv.public_send(node.name, *args)
      end
    when :[]
      recv[*args]
    when :format
      format(*args)
    else raise Unresolved, "call #{node.name}"
    end
  rescue ArgumentError, NoMethodError, TypeError => e
    raise Unresolved, "#{node.name}: #{e.message}"
  end
end

def default_filename(cop, config, idx)
  include = config.dig(cop, "Include") || DEFAULTS.dig(cop, "Include")
  pat = include&.first
  return format("%03d.rb", idx) unless pat

  base = File.basename(pat)
  base.start_with?("*") ? format("%03d", idx) + base.delete_prefix("*") : File.join(format("%03d", idx), base)
end

# PLUGINS mode: a cop spec runs its source whatever the file name, but the recorded corpus runs
# RuboCop on real paths, so each file goes where the cop's first Include pattern matches.
def plugin_path(cop, config, idx, file)
  include = config.dig(cop, "Include") || DEFAULTS.dig(cop, "Include")
  file = file&.delete_prefix("/")
  num = format("%03d", idx)
  return File.join(num, file || "#{num}.rb") unless include

  flags = File::FNM_PATHNAME | File::FNM_EXTGLOB
  ok = ->(path) { include.any? { |p| File.fnmatch?(p, path, flags) } }
  if file
    # Keep the spec's name; a numbered directory keeps names unique where the pattern allows.
    found = [File.join(num, file), File.join(File.dirname(file), num, File.basename(file)), file].find(&ok)
    return found if found
  end
  pat = include.first.gsub("**/", "")
  base = File.basename(pat)
  return File.join(num, pat) unless base.include?("*")

  base = file ? File.basename(file) : base.sub("*", num).tr("*", "x")
  base += ".rb" if File.extname(base).empty?
  dir = File.dirname(pat)
  dir = dir == "." ? num : File.join(dir.tr("*", "x"), num)
  File.join(dir, base)
end

def snippet_path(cop, snip, idx)
  f = snip[:filename]
  return plugin_path(cop, snip[:config], idx, f) if ENV["PLUGINS"]
  return default_filename(cop, snip[:config], idx) unless f

  f = File.basename(f) if f.start_with?("/")
  File.join(format("%03d", idx), f)
end

def rubocop_yml(snip)
  all = { "AllCops" => { "EnabledByDefault" => true, "NewCops" => "enable", "SuggestExtensions" => false,
                         "TargetRubyVersion" => snip[:ruby].to_f } }
  # PLUGINS=gem[,gem]: a plugin's specs run with the plugin loaded.
  all = { "plugins" => ENV["PLUGINS"].split(",") }.merge(all) if ENV["PLUGINS"]
  all["AllCops"]["TargetRailsVersion"] = snip[:rails].to_f if snip[:rails]
  overrides = snip[:config].reject { |_, v| v.empty? }
  # `--only` refuses Lint/RedundantCopDisableDirective, so the spec corpus runs it alone this way.
  if SPEC_MODE && snip[:config].key?("Lint/RedundantCopDisableDirective")
    all["AllCops"] = all["AllCops"].except("EnabledByDefault").merge("DisabledByDefault" => true)
    overrides["Lint/RedundantCopDisableDirective"] = { "Enabled" => true }.merge(snip[:config]["Lint/RedundantCopDisableDirective"])
  end
  all.merge(overrides).to_yaml
end

cops = File.readlines(cops_file).map { |l| l.split.first }.compact
total = 0

# GemFilename's spec builds processed sources by file name instead of expect_offense.
if cops.include?("Bundler/GemFilename")
  names = %w[gems.rb gems.locked Gemfile Gemfile.lock].flat_map { |n| [n, "spec/#{n}"] }
  [[{}, ""], [{ "EnforcedStyle" => "gems.rb" }, "-c1"]].each do |cfg, tag|
    dir = File.join(out_root, "Bundler_GemFilename", "r3.4#{tag}")
    FileUtils.mkdir_p(File.join(dir, "spec"))
    File.write(File.join(dir, ".rubocop.yml"), rubocop_yml(ruby: "3.4", config: { "Bundler/GemFilename" => cfg }))
    names.each { |n| File.write(File.join(dir, n), "print 1\n") }
    total += names.size
  end
end
cops.each do |cop|
  dept, name = cop.split("/")
  snake = name.gsub(/([A-Z]+)([A-Z][a-z])/, '\1_\2').gsub(/([a-z\d])([A-Z])/, '\1_\2').downcase
  spec = File.join(spec_root, dept.downcase, "#{snake}_spec.rb")
  # Some cops split their spec (conditional_assignment_assign_in_condition_spec.rb, ...).
  specs = File.exist?(spec) ? [spec] : Dir[File.join(spec_root, dept.downcase, "#{snake}_*_spec.rb")].sort
  if specs.empty?
    warn "#{cop}: no spec #{spec}"
    next
  end
  exs = specs.map { |f| Extractor.new(cop, f).tap(&:run) }
  exs.each(&:harvest_sources) if SPEC_MODE && exs.all? { |e| e.snippets.empty? }
  failures = exs.flat_map(&:failures)
  snippets = exs.flat_map(&:snippets)
  failures.each { |f| warn "#{cop}: #{f}" }
  # Variant key: cop config, then (rubocop-rails) target Rails and db/schema.rb.
  variants = snippets.group_by { |s| [s[:ruby], [s[:config], s[:rails], s[:schema], s[:gems]]] }
  configs = variants.keys.map(&:last).uniq
  variants.each do |(ruby, config), snips|
    plain = config[0].values.all?(&:empty?) && config.drop(1).all?(&:nil?)
    tag = configs.index(config).zero? && plain ? "" : "-c#{configs.index(config)}"
    dir = File.join(out_root, cop.tr("/", "_"), "r#{ruby}#{tag}")
    FileUtils.mkdir_p(dir)
    File.write(File.join(dir, ".rubocop.yml"), rubocop_yml(snips.first))
    if snips.first[:schema]
      FileUtils.mkdir_p(File.join(dir, "db"))
      File.write(File.join(dir, "db", "schema.rb"), snips.first[:schema])
    end
    if (gems = snips.first[:gems])
      specs = gems.sort.map { |g, v| "    #{g} (#{v})\n" }.join
      File.write(File.join(dir, "Gemfile.lock"), "GEM\n  remote: https://rubygems.org/\n  specs:\n#{specs}\n" \
                                                 "PLATFORMS\n  ruby\n\nDEPENDENCIES\n#{gems.keys.sort.map { |g| "  #{g}\n" }.join}")
    end
    snips.uniq { |s| [s[:filename], s[:text]] }.each_with_index do |s, i|
      path = File.join(dir, snippet_path(cop, s, i))
      FileUtils.mkdir_p(File.dirname(path))
      File.write(path, s[:text])
      total += 1
    end
  end
  warn "#{cop}: #{snippets.size} snippets, #{variants.size} variants, #{failures.size} unresolved"
end
puts "#{total} files"

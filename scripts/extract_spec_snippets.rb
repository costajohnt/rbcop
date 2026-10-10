# Extract the Ruby snippets RuboCop's own cop specs lint into a corpus of plain files.
#
# usage: ruby scripts/extract_spec_snippets.rb <rubocop-src>/spec/rubocop/cop <out-dir>
#
# Every `expect_offense` / `expect_no_offenses` / `expect_correction` heredoc without
# interpolation becomes <out>/<department>/<cop>/<n>.rb, with offense annotation lines
# (`^^^ message`) removed. Real RuboCop is the oracle on these files, so expected offenses
# are not taken from the specs (whose cop_config contexts vary).
require "prism"
require "fileutils"

spec_root, out_root = ARGV
abort "usage: #{$0} <spec/rubocop/cop> <out-dir>" unless spec_root && out_root

METHODS = %i[expect_offense expect_no_offenses expect_correction].freeze
ANNOTATION = /\A\s*\^+(?:\{[^}]*\})?\s|\A\s*\^\{[^}]*\}|\A\s*\^\^\^/

count = 0
Dir.glob(File.join(spec_root, "*", "*_spec.rb")).sort.each do |spec|
  dept = File.basename(File.dirname(spec))
  cop = File.basename(spec, "_spec.rb")
  result = Prism.parse_file(spec)
  next unless result.success?

  snippets = []
  queue = [result.value]
  until queue.empty?
    node = queue.shift
    queue.concat(node.compact_child_nodes)
    next unless node.is_a?(Prism::CallNode) && METHODS.include?(node.name)

    arg = node.arguments&.arguments&.first
    next unless arg&.respond_to?(:opening_loc) && arg.opening_loc&.slice&.start_with?("<<")

    # Only literal heredocs: interpolated ones splice in spec variables. Prism splits dedented
    # `<<~` heredocs into string parts, so an InterpolatedStringNode of plain parts is literal too.
    text = case arg
           when Prism::StringNode then arg.unescaped
           when Prism::InterpolatedStringNode
             next unless arg.parts.all?(Prism::StringNode)
             arg.parts.map(&:unescaped).join
           else next
           end
    text = text.lines.reject { |l| l.match?(ANNOTATION) }.join if node.name == :expect_offense
    snippets << text unless text.strip.empty?
  end
  next if snippets.empty?

  dir = File.join(out_root, dept, cop)
  FileUtils.mkdir_p(dir)
  snippets.uniq.each_with_index do |text, i|
    File.write(File.join(dir, format("%03d.rb", i)), text)
    count += 1
  end
end
puts "#{count} snippets"

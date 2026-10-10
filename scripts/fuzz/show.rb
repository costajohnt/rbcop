# Show fuzz variants: their diff lines, manifest entry and source (each line inspected, so odd bytes show).
# usage: ruby scripts/fuzz/show.rb <base-dir> <batch/vNNNNN>... [--lines N]
args = ARGV.dup
max = 60
if (i = args.index('--lines'))
  max = Integer(args[i + 1])
  args.slice!(i, 2)
end
base, *specs = args
specs.each do |spec|
  dir = File.join(base, File.dirname(spec))
  name = File.basename(spec)
  name = "#{name}.rb" unless name.end_with?('.rb')
  puts "== #{spec}"
  %w[all.diff default.diff].each do |d|
    path = File.join(dir, d)
    next unless File.exist?(path)

    File.foreach(path).grep(%r{/#{Regexp.escape(name)}:}).each { puts "#{d}: #{_1.sub("#{dir}/", '')}" }
  end
  puts File.foreach(File.join(dir, 'manifest.tsv')).grep(/\A#{Regexp.escape(name)}\t/)
  File.binread(File.join(dir, name)).lines.first(max).each_with_index { |l, j| puts format('%3d %s', j + 1, l.inspect) }
end

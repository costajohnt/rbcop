# Differential -A check on fuzz variants: copy a sample into <dir>/ac/{rubocop,rbcop}, run
# `rubocop -A` and `rbcop -A` (default config) on each copy, diff the reports and the rewritten files.
# usage: RBENV_VERSION=4.0.6 rbenv exec ruby scripts/fuzz/ac.rb <variant-dir> [sample-size] [rng-seed]
require 'fileutils'
require 'open3'

dir = File.expand_path(ARGV[0])
n = Integer(ARGV[1] || 150)
rng = Random.new(Integer(ARGV[2] || 1))
root = File.expand_path('../..', __dir__)
ac = File.join(dir, 'ac')
FileUtils.rm_rf(ac)
files = Dir.glob(File.join(dir, '*.rb')).sort.sample(n, random: rng)
%w[rubocop rbcop].each do |side|
  FileUtils.mkdir_p(File.join(ac, side))
  File.write(File.join(ac, side, '.ruby-version'), "3.4\n")
  files.each { FileUtils.cp(_1, File.join(ac, side)) }
end
run = lambda do |side, cmd|
  out, err, = Open3.capture3(*cmd, chdir: File.join(ac, side), binmode: true)
  File.binwrite(File.join(ac, "#{side}.err"), err)
  out.force_encoding('UTF-8').scrub.lines.map { _1.sub("#{File.join(ac, side)}/", '') }.sort
end
a = run.('rubocop', %w[rubocop -A --force-default-config --cache false --format emacs .])
b = run.('rbcop', [File.join(root, 'target/release/rbcop'), '-A', '--force-default-config', '--format', 'emacs', '.'])
# Files RuboCop crashed on mid-correction are not comparable.
crashed = File.read(File.join(ac, 'rubocop.err')).scan(/An error occurred while \S+ cop was inspecting (?:.*\/)?([^\/]+?):\d+:\d+/).flatten.uniq
report = (a - b).map { "< #{_1}" } + (b - a).map { "> #{_1}" }
differing = files.map { File.basename(_1) }.reject do |f|
  File.binread(File.join(ac, 'rubocop', f)) == File.binread(File.join(ac, 'rbcop', f))
end
File.write(File.join(ac, 'report.diff'), report.join)
puts "ac: #{files.size} files, report diff #{report.size}, files differing #{differing.size} #{differing.first(20).join(' ')}"
puts "ac: rubocop crashed on #{crashed.uniq.join(' ')}" unless crashed.empty?
puts report.first(30)

# Delta-debug a fuzz variant: drop lines (then characters, for short files) while RuboCop and rbcop
# still disagree. The file is only parsed by both tools, never executed.
# usage: RBENV_VERSION=4.0.6 rbenv exec ruby scripts/fuzz/minimize.rb <file> <Cop/Name> [--all]
#        RBENV_VERSION=4.0.6 rbenv exec ruby scripts/fuzz/minimize.rb <file> --autocorrect
#   default: diff the cop's offenses (all-cops config, --only the cop; --all runs every cop).
#   --autocorrect: diff `-A --force-default-config` reports and rewritten files.
# Writes <file>.min next to the input and prints it.
require 'fileutils'
require 'open3'

file, cop, flag = ARGV
root = File.expand_path('../..', __dir__)
bin = File.join(root, 'target/release/rbcop')
cfg = File.join(root, 'config/all-cops.yml')
fmt = File.join(root, 'scripts/oracle_formatter.rb')
work = File.join(File.dirname(File.expand_path(file)), "min_work_#{Process.pid}")
autocorrect = cop == '--autocorrect'
only = flag == '--all' || autocorrect ? [] : ['--only', cop]

run = lambda do |side, src, cmd|
  dir = File.join(work, side)
  FileUtils.mkdir_p(dir)
  File.write(File.join(dir, '.ruby-version'), "3.4\n")
  File.binwrite(File.join(dir, 'probe.rb'), src)
  out, = Open3.capture3(*cmd, 'probe.rb', chdir: dir, binmode: true)
  lines = out.force_encoding('UTF-8').scrub.lines.map { _1.sub("#{dir}/", '') }
  lines = lines.select { _1.include?(" #{cop}: ") || _1.include?("] #{cop}: ") } unless autocorrect
  [lines.sort, autocorrect ? File.binread(File.join(dir, 'probe.rb')) : nil]
end
differs = lambda do |src|
  if autocorrect
    run.('a', src, %w[rubocop -A --force-default-config --cache false --format emacs]) !=
      run.('b', src, [bin, '-A', '--force-default-config', '--format', 'emacs'])
  else
    run.('a', src, ['rubocop', '--config', cfg, '--cache', 'false', '-r', fmt, '-f', 'RuboCop::Formatter::RbcopOracleFormatter', *only]) !=
      run.('b', src, [bin, '--config', cfg, '--format', 'rbcop-oracle', *only])
  end
end

src = File.binread(file)
abort 'no diff to minimize' unless differs.(src)
lines = src.lines
chunk = [lines.size / 2, 1].max
while chunk >= 1
  i = 0
  while i < lines.size
    cand = lines[0...i] + (lines[(i + chunk)..] || [])
    if !cand.empty? && differs.(cand.join)
      lines = cand
    else
      i += chunk
    end
  end
  chunk /= 2
end
s = lines.join.force_encoding('UTF-8')
if s.valid_encoding? && s.size < 150
  chars = s.chars
  i = 0
  while i < chars.size
    cand = chars[0...i] + chars[(i + 1)..]
    if differs.(cand.join)
      chars = cand
    else
      i += 1
    end
  end
  s = chars.join
end
FileUtils.rm_rf(work)
File.binwrite("#{file}.min", s)
puts s.lines.map(&:inspect)

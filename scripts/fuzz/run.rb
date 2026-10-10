# Differential-fuzz runner: compare rbcop with RuboCop on a directory of variants (scripts/fuzz/mutate.rb).
# usage: RBENV_VERSION=4.0.6 rbenv exec ruby scripts/fuzz/run.rb <variant-dir> [--no-default] [--no-timing]
# Two passes (rbcop-oracle format: range ends, scrubbed messages): config/all-cops.yml and --force-default-config.
# Writes <dir>/{all,default}.diff ("<" only RuboCop, ">" only rbcop), <dir>/errors.txt (rbcop cop errors
# RuboCop did not have, panics) and <dir>/slow.txt (files rbcop needed over 5 s for); prints buckets by cop.
require 'open3'
require 'timeout'

dir = File.expand_path(ARGV[0] || abort('usage: run.rb <dir>'))
root = File.expand_path('../..', __dir__)
bin = File.join(root, 'target/release/rbcop')
cfg = File.join(root, 'config/all-cops.yml')
fmt = File.join(root, 'scripts/oracle_formatter.rb')
File.write(File.join(dir, '.ruby-version'), "3.4\n") unless File.exist?(File.join(dir, '.ruby-version'))
File.write(File.join(dir, 'Gemfile'), "source 'https://rubygems.org'\n") unless File.exist?(File.join(dir, 'Gemfile'))

def run(cmd, dir)
  out, err, = Open3.capture3(*cmd, chdir: dir, binmode: true)
  [out.force_encoding('UTF-8').scrub, err.force_encoding('UTF-8').scrub]
end

CRASH = /^An error occurred while (\S+) cop was inspecting (.*):\d+:\d+\.$/
def crashes(err) = err.scan(CRASH).to_set { |cop, file| [cop, File.basename(file)] }
def cop_of(line) = line[/: [CWEFRI]: (?:\[Correctable\] )?([A-Za-z]+\/[A-Za-z0-9]+)/, 1] || line[/: [CWEFRI]: ([^:]+)/, 1]
def file_of(line) = line[/\A(.*?\.rb):\d+:/, 1]

def compare(name, dir, rubo, rb)
  rc = crashes(rubo[1])
  keep = ->(l) { !rc.include?([cop_of(l), File.basename(file_of(l).to_s)]) }
  a = rubo[0].lines.map(&:chomp).sort
  b = rb[0].lines.map(&:chomp).select(&keep).sort
  diff = (a - b).map { "< #{_1}" } + (b - a).map { "> #{_1}" }
  File.write(File.join(dir, "#{name}.diff"), diff.sort_by { [file_of(_1[2..]).to_s, _1] }.join("\n") + "\n")
  buckets = diff.group_by { |l| [l[0], cop_of(l)] }
  puts "#{name}: rubocop #{a.size} rbcop #{b.size} diff #{diff.size}"
  buckets.sort_by { -_2.size }.each do |(side, cop), ls|
    files = ls.map { file_of(_1[2..]) }.uniq
    puts format('  %s %-45s %4d lines %4d files  e.g. %s', side, cop, ls.size, files.size, File.basename(files.first.to_s))
  end
  # Cop errors: rbcop's that RuboCop does not share.
  errs = rb[1].lines.grep(/An error occurred|internal error|panicked|nested more than/).map(&:chomp)
  extra = errs.reject { |e| (m = e.match(CRASH)) && rc.include?([m[1], File.basename(m[2])]) }
  missing = rc.reject { |c| errs.any? { _1.include?(c[0]) && _1.include?(c[1]) } }
  File.open(File.join(dir, 'errors.txt'), 'a') do |f|
    extra.each { f.puts "#{name} rbcop: #{_1}" }
    missing.each { f.puts "#{name} rubocop-only crash: #{_1.join(' ')}" }
  end
  puts "  rbcop errors not in RuboCop: #{extra.size}; RuboCop crashes: #{rc.size}" if extra.any? || rc.any?
end

File.write(File.join(dir, 'errors.txt'), '')
cache = "#{dir}.cache"
rubo = run(['rubocop', '--config', cfg, '--cache', 'true', '--cache-root', cache, '--parallel', '-r', fmt,
            '-f', 'RuboCop::Formatter::RbcopOracleFormatter', '.'], dir)
rb = run([bin, '--config', cfg, '--format', 'rbcop-oracle', '.'], dir)
compare('all', dir, rubo, rb)
unless ARGV.include?('--no-default')
  rubo = run(['rubocop', '--force-default-config', '--cache', 'true', '--cache-root', cache, '--parallel',
              '-r', fmt, '-f', 'RuboCop::Formatter::RbcopOracleFormatter', '.'], dir)
  rb = run([bin, '--force-default-config', '--format', 'rbcop-oracle', '.'], dir)
  compare('default', dir, rubo, rb)
end

unless ARGV.include?('--no-timing')
  files = Dir.glob(File.join(dir, '*.rb')).sort
  slow = Queue.new
  q = Queue.new
  files.each { q << _1 }
  4.times.map do
    Thread.new do
      while (f = (q.pop(true) rescue nil))
        t = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        pid = Process.spawn(bin, '--config', cfg, '--format', 'emacs', f, out: File::NULL, err: File::NULL, chdir: dir)
        begin
          Timeout.timeout(10) { Process.wait(pid) }
        rescue Timeout::Error
          Process.kill('KILL', pid)
          Process.wait(pid)
        end
        dt = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t
        slow << format('%.1fs %s', dt, f) if dt > 5
      end
    end
  end.each(&:join)
  s = []
  s << slow.pop until slow.empty?
  File.write(File.join(dir, 'slow.txt'), s.join("\n"))
  puts "slow files (>5s): #{s.size}"
end

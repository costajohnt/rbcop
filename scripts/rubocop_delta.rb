# Inventory what changed between two RuboCop releases (docs/upgrading-rubocop.md), as TSV on stdout:
# one row per cop whose source, default config, release notes or corpus output changed.
# usage: ruby scripts/rubocop_delta.rb <old-gem-dir> <new-gem-dir> [notes.md] [old-oracle-dir new-oracle-dir]
#   gem dirs: e.g. ~/.rbenv/versions/4.0.6/lib/ruby/gems/4.0.0/gems/rubocop-1.91.0
#   notes.md: the new release's notes (`gh release view vX.Y.Z -R rubocop/rubocop --json body -q .body`),
#     several releases concatenated when skipping versions
#   oracle dirs: scripts/oracle.sh output of each version on the same corpora (<corpus>.txt and .crashes)
# Columns: cop, lib lines changed (diff -u +/- lines of the cop file), default.yml changed (yes/no),
#   release-note entries naming the cop, corpus offenses only in old, only in new, crash pairs fixed
#   (old crashed, new did not), crash pairs new. Non-cop lib files that changed go to stderr: those
#   are runner, CLI, formatter, config or mixin changes that need their own reading.
require "yaml"
require "open3"

old_gem, new_gem, notes, old_oracle, new_oracle = ARGV
abort "usage: #{$PROGRAM_NAME} <old-gem-dir> <new-gem-dir> [notes.md] [old-oracle-dir new-oracle-dir]" unless new_gem

def snake(name)
  name.gsub(/([A-Z]+)([A-Z][a-z])/, '\1_\2').gsub(/([a-z\d])([A-Z])/, '\1_\2').downcase
end

def cops(gem_dir)
  YAML.unsafe_load_file(File.join(gem_dir, "config/default.yml")).select { |k, v| k.include?("/") && v.is_a?(Hash) }
end

old_cfg = cops(old_gem)
new_cfg = cops(new_gem)
by_file = (old_cfg.keys | new_cfg.keys).to_h { |c| ["lib/rubocop/cop/#{c.split('/').map { |p| snake(p) }.join('/')}.rb", c] }
rows = Hash.new { |h, k| h[k] = Hash.new(0) }

(old_cfg.keys | new_cfg.keys).each { |c| rows[c][:config] = 1 if old_cfg[c] != new_cfg[c] }

files = Dir.chdir(old_gem) { Dir.glob("lib/**/*.rb") } | Dir.chdir(new_gem) { Dir.glob("lib/**/*.rb") }
files.sort.each do |f|
  a = File.join(old_gem, f)
  b = File.join(new_gem, f)
  out, = Open3.capture2("diff", "-u", File.exist?(a) ? a : "/dev/null", File.exist?(b) ? b : "/dev/null")
  n = out.lines.drop(2).count { |l| l.start_with?("+", "-") }
  next if n.zero?

  if by_file[f]
    rows[by_file[f]][:lines] = n
  else
    warn "#{File.exist?(a) ? (File.exist?(b) ? 'changed' : 'removed') : 'new'}\t#{n}\t#{f}"
  end
end

File.read(notes).scan(/^\* .*$/).each { |e| e.scan(%r{`([A-Z]\w+/\w+)`}).flatten.uniq.each { |c| rows[c][:notes] += 1 } } if notes

if new_oracle
  Dir.glob(File.join(old_oracle, "*.txt")).each do |o|
    n = File.join(new_oracle, File.basename(o))
    next unless File.exist?(n)

    # Both sides are LC_ALL=C sorted by scripts/oracle.sh, so comm streams them.
    Open3.popen2({ "LC_ALL" => "C" }, "comm", "-3", o, n) do |_, io|
      io.each_line do |l|
        cop = l[/: [CWEFRI]: (?:\[Correctable\] )?([A-Z]\w+\/\w+): /, 1] or next
        rows[cop][l.start_with?("\t") ? :added : :removed] += 1
      end
    end
    oc = File.exist?("#{o}.crashes") ? File.readlines("#{o}.crashes") : []
    nc = File.exist?("#{n}.crashes") ? File.readlines("#{n}.crashes") : []
    (oc - nc).each { |l| rows[l.split("\t").first][:fixed_crashes] += 1 }
    (nc - oc).each { |l| rows[l.split("\t").first][:new_crashes] += 1 }
  end
end

puts %w[cop lib_lines config notes only_old only_new crashes_fixed crashes_new].join("\t")
rows.sort.each do |cop, r|
  puts [cop, r[:lines], r[:config] == 1 ? "yes" : "no", r[:notes], r[:removed], r[:added], r[:fixed_crashes], r[:new_crashes]].join("\t")
end

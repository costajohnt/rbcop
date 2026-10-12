# Builds tests/spec_corpus (checked by tests/spec_corpus.rs): every cop's RuboCop spec examples,
# extracted by scripts/extract_cold_corpus.rb in SPEC=1 mode, with RuboCop's output recorded per
# cop and config variant. Run through scripts/record-spec-corpus.sh, which checks RuboCop runs.
#
# usage: ruby scripts/record_spec_corpus.rb <rubocop-src> <out-dir>
# env:   RUBOCOP (command, word-split; default `rubocop`), JOBS (default: CPU count),
#        PLUGIN=<gem>: <rubocop-src> is that plugin's source at the version in docs/plugins.md, and
#        its cops (from config/plugins/<gem>.yml) are recorded with `plugins: [<gem>]`
#
# Per variant (<Dept_Cop>/<variant>, one .rubocop.yml), two bundle files:
#   <variant>.input     the .rubocop.yml and the source files
#   <variant>.expected  `offenses exit=N`: `rubocop --only <cop> --format emacs .` (no --only for
#                       Lint/RedundantCopDisableDirective, whose .rubocop.yml enables only it);
#                       `autocorrect exit=N`: the same with -A, on a copy;
#                       `corrected/<path>`: each file -A changed, as rewritten
# A bundle is a list of sections: a header line `==> <name> <bytes> <==`, then exactly <bytes>
# bytes of content and a newline. Emacs paths are relative to the variant directory.
# <out-dir>/skip.txt lists files left out (see its header). Variants above MAX_FILES are capped and
# reported, as is the number of spec examples the extractor could not resolve.
require "fileutils"
require "open3"
require "shellwords"
require "tmpdir"
require "etc"

src, out = ARGV
abort "usage: #{$0} <rubocop-src> <out-dir>" unless out

root = File.expand_path("..", __dir__)
plugin = ENV["PLUGIN"]
default_yml = File.join(root, plugin ? "config/plugins/#{plugin}.yml" : "config/default.yml")
rubocop = Shellwords.split(ENV.fetch("RUBOCOP", "rubocop"))
jobs = Integer(ENV.fetch("JOBS", Etc.nprocessors))
# ponytail: per-variant cap on source files, first N by path (caps 5 variants of the 1.91 specs);
# lower it if the corpus outgrows the repo.
MAX_FILES = 400

skip = File.readlines(File.join(out, "skip.txt")).map { |l| l.sub(/#.*/, "").strip }.reject(&:empty?)
skipped = ->(rel) { skip.any? { |s| rel == s || rel.start_with?("#{s}/") } }

# `--only` refuses Lint/RedundantCopDisableDirective; its variants' .rubocop.yml enable only it.
def only_args(cop)
  cop == "Lint/RedundantCopDisableDirective" ? [] : ["--only", cop]
end

def bundle(sections)
  sections.map { |name, body| "==> #{name} #{body.bytesize} <==\n".b + body.b + "\n".b }.join.b
end

def files_under(dir)
  Dir.glob("**/*", File::FNM_DOTMATCH, base: dir).select { |f| File.file?(File.join(dir, f)) }.sort
end

Dir.mktmpdir("rbcop-spec-corpus") do |tmp|
  extracted = File.join(tmp, "extracted")
  cops = File.join(tmp, "cops.txt")
  File.write(cops, File.read(default_yml).scan(%r{^([A-Z]\w+/\w+):$}).join("\n"))
  log = File.join(tmp, "extract.log")
  ok = system({ "SPEC" => "1", "PLUGINS" => plugin }, RbConfig.ruby, File.join(root, "scripts/extract_cold_corpus.rb"),
              File.join(src, "spec/rubocop/cop"), extracted, cops, default_yml,
              out: File::NULL, err: log)
  abort "extractor failed, see #{log}:\n#{File.read(log)[-2000..]}" unless ok
  unresolved = File.readlines(log).count { |l| !l.include?(" snippets, ") }

  variants = Dir.glob("*/*", base: extracted).select { |v| File.file?(File.join(extracted, v, ".rubocop.yml")) }.sort
  variants.each do |v|
    files_under(File.join(extracted, v)).each { |f| File.delete(File.join(extracted, v, f)) if skipped.call("#{v}/#{f}") }
  end

  capped = []
  queue = Queue.new
  variants.each { |v| queue << v }
  results = {}
  lock = Mutex.new
  workers = Array.new(jobs) do
    Thread.new do
      while (v = (queue.pop(true) rescue nil))
        dir = File.join(extracted, v)
        files = files_under(dir) - [".rubocop.yml"]
        if files.size > MAX_FILES
          files.drop(MAX_FILES).each { |f| File.delete(File.join(dir, f)) }
          lock.synchronize { capped << "#{v}: #{files.size} -> #{MAX_FILES}" }
          files = files.take(MAX_FILES)
        end
        next if files.empty?

        cop = v.split("/").first.sub("_", "/")
        work = File.join(tmp, "ac", v)
        FileUtils.mkdir_p(File.dirname(work))
        FileUtils.cp_r(dir, work)
        run = lambda do |cwd, extra|
          o, _e, st = Open3.capture3(*rubocop, *extra, *only_args(cop), "--config", ".rubocop.yml", "--cache", "false",
                                     "--format", "emacs", ".", chdir: cwd)
          [o.gsub("#{File.realpath(cwd)}/", ""), st.exitstatus]
        end
        offenses, s1 = run.call(dir, [])
        report, s2 = run.call(work, ["-A"])
        input = [[".rubocop.yml", File.binread(File.join(dir, ".rubocop.yml"))]] +
                files.map { |f| [f, File.binread(File.join(dir, f))] }
        expected = [["offenses exit=#{s1}", offenses], ["autocorrect exit=#{s2}", report]]
        files.each do |f|
          after = File.binread(File.join(work, f))
          expected << ["corrected/#{f}", after] unless after == File.binread(File.join(dir, f))
        end
        lock.synchronize { results[v] = [bundle(input), bundle(expected)] }
      end
    end
  end
  workers.each(&:join)

  # Only this run's cops are replaced: RuboCop's own and each plugin's are recorded separately.
  recorded = File.read(cops).split.map { |c| c.tr("/", "_") }
  Dir.children(out).each { |c| FileUtils.rm_rf(File.join(out, c)) if recorded.include?(c) }
  results.sort.each do |v, (input, expected)|
    FileUtils.mkdir_p(File.join(out, File.dirname(v)))
    File.binwrite(File.join(out, "#{v}.input"), input)
    File.binwrite(File.join(out, "#{v}.expected"), expected)
  end
  puts "#{results.size} variants recorded in #{out}; #{unresolved} spec examples the extractor could not resolve"
  puts "capped: #{capped.sort.join(", ")}" unless capped.empty?
end

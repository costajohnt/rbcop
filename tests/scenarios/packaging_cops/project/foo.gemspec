require_relative "lib/foo/version"
Gem::Specification.new do |s|
  s.files = `git ls-files -z`.split("\x0")
  s.executables = `git ls-files -- bin/*`.split("\n")
end
::Gem::Specification.new do |spec, |
  spec.files = Dir.chdir(__dir__) { %x(git ls-files) }
end
Gem::Specification.new do |a, b|
  a.files = `git ls-files`
end
Gem::Specification.new("x") do |s|
  s.files = `git ls-files`
end
Gem::Specification.new do |s|
  s.files = `ls #{x}`
  s.test_files = `git ls-files -- test`
  s.other = `
git ls-files`
end
Gem::Specification.new { _1.files = `git ls-files` }

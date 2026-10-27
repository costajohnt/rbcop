# frozen_string_literal: true

# One gem per platform, each carrying its prebuilt binary in libexec/ (no Rust toolchain needed to
# install), plus a `ruby` platform gem with every binary in libexec/<target>/ for lockfiles that
# list only `ruby`. Built by scripts/dist/gem.sh, which sets the version and platform and stages
# the files.
Gem::Specification.new do |s|
  s.name = 'rbcop'
  s.version = ENV.fetch('RBCOP_GEM_VERSION')
  s.platform = ENV.fetch('RBCOP_GEM_PLATFORM')
  s.summary = 'RuboCop reimplemented in Rust, aiming for byte-identical output'
  s.description = 'A native reimplementation of RuboCop 1.92: same command line, configuration, ' \
                  'offenses, autocorrections and formatter output, run by the `rbcop` executable.'
  s.authors = ['John Costa']
  s.homepage = 'https://github.com/costajohnt/rbcop'
  s.license = 'MIT'
  s.metadata = { 'rubygems_mfa_required' => 'true', 'source_code_uri' => s.homepage }
  s.required_ruby_version = '>= 2.7'
  s.files = Dir['exe/rbcop', 'libexec/rbcop', 'libexec/*/rbcop', 'LICENSE', 'README.md', 'licenses/*']
  s.bindir = 'exe'
  s.executables = ['rbcop']
end

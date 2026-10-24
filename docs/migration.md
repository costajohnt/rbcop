# Switching from RuboCop

rbcop is RuboCop 1.91.0's command line reimplemented in Rust. It reads the same `.rubocop.yml`, takes the same options and prints the same offenses, so switching is mostly a matter of installing it and calling `rbcop` where you called `rubocop`. This guide covers the Gemfile, `bundle exec`, CI and editors.

Before switching, check two things:

- **Plugins and custom cops.** rbcop runs RuboCop's 613 core cops and the plugins listed in [plugins.md](plugins.md) (rubocop-performance, rubocop-rails, rubocop-minitest, rubocop-rspec, rubocop-rspec_rails, rubocop-factory_bot, rubocop-capybara). Cops from other plugins and custom cops loaded with `require:` are not run natively. rbcop names them on stderr, and `RBCOP_FALLBACK=1` runs them with RuboCop (see [Plugins and custom cops](#plugins-and-custom-cops) below). A project that uses only core cops needs nothing else.
- **Unsupported options.** `--auto-gen-config`, `--server`, `--lsp`, `-d` and a few others are not implemented; [known-gaps.md](known-gaps.md) lists them and every known difference.

## Gemfile

Add rbcop next to (or instead of) rubocop:

```ruby
group :development do
  gem "rbcop", require: false
end
```

and run `bundle install`. The gem ships a prebuilt binary, so no Rust toolchain is needed. There are gems for x86_64 and arm64 macOS and for x86_64 and aarch64 Linux (glibc and musl), plus a `ruby` platform gem carrying all of those binaries for lockfiles that list only `PLATFORMS ruby`. Windows is not supported yet.

If you replace the `gem "rubocop"` line in place, Bundler/OrderedGems may flag it: `rbcop` sorts before `rdoc` and `rspec`, where `rubocop` sorts after them. Move the line into alphabetical order (or run `bundle exec rbcop -a Gemfile`).

Keep `gem "rubocop"` and the plugin gems if you want the fallback for plugin cops, if other tools load RuboCop as a library (haml-lint, erb_lint, ruby-lsp's RuboCop add-on, standard), or simply to compare the two. Remove it if nothing else needs it.

## bundle exec

The executable is `rbcop`, with RuboCop's options:

```sh
bundle exec rbcop              # was: bundle exec rubocop
bundle exec rbcop -a app lib
bundle exec rbcop --format github
```

The gem deliberately does not install a `rubocop` executable. Most projects that use plugins keep the rubocop gem in the bundle (every plugin depends on it), and two gems installing the same executable name means whichever was installed last wins: `bundle exec rubocop` would silently switch between the two. Calling `rbcop` keeps the choice explicit.

If scripts call a binstub, change its last line. Rails' generated `bin/rubocop`:

```ruby
#!/usr/bin/env ruby
require "rubygems"
require "bundler/setup"

# explicit rubocop config increases performance slightly while avoiding config confusion.
ARGV.unshift("--config", File.expand_path("../.rubocop.yml", __dir__))

load Gem.bin_path("rbcop", "rbcop")   # was: load Gem.bin_path("rubocop", "rubocop")
```

`bin/rubocop` then runs rbcop, and every script and CI step that calls `bin/rubocop` keeps working.

A Rake task built on `RuboCop::RakeTask` needs RuboCop's Ruby API; replace it with a task that shells out:

```ruby
desc "Run rbcop"
task :rubocop do
  sh "bundle exec rbcop"
end
```

## Without Bundler

- Homebrew: a formula (`rbcop.rb`) is attached to each release.
- Release archives: `rbcop-<version>-<target>.tar.gz`, for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `{x86_64,aarch64}-unknown-linux-gnu` (glibc 2.17 or newer) and `{x86_64,aarch64}-unknown-linux-musl` (static). `scripts/dist/install.sh VERSION DIR` downloads the right one, checks it against the SHA-256 pinned in the repository (`packaging/checksums/VERSION.sha256`) and installs `DIR/rbcop`; it refuses an archive that is not listed or does not match.
- From source: `cargo build --release` (Rust 1.88 or newer).

## CI

rbcop has no result cache: every run inspects every file, using all cores. On a cold CI runner that is where it gains most, since RuboCop's cache is usually empty there.

### GitHub Actions

The action downloads the release archive (cached across runs), checks it against the checksum pinned in the action's own ref on every run, and runs rbcop. A drop-in workflow for a project with core cops only:

```yaml
name: Lint
on: [push, pull_request]
jobs:
  rbcop:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: costajohnt/rbcop@v0.1.0
```

By default it runs `rbcop --format github --format progress`: offenses become annotations on the pull request, and the log keeps the usual progress output. Inputs:

| Input | Default | |
|---|---|---|
| `args` | `--format github --format progress` | rbcop's arguments; empty only installs `rbcop` on `PATH` |
| `version` | the action's version | the rbcop release to run |
| `fallback` | `false` | run plugin and custom cops with RuboCop (`RBCOP_FALLBACK=1`) |
| `rubocop` | `bundle exec rubocop` | the RuboCop command the fallback runs |

With plugins, install the bundle first and turn on the fallback:

```yaml
      - uses: actions/checkout@v4
      - uses: ruby/setup-ruby@v1
        with:
          bundler-cache: true
      - uses: costajohnt/rbcop@v0.1.0
        with:
          fallback: true
```

If the project already runs `bundle exec rubocop` in a Ruby job, swapping that step for `bundle exec rbcop` (with the gem in the Gemfile) works as well.

### Other CI systems

Use the gem through Bundler (`bundle exec rbcop`), or `scripts/dist/install.sh` to fetch a binary.

## Plugins and custom cops

When the configuration enables cops rbcop cannot run, it prints one line on stderr, for example:

```
rbcop: not running 45 cops it does not implement (Sorbet: 41, ThreadSafety: 4) or cops from Ruby code the config requires; set RBCOP_FALLBACK=1 to run them with RuboCop
```

Cops from Ruby files the config requires (`require: ./lib/cops/foo.rb`) are named as "cops from Ruby code the config requires", since rbcop cannot know their names.

`RBCOP_FALLBACK=1` runs RuboCop once more, for exactly those cops (`--except` every cop rbcop implements), on the same files with the same configuration and options, and merges its offenses into rbcop's report: one report, one exit status. RuboCop has to be installed (`rubocop` on `PATH`, or set `RBCOP_RUBOCOP`, for example `RBCOP_RUBOCOP="bundle exec rubocop"`). The run is slower than core cops alone, since RuboCop still parses every file, but the core cops no longer run in Ruby. The README lists the fallback's limits.

## Editors

- Editors that run the `rubocop` executable with `--stdin` (Vim/ALE, Neovim's nvim-lint, Sublime Text's SublimeLinter-rubocop, Emacs flycheck) work with rbcop: point their executable setting at `rbcop` (or `bundle exec rbcop`, or `bin/rubocop` once the binstub runs rbcop). `--stdin` with `-a`/`-A` prints the corrected source as RuboCop does.
- Language-server integrations (`rubocop --lsp`, ruby-lsp's RuboCop add-on, which loads RuboCop as a library) are not supported: keep RuboCop for those.

## Switching back

Change `rbcop` back to `rubocop` in the places above. rbcop never writes configuration and reads RuboCop's files as they are.

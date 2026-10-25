# rbcop

A reimplementation of [RuboCop](https://github.com/rubocop/rubocop) 1.91.0 in Rust, built on [Prism](https://github.com/ruby/prism) (`ruby-prism` crate).

The target is byte-identical output: same offenses, ranges, messages, `[Correctable]` flags, autocorrections, formatter output and exit status as RuboCop 1.91.0 for the same files and configuration. All 613 cops in RuboCop's `config/default.yml` are implemented. rubocop-performance, rubocop-rails, rubocop-minitest, rubocop-rspec, rubocop-rspec_rails, rubocop-factory_bot and rubocop-capybara run natively ([docs/plugins.md](docs/plugins.md)). Other plugins (rubocop-sorbet and so on) and custom cops do not; rbcop says which cops it skips and can hand them to RuboCop (see [Plugins and custom cops](#plugins-and-custom-cops)).

## Install

- Gemfile: `gem "rbcop", require: false`, then `bundle exec rbcop` where you ran `bundle exec rubocop`. The gem carries a prebuilt binary (macOS arm64/x86_64, Linux x86_64/aarch64 glibc and musl); no Rust toolchain is needed.
- GitHub Actions: `uses: costajohnt/rbcop@v<version>` downloads and caches the release binary.
- Homebrew formula and release archives with each release.

[docs/migration.md](docs/migration.md) covers switching a project from RuboCop: Gemfile, binstubs, CI, editors and plugins. `docs/releasing.md` describes how releases are built.

## Build

```sh
cargo build --release   # needs Rust 1.88 or newer
```

The binary is `target/release/rbcop`. The crate is not published to crates.io: it builds against a patched copy of `ruby-prism` in `vendor/ruby-prism`.

## Usage

The command line is RuboCop's. Options, `.rubocop.yml` lookup and inheritance, `RUBOCOP_OPTS`, `.rubocop` argument files, inline `# rubocop:disable` / `enable` / `todo` comments, output and exit status follow RuboCop 1.91.0.

```sh
rbcop [options] [paths...]
rbcop -a [paths...]    # safe autocorrect
rbcop -A [paths...]    # all autocorrections
rbcop -f json -o report.json
```

All builtin formatters are supported, as are the display options, `--fail-level`, `--stdin` (with `-a`/`-A` it prints the corrected source after the report), `-L`, `--show-cops`, `-v` and `-V`.

`--auto-gen-config` (with its options), `--regenerate-todo`, `--disable-uncorrectable` and `--report-unused-todo-entries` are supported. Not supported: `--changed`, the server, LSP and MCP modes, `-r`/`--plugin`, `-d`, `--profile`, `--memory`, custom formatter classes, `require:` of local Ruby files and remote `inherit_from` URLs. ERB in config files is rendered by running `ruby`, as RuboCop renders it. `--cache` is accepted and has no effect. [docs/known-gaps.md](docs/known-gaps.md) lists these and every other known difference, including a few deliberate ones (for example, `-o` pointing at a file being inspected is refused). `-a`/`-A` do not write files that resolve (through symlinks) outside the working directory and the paths given on the command line; those are reported uncorrected with a warning.

## Plugins and custom cops

rbcop reads plugin configuration (`plugins:` and `require:` gems contribute their `config/default.yml`), but it only runs the cops it implements: RuboCop's own and those of the plugins in [docs/plugins.md](docs/plugins.md) (rubocop-performance, rubocop-rails, rubocop-minitest, rubocop-rspec, rubocop-rspec_rails, rubocop-factory_bot, rubocop-capybara). When the configuration enables cops rbcop cannot run, it says so on stderr instead of silently skipping them:

```
rbcop: not running 45 cops it does not implement (Sorbet: 41, ThreadSafety: 4); set RBCOP_FALLBACK=1 to run them with RuboCop
```

Cops defined by Ruby code the config requires (`require: ./lib/cops/my_cop.rb`, or a plugin gem rbcop could not load) are named the same way ("cops from Ruby code the config requires"), since rbcop cannot know their names. A plugin that defines no cops but works through Ruby code (rubocop-erb's ERB extraction) is named as "the Ruby code of plugin rubocop-erb": rbcop runs no cops on the files its `AllCops: Include` brings in (the `.erb` views), and with the fallback RuboCop runs every cop on them.

With `RBCOP_FALLBACK=1`, rbcop runs RuboCop once more for exactly those cops (`--except` every cop rbcop implements) on the same files with the same configuration and options, and merges its offenses into its own report, so formatters, `--fail-level` and the exit status see one run. RuboCop must be installed: `rubocop` on `PATH`, or the command in `RBCOP_RUBOCOP` (for example `RBCOP_RUBOCOP="bundle exec rubocop"`). The saving is smaller, since RuboCop still parses every file, but core cops no longer run in Ruby. Limits:

- With `-a`/`-A`, RuboCop corrects its cops first, then rbcop corrects the result; the two never iterate together, so a correction that triggers the other side's cop is fixed on the next run.
- `--stdin` does not use the fallback (the notice is printed instead).
- `Lint/RedundantCopDisableDirective` never reports a directive naming a plugin or custom cop rbcop does not implement (nor a `disable all` while one is loaded), with or without the fallback; RuboCop reports those that turn out redundant (docs/known-gaps.md).
- The `offenses` formatter does not mark plugin cops `[Safe Correctable]`, and `--display-only-safe-correctable` leaves them out.

## Parity

`scripts/oracle.sh` records RuboCop's offenses on a corpus with every cop enabled (`config/all-cops.yml`), and `scripts/check-cops.sh` compares rbcop with those recordings. On nine corpora every implemented cop matches with zero differences:

| Corpus | Offenses (RuboCop = rbcop) | Differences |
|---|---:|---:|
| specs (RuboCop's spec examples) | 78,303 | 0 |
| rails | 371,334 | 0 |
| discourse | 905,462 | 0 |
| rubocop-src | 34,296 | 0 |
| jekyll | 15,218 | 0 |
| chef | 170,509 | 0 |
| fastlane | 119,121 | 0 |
| brew | 257,707 | 0 |
| mastodon | 73,899 | 0 |

`scripts/check-cold.sh` covers cops that rarely fire on those corpora, using RuboCop's own spec examples with their config variants: 1,443 offenses, 0 differences, and `-A` output identical to a live `rubocop -A` run. `scripts/check-autocorrect.sh` and `scripts/check-autocorrect-all.sh` compare `-a`/`-A` output with RuboCop on corpus files. Files skipped by the gates are listed in `scripts/check-skip.txt`, `scripts/cold-skip.txt` and `scripts/autocorrect-skip.txt`, with the reasons in docs/known-gaps.md.

## Speed

CI-shaped runs (Linux container, 4 vCPUs, each project's own config, RuboCop with an empty result cache, median of 3): rbcop is 10x to 19x faster than `rubocop --parallel`, about 14x on average.

| Codebase | Files | `rubocop --parallel` | rbcop | Speedup |
|---|---:|---:|---:|---:|
| rails | 3,476 | 50.6 s | 2.64 s | 19x |
| discourse | 12,441 | 140.6 s | 10.16 s | 14x |
| mastodon | 3,297 | 61.2 s | 6.10 s | 10x |
| brew | 1,977 | 69.0 s | 4.39 s | 16x |
| solidus | 2,171 | 28.3 s | 2.12 s | 13x |
| fastlane | 1,355 | 30.3 s | 2.02 s | 15x |

rbcop has no result cache, so RuboCop with a warm cache can be faster on an unchanged tree. Method, 2 vCPU numbers and raw data: [docs/benchmarks.md](docs/benchmarks.md).

## Gates

```sh
cargo fmt --check
cargo clippy --release --all-targets -- -D warnings
cargo test --release
RBCOP_CORPUS=<corpus dir> RBCOP_ORACLE=<oracle dir> scripts/verify.sh   # the above plus check-cops.sh all
```

`cargo test` replays recorded RuboCop output: every cop's RuboCop spec examples with their config variants, offenses and `-A` results (`tests/spec_corpus`, recorded by `scripts/record-spec-corpus.sh`), config scenarios (`tests/scenarios`), formatter and CLI goldens (`tests/formatters`), `--show-cops` (`tests/show_cops`) and autocorrect cases (`tests/autocorrect`). The scripts that record them (`scripts/record-spec-corpus.sh`, `scripts/scenarios.sh`, `scripts/formatters.sh`, `scripts/show-cops.sh`, `tests/autocorrect/record.sh`) and the oracle, autocorrect and cold-corpus gates need RuboCop 1.91.0 on `PATH` (with rbenv: `RBENV_VERSION=4.0.6`). Each script's header documents its arguments and environment.

[docs/porting-cops.md](docs/porting-cops.md) describes how a cop is ported and checked, including how the corpora and oracles are laid out. [docs/rubocop-config-semantics.md](docs/rubocop-config-semantics.md) describes the configuration rules `src/config/` follows.

## License and credits

rbcop is released under the MIT license (see `LICENSE`).

- [RuboCop](https://github.com/rubocop/rubocop) (MIT, Copyright (c) 2012-26 Bozhidar Batsov): rbcop's cop logic, messages and behaviour are ported from RuboCop's source, and `config/default.yml` and `config/obsoletion.yml` are RuboCop's files, compiled into the binary. RuboCop's license is in `config/LICENSE`.
- [Prism](https://github.com/ruby/prism) (MIT, Copyright 2022-present Shopify Inc.): the Ruby parser, through the `ruby-prism` crate. A patched copy lives in `vendor/ruby-prism` with its license in `vendor/ruby-prism/LICENSE.md`.

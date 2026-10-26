# rbcop

A reimplementation of [RuboCop](https://github.com/rubocop/rubocop) 1.91.0 in Rust, built on [Prism](https://github.com/ruby/prism) (`ruby-prism` crate).

The target is byte-identical output: same offenses, ranges, messages, `[Correctable]` flags, autocorrections, formatter output and exit status as RuboCop 1.91.0 for the same files and configuration. All 613 cops in RuboCop's `config/default.yml` are implemented. rubocop-performance, rubocop-rails, rubocop-minitest, rubocop-rspec, rubocop-rspec_rails, rubocop-factory_bot and rubocop-capybara run natively ([docs/plugins.md](docs/plugins.md)). Other plugins (rubocop-sorbet and so on) and custom cops do not; rbcop hands them to RuboCop when one is installed, and otherwise says which cops it skips (see [Plugins and custom cops](#plugins-and-custom-cops)).

## Install

- Gemfile: `gem "rbcop", require: false`, then `bundle exec rbcop` where you ran `bundle exec rubocop`. The gem carries a prebuilt binary (macOS arm64/x86_64, Linux x86_64/aarch64 glibc and musl); no Rust toolchain is needed.
- GitHub Actions: `uses: costajohnt/rbcop@v<version>` downloads and caches the release binary.
- Homebrew formula and release archives with each release.

[docs/migration.md](docs/migration.md) covers switching a project from RuboCop: Gemfile, binstubs, CI, editors and plugins. `docs/releasing.md` describes how releases are built.

## Build

```sh
cargo build --release   # needs Rust 1.89 or newer
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

`--auto-gen-config` (with its options), `--regenerate-todo`, `--disable-uncorrectable` and `--report-unused-todo-entries` are supported. The server options are accepted and run in process (no server is started), and `--lsp` runs RuboCop's language server. Not supported: `--changed` and the MCP mode. Plugin and custom cops rbcop does not implement, `-r`/`--plugin` and custom formatter classes run with RuboCop when one is installed (see [Plugins and custom cops](#plugins-and-custom-cops)). ERB in config files is rendered by running `ruby`, as RuboCop renders it, and remote `inherit_from` URLs are fetched as RuboCop fetches them. The result cache (`--cache`, `--cache-root`, `AllCops: UseCache`) works as RuboCop's, in its own `rbcop_cache` directory. [docs/known-gaps.md](docs/known-gaps.md) lists these and every other known difference, including a few deliberate ones (for example, `-o` pointing at a file being inspected is refused). `-a`/`-A` do not write files that resolve (through symlinks) outside the working directory and the paths given on the command line; those are reported uncorrected with a warning.

## Plugins and custom cops

rbcop reads plugin configuration (`plugins:` and `require:` gems contribute their `config/default.yml`), and runs natively the cops it implements: RuboCop's own and those of the plugins in [docs/plugins.md](docs/plugins.md) (rubocop-performance, rubocop-rails, rubocop-minitest, rubocop-rspec, rubocop-rspec_rails, rubocop-factory_bot, rubocop-capybara). Cops it cannot run (other plugins such as rubocop-sorbet, custom cops from `require: ./lib/cops/my_cop.rb`) go to RuboCop: the fallback.

The fallback is on by default and needs a RuboCop. rbcop uses `RBCOP_RUBOCOP` when set (for example `RBCOP_RUBOCOP="bin/rubocop"`), else `bundle exec rubocop` when the project's `Gemfile.lock` has the rubocop gem, else `rubocop` on `PATH`. It runs RuboCop once more for exactly the cops rbcop cannot run (`--except` every cop rbcop implements) on the same files with the same configuration and options, and merges its offenses into its own report, so formatters, `--fail-level` and the exit status see one run. The saving is smaller, since RuboCop still parses every file, but core cops no longer run in Ruby. A project that needs no fallback never starts RuboCop. If RuboCop fails to run (say `bundle exec` outside an installed bundle), rbcop exits 2 with its error.

RuboCop loads the project's Ruby (its Gemfile, plugins, cop files), as it would if you ran it, so rbcop says so first on stderr: `rbcop: running bundle exec rubocop for 4 cops it does not implement (ThreadSafety: 4); set RBCOP_FALLBACK=0 to never run Ruby` (stdout is unchanged; no line under `RBCOP_FALLBACK=1`, which asked for it). Under `--lsp`, `--mcp`, the server options, `--editor-mode` or `--stdin` (an editor opening a project), wherever they come from (command line, `.rubocop`, `RUBOCOP_OPTS`), the fallback and the hand-off run only with `RBCOP_FALLBACK=1`. `RBCOP_FALLBACK=0` turns it off: no Ruby is started for it at all. rbcop also runs `ruby` to render ERB in config files and to fetch remote `inherit_from` URLs, only when the config has them; [docs/known-gaps.md](docs/known-gaps.md) ("Security: when rbcop runs Ruby") lists every case and how to avoid each.

With no RuboCop found, or with `RBCOP_FALLBACK=0`, rbcop skips those cops and says so on stderr instead of skipping them silently:

```
rbcop: not running 45 cops it does not implement (Sorbet: 41, ThreadSafety: 4); set RBCOP_FALLBACK=1 to run them with RuboCop
rbcop: RuboCop runs them when one is found: add the rubocop gem to the bundle, put rubocop on PATH, or set RBCOP_RUBOCOP
```

(the second line only when none was found). Cops defined by Ruby code the config requires (`require: ./lib/cops/my_cop.rb`, or a plugin gem rbcop could not load) are named as "cops from Ruby code the config requires", since rbcop cannot know their names. A plugin that defines no cops but works through Ruby code (rubocop-erb's ERB extraction) is named as "the Ruby code of plugin rubocop-erb": rbcop runs no cops on the files its `AllCops: Include` brings in (the `.erb` views), and with the fallback RuboCop runs every cop on them. `RBCOP_FALLBACK=1` runs the fallback with `rubocop` even when none was found.

`-r`/`--require`, `--plugin` and custom formatter classes (`-r ./my_formatter.rb --format MyFormatter`) load Ruby code that can change anything, so rbcop hands the whole run to the same RuboCop with the same arguments: the output is RuboCop's, with no speedup. With no RuboCop found, or `RBCOP_FALLBACK=0`, rbcop refuses them (exit 2).

Limits of the fallback:

- With `-a`/`-A`, RuboCop corrects its cops first, then rbcop corrects the result; the two never iterate together, so a correction that triggers the other side's cop is fixed on the next run.
- `--stdin` does not use the fallback (the notice is printed instead), nor does `--diff` (the RuboCop child corrects by writing files; with `RBCOP_FALLBACK=1` the combination is refused).
- What a fallback cop prints itself (rubocop-require_tools' `Note: Could not load` lines) comes before rbcop's report instead of interleaved with RuboCop's.
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

rbcop with a warm result cache (an unchanged tree, as RuboCop's cache serves it) takes about 3% of the CPU time of an uncached run: 0.9 s of CPU instead of 27 s on rails, 3.0 s instead of 82 s on discourse. Method, 2 vCPU numbers and raw data: [docs/benchmarks.md](docs/benchmarks.md).

## Gates

```sh
cargo fmt --check
cargo clippy --release --all-targets -- -D warnings
cargo test --release
RBCOP_CORPUS=<corpus dir> RBCOP_ORACLE=<oracle dir> scripts/verify.sh   # the above plus check-cops.sh all
```

`cargo test` replays recorded RuboCop output: every cop's RuboCop spec examples with their config variants, offenses and `-A` results (`tests/spec_corpus`, recorded by `scripts/record-spec-corpus.sh`), config scenarios (`tests/scenarios`), formatter and CLI goldens (`tests/formatters`), `--show-cops` (`tests/show_cops`) and autocorrect cases (`tests/autocorrect`). The scripts that record them (`scripts/record-spec-corpus.sh`, `scripts/scenarios.sh`, `scripts/formatters.sh`, `scripts/show-cops.sh`, `tests/autocorrect/record.sh`) and the oracle, autocorrect and cold-corpus gates need RuboCop 1.91.0 on `PATH` (with rbenv: `RBENV_VERSION=4.0.6`). Each script's header documents its arguments and environment.

[docs/porting-cops.md](docs/porting-cops.md) describes how a cop is ported and checked, including how the corpora and oracles are laid out. [docs/rubocop-config-semantics.md](docs/rubocop-config-semantics.md) describes the configuration rules `src/config/` follows. [docs/upgrading-rubocop.md](docs/upgrading-rubocop.md) is the process for moving to a new RuboCop release.

## License and credits

rbcop is released under the MIT license (see `LICENSE`).

- [RuboCop](https://github.com/rubocop/rubocop) (MIT, Copyright (c) 2012-26 Bozhidar Batsov): rbcop's cop logic, messages and behaviour are ported from RuboCop's source, and `config/default.yml` and `config/obsoletion.yml` are RuboCop's files, compiled into the binary. RuboCop's license is in `config/LICENSE`.
- [Prism](https://github.com/ruby/prism) (MIT, Copyright 2022-present Shopify Inc.): the Ruby parser, through the `ruby-prism` crate. A patched copy lives in `vendor/ruby-prism` with its license in `vendor/ruby-prism/LICENSE.md`.

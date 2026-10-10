# rbcop

A reimplementation of [RuboCop](https://github.com/rubocop/rubocop) 1.91.0 in Rust, built on [Prism](https://github.com/ruby/prism) (`ruby-prism` crate).

The target is byte-identical output: same offenses, ranges, messages, `[Correctable]` flags, autocorrections, formatter output and exit status as RuboCop 1.91.0 for the same files and configuration. All 613 cops in RuboCop's `config/default.yml` are implemented. Plugins (rubocop-rails, rubocop-rspec and so on) and custom cops are not.

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

All builtin formatters except `autogenconf` are supported, as are the display options, `--fail-level`, `--stdin`, `-L`, `--show-cops`, `-v` and `-V`.

Not supported: `--auto-gen-config`, `--disable-uncorrectable`, `--server`, `--lsp`, custom formatter classes, `require:` of local Ruby files, ERB in `.rubocop.yml` and remote `inherit_from` URLs. `--cache` is accepted and has no effect. [docs/known-gaps.md](docs/known-gaps.md) lists these and every other known difference, including a few deliberate ones (for example, `-o` pointing at a file being inspected is refused).

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

`scripts/bench.sh` on the rails corpus (3,573 files, 612 cops, `--force-default-config`), Apple M4 with 10 cores, wall time. The machine was running other heavy jobs at the time (load average 12 to 26), so these are rough:

| Run | Time |
|---|---:|
| rubocop, one process, no cache | 198 s |
| rubocop --parallel, cold cache | 62 s |
| rubocop --parallel, warm cache | 6 s |
| rbcop (no cache; uses every core) | 10 s |

rbcop has no result cache, so RuboCop with a warm cache is faster on an unchanged tree.

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

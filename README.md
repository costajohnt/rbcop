# rbcop

A RuboCop-compatible Ruby linter in Rust, built on [Prism](https://github.com/ruby/prism) (`ruby-prism` crate).

The goal is output identical to RuboCop for every cop it implements, so it can replace RuboCop in CI and editors without changing what gets flagged.

## Status

Early. Eight cops, default config only:

- Layout/EmptyLines
- Layout/IndentationStyle
- Layout/LineLength
- Layout/TrailingEmptyLines
- Layout/TrailingWhitespace
- Lint/Debugger
- Style/FrozenStringLiteralComment
- Style/NilComparison

Inline `# rubocop:disable` / `enable` / `todo` comments are honored.

## Parity

`scripts/compare.sh <dir>` runs RuboCop 1.88.2 (`--force-default-config --only <these cops>`) and rbcop on the same tree and diffs the offenses.

| Corpus | Files | Offenses | Differences |
|---|---|---|---|
| rails/rails | ~3.5k | 5,529 | 0 |
| discourse/discourse | ~12.7k | 4,030 | 0 |

Known gap: RuboCop tags some LineLength offenses `[Correctable]` when it finds a place to break the line. rbcop does not model that yet, so the compare script ignores the tag for that cop.

## Speed

`scripts/bench.sh <dir>` on discourse (12,733 files, same eight cops, Apple Silicon, other work running on the machine):

| Run | Time |
|---|---|
| rubocop, one process | 329.5s |
| rubocop --parallel, cold cache | 71.0s |
| rubocop --parallel, warm cache | 61.0s |
| rbcop | 1.5s |

## Usage

```sh
cargo build --release
./target/release/rbcop [options] [paths...]   # same options, output and exit status as rubocop
./target/release/rbcop -A [paths...]          # autocorrect (-a: safe corrections only)
```

Every builtin RuboCop formatter except autogenconf (`-f progress|simple|clang|emacs|files|fuubar|github|html|json|junit|markdown|offenses|pacman|quiet|sarif|tap|worst`, with RuboCop's key abbreviations and `-o FILE`), the display options (`-D -E -S --display-time --display-only-*`), `--fail-level`, `--color`, `--stdin FILE`, `-L`, `--show-cops`, `-v` and `-V`. `scripts/formatters.sh` records RuboCop's output for `tests/formatters/cases.txt`; `tests/formatters.rs` checks rbcop reproduces it byte for byte.

## Not yet

- Autocorrect for cops that have no corrector yet (`-a`/`-A` reports them but does not fix them)

## License and credits

rbcop is released under the MIT license (see `LICENSE`).

- [RuboCop](https://github.com/rubocop/rubocop) (MIT, Copyright (c) 2012-26 Bozhidar Batsov): rbcop's cop logic, messages and behaviour are ported from RuboCop's source, and `config/default.yml` and `config/obsoletion.yml` are RuboCop's files, compiled into the binary. RuboCop's license is in `config/LICENSE`.
- [Prism](https://github.com/ruby/prism) (MIT, Copyright 2022-present Shopify Inc.): the Ruby parser, through the `ruby-prism` crate. A patched copy lives in `vendor/ruby-prism` with its license in `vendor/ruby-prism/LICENSE.md`.

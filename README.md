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
./target/release/rbcop [--only Cop1,Cop2] [paths...]   # emacs-format output, exit 1 on offenses
./target/release/rbcop -L .                             # list files that would be inspected
./target/release/rbcop -A [paths...]                    # autocorrect (-a: safe corrections only)
```

## Not yet

- `.rubocop.yml` (rbcop always behaves like `--force-default-config`)
- Output formats other than emacs
- Autocorrect for cops that have no corrector yet (`-a`/`-A` reports them but does not fix them)
- `# rubocop:push` / `pop`
- Lint/Syntax: files with syntax errors are skipped with a warning

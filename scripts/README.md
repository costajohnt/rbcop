# scripts/

One line per script; each header documents its arguments. Gates take RuboCop from `$RUBOCOP`
(`rubocop.sh`) and build rbcop through `rbcop.sh`.

## Gates
- `ci.sh`: the gate (fmt, clippy, build, tests; with `RBCOP_CORPUS`/`RBCOP_ORACLE` also the corpus tier).
- `check-cops.sh`: rbcop against recorded oracles, cop by cop, on the corpora.
- `check-cold.sh`: RuboCop's spec examples for cops too rare on the corpora.
- `check-plugins.sh`: each native plugin's cops against oracles recorded with that plugin.
- `check-autocorrect.sh`, `check-autocorrect-all.sh`: `-A` rewrites against RuboCop's, per cop and all cops together.
- `check-cache.sh`: result cache parity (cold, warm, warm after edits); not wired into `ci.sh`.
- `compare.sh`: diff rbcop against live RuboCop on one directory.
- `missing-correctors.sh`: cops marked correctable that never add a fix.
- `check-skip.txt`, `cold-skip.txt`, `autocorrect-skip.txt`: files the gates skip (docs/known-gaps.md).

## Recording (tests and oracles)
- `oracle.sh`, `oracle_formatter.rb`: record the corpus oracles `check-cops.sh` reads.
- `scenarios.sh`, `formatters.sh`, `show-cops.sh`, `lsp.sh`, `mcp.sh`: record tests/scenarios, tests/formatters, tests/show_cops, tests/lsp, tests/mcp.
- `record-spec-corpus.sh`, `record_spec_corpus.rb`: re-record tests/spec_corpus.
- `extract_cold_corpus.rb`, `extract_minitest_corpus.rb`: build the cold corpus from RuboCop's (and rubocop-minitest's) specs.

## Generated tables
- `autocorrect_registry.rb`: src/autocorrect/registry.tsv (registry order, `support_autocorrect?`, `autocorrect_incompatible_with`).
- `dump-cop-docs.rb`: src/cop_docs.txt (`--explain`).
- `dump-ruby-exceptions.rb`: src/helpers/ruby_exceptions/<linux|darwin>.rs, for the platform it runs on.
- `display_widths.rb`: the display width table in src/helpers/alignment.rs.
- `encodings.rb`: src/encodings.bin.
- `gen_node_kinds.py`: src/node_kind.rs.
- `cop_inventory.rb`: docs/cop-inventory.tsv (porting inventory).

## Upgrading RuboCop
- `rubocop_delta.rb`: what changed between two RuboCop releases (docs/upgrading-rubocop.md).
- `prism-patch.sh`: rebuild vendor/ruby-prism from the crates.io crate and vendor/ruby-prism.patches.

## Benchmarks
- `bench-ci.sh`: the CI-shaped benchmark in docs/benchmarks.md.

## Release (`dist/`, docs/releasing.md)
- `dist/build.sh`: release archives; `dist/gem.sh`: gems; `dist/homebrew.sh`: Homebrew formula.
- `dist/compare.sh`: a packaged binary against a cargo build on the corpora.
- `dist/install.sh`: checksum-verified install of a release binary.

## Differential fuzzing (`fuzz/`)
- `fuzz/round.sh`: one round (mutate, then compare); `fuzz/mutate.rb`: variant generator; `fuzz/run.rb`: compare on variants.
- `fuzz/ac.rb`, `fuzz/acdiff.sh`: `-A` comparison on variants or one file; `fuzz/cmp1.sh`: compare on single files.
- `fuzz/minimize.rb`: shrink a disagreeing variant; `fuzz/show.rb`: inspect variants.

## Shared
- `rbcop.sh`, `rubocop.sh`: sourced helpers that locate (and build) rbcop and RuboCop.

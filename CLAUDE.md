# rbcop

Rust reimplementation of RuboCop 1.91.0 on Prism. Target is byte-identical output
to RuboCop (offenses, ranges, messages, `[Correctable]`, autocorrect, formatters,
exit status). `README.md` covers install, usage, parity and speed.

## Build / gates
- `cargo build --release` (Rust 1.88+). Binary: `target/release/rbcop`.
- Builds against the patched `vendor/ruby-prism`; the crate is not on crates.io.
- `scripts/ci.sh` is the gate: `cargo fmt --check`,
  `cargo clippy --release --all-targets -- -D warnings`, `cargo build --release`,
  `cargo test --release`. With `RBCOP_CORPUS` and `RBCOP_ORACLE` set it also runs
  `check-cops.sh all`, `check-cold.sh` and `check-plugins.sh`. Prints `ci: ok at <sha>`.
- There is no PR CI workflow. `.github/workflows/release.yml` is `workflow_dispatch`
  only. Run `scripts/ci.sh` before pushing.
- `cargo test` replays recorded RuboCop output (`tests/spec_corpus`, `tests/scenarios`,
  `tests/formatters`, `tests/show_cops`, `tests/autocorrect`). Recording scripts and
  the oracle/cold/autocorrect gates need RuboCop 1.91.0 on PATH
  (`RBENV_VERSION=4.0.6`; scripts take `$RUBOCOP`). Each script's header documents
  its arguments.

## Docs to read first
- `docs/porting-cops.md`: how a cop is ported and checked, corpus and oracle layout.
- `docs/rubocop-config-semantics.md`: config rules `src/config/` follows.
- `docs/known-gaps.md`: why files are in `scripts/*-skip.txt`.
- `docs/plugins.md`, `docs/migration.md`, `docs/releasing.md`.
- `docs/upgrading-rubocop.md`: moving the target to a new RuboCop release (#299).

## Rules
- RuboCop is the oracle. When unsure what a cop should do, run RuboCop 1.91.0.
- Releases: follow `docs/releasing.md`. Checksums in `packaging/checksums/X.Y.Z.sha256`
  are committed before the tag; never rebuild archives between committing checksums
  and uploading.
- Other sessions work in this repo via `.claude/worktrees/`. Use your own worktree.

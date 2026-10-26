# Upgrading the target RuboCop version

rbcop targets one RuboCop release at a time (`RUBOCOP_VERSION` in `src/config/mod.rs`, printed by
`rbcop -v`). Every recorded expectation in the repo and every oracle the gate reads comes from that
release, so the target moves in one step: port everything, re-record everything, bump the version
in the same change. A half-ported upgrade matches neither release: until the whole delta is
ported, upgrade work stays on its branch and master keeps gating against the current target.
Issue #299 tracks the upgrades.

The steps run on the CI host (CT 440 `rbcop-ci`, see `rbcop-ci/README-agents.md` in rbcop-ops),
never on a Mac, with `export LC_ALL=C.UTF-8 LANG=C.UTF-8`.

## 1. Install the new release next to the old one

Keep the current release as the default `rubocop` (other gates still need it) and put the new one
in a scratch gem home:

    S=~/scratch/rubocop-X.Y.Z; mkdir -p $S
    GEM_HOME=$S/gems gem install rubocop -v X.Y.Z --no-document
    export NEW="env GEM_PATH=$S/gems:$(gem env gempath) rubocop _X.Y.Z_"
    $NEW -V

Check its gemspec: a new minimum for `rubocop-ast`, `parser` or `unicode-display_width` means
those move too (`rbcop -V` prints the rubocop-ast version, `src/helpers/alignment.rs` is generated
from unicode-display_width by `scripts/display_widths.rb`). For each native plugin
(docs/plugins.md), find the newest release whose gemspec accepts X.Y.Z and install it in the same
gem home; a plugin bump is its own port (docs/plugins.md, "Porting a plugin").

## 2. Inventory the delta

    gh release view vX.Y.Z -R rubocop/rubocop --json body -q .body > notes.md   # each skipped release too
    OLD=$(ruby -e 'puts Gem::Specification.find_by_name("rubocop", "<old>").gem_dir')
    ruby scripts/rubocop_delta.rb $OLD $S/gems/gems/rubocop-X.Y.Z notes.md > delta.tsv 2> delta-lib.txt

`delta.tsv` has one row per cop whose source, `config/default.yml` entry or release notes changed;
`delta-lib.txt` lists the non-cop files that changed (runner, CLI options, formatters, config
loading, LSP/MCP, mixins). Read `diff -u` of `config/default.yml` in full: new cops, new or changed
parameters, `Enabled`/`Safe`/`SafeAutoCorrect` flips and `VersionChanged` stamps are all visible
in `--show-cops` and in config validation. Read `config/obsoletion.yml` too (renamed and removed
cops feed `src/config/validator.rs`).

Then measure what reaches real code: record the new release's oracles into scratch (never over the
gate's oracle dirs) and rerun the script with both oracle dirs:

    for c in specs rails discourse rubocop-src jekyll chef fastlane brew mastodon; do
      RUBOCOP="$NEW" scripts/oracle.sh $RBCOP_CORPUS/$c $S/oracle/$c.txt
    done
    ruby scripts/rubocop_delta.rb $OLD $S/gems/gems/rubocop-X.Y.Z notes.md $RBCOP_ORACLE $S/oracle > delta.tsv

`only_old` / `only_new` count corpus offenses that disappear or appear for each cop, and the crash
columns count (cop, file) pairs whose RuboCop errors were fixed or introduced. Rows with corpus
changes are the ones the gate will catch; rows with only source or notes changes need the spec
corpus (step 4) and targeted cases to catch. Commit the final `delta.tsv` as
`docs/upgrade-X.Y.Z.tsv` and work from it.

## 3. Port

- Cops: diff each changed cop file (`diff -u $OLD/lib/rubocop/cop/<dept>/<cop>.rb ...`) and port the
  change into `src/cops/<dept>/<cop>.rs` as in docs/porting-cops.md. New cops are ordinary ports;
  add them to `config/default.yml` (step 5) and the inventory (`ruby scripts/cop_inventory.rb >
  docs/cop-inventory.tsv`, run with the new release on the load path).
- Mixins and correctors in `delta-lib.txt` (`lib/rubocop/cop/mixin/`, `cop/correctors/`, `cop/util.rb`)
  map to `src/helpers/`; grep every cop that uses the helper before changing it.
- Runner, options, formatters, config loading, LSP and MCP map to `src/main.rs`, `src/runner*`,
  `src/formatter*`, `src/config/`, `src/lsp.rs`, `src/mcp.rs`. New options go in the option table in
  `src/main.rs`, and `rubocop -h` is re-recorded verbatim there.
- Regenerated tables: `src/autocorrect/registry.tsv` (`scripts/autocorrect_registry.rb`, registry
  order and `autocorrect_incompatible_with`), `src/helpers/ruby_exceptions.rs`
  (`scripts/dump-ruby-exceptions.rb`), `src/helpers/nil_methods.rs` and the display width table when
  their gems move.
- Behaviour the new release drops on purpose may already be listed in docs/known-gaps.md as a
  difference from the old one: re-check every entry that names the old version.

## 4. Re-record

All with `RUBOCOP="$NEW"`, so nothing records with the old release by accident:

- `tests/spec_corpus`: `scripts/record-spec-corpus.sh <rubocop-src checkout at vX.Y.Z>` (clone into
  scratch, `git -C <dir> checkout vX.Y.Z`; the corpus copy of rubocop-src stays as it is).
- `scripts/scenarios.sh`, `scripts/formatters.sh`, `scripts/show-cops.sh`, `scripts/lsp.sh`,
  `scripts/mcp.sh`, `tests/autocorrect/record.sh`, and the `tests/auto_gen` expectations (their
  `.rubocop_todo.yml` header names the version).
- Gate oracles: `scripts/oracle.sh` for the nine corpora, the corpus2 projects (their own configs,
  as the existing oracle3 set) and each plugin oracle set (`CONFIG=config/all-cops-<plugin>.yml`),
  and `scripts/check-cold.sh oracle` for the cold corpus, all into a NEW oracle root. The CI host's
  `ci-run.sh` points `RBCOP_ORACLE` at one root for every branch, so switching roots is a
  coordinated change in rbcop-ops made when the upgrade PR merges, not before (open branches still
  gate against the old root).

Review each re-recorded diff: every change should trace to a row of `delta.tsv`. A change with no
row is a recording problem (wrong version, locale, plugin version) until shown otherwise.

## 5. Bump the version

In the same change as the ports and recordings:

- `RUBOCOP_VERSION` in `src/config/mod.rs` (`rbcop -v`, `-V`, the JSON/HTML formatters,
  `--auto-gen-config`, LSP, the remote-config shim).
- `config/default.yml`: copy the new release's file verbatim.
- The version named in prose and comments: `git grep -n '<old>'` (README.md, CLAUDE.md, action.yml,
  packaging/gem/rbcop.gemspec, packaging/homebrew/rbcop.rb.in, docs/*.md, script headers). Comments
  that describe what the old release did stay as history only where they explain a known gap.
- docs/plugins.md if a plugin pin moved, and its "newest release that supports RuboCop X.Y.Z" text.

## 6. Gate

`scripts/ci.sh` with `RBCOP_ORACLE` at the new oracle root (on the CI host:
`FRESH=1 rbcop-remote-ci <worktree>` once `ci-run.sh` is switched, or run `ci-run.sh`'s commands by
hand with the new root before that). The bar is the usual one: `plugins: all corpora match`, every
check-cops corpus at diff 0, the cold corpus and `cargo test` green, the corpus2 baseline no worse.
Then also run `scripts/check-autocorrect-all.sh` with the new release, since -A output moves with
almost every bug-fix release.

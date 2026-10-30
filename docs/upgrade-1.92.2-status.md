# RuboCop 1.92.2 port status

**Done.** Every batch below landed on `feat/rubocop-1.92.2` (L1 #705, C1 #706, C2 #707, C3 #708,
C4 #709), which merged to master in #704 (full gate `remote ci: ok at e853378b`, all-cops `-A`
diff 0 on specs, rails, discourse and mastodon). Kept as the record of the port; the tables describe
the base commit, not master.

Worklist for #299 on `feat/rubocop-1.92.2`, taken 2026-10-29 at the base commit that bumps
`RUBOCOP_VERSION` to 1.92.2 and re-records every expectation with RuboCop 1.92.2 (no cop ports yet).
Port PRs target `feat/rubocop-1.92.2`; the branch merges to master once everything here is green.
Process: docs/upgrading-rubocop.md. Delta inventory: docs/upgrade-1.92.2.tsv (cops) and
docs/upgrade-1.92.2-lib.tsv (everything else).

## The delta

1.92.2 (released 2026-10-29) is a bug-fix release: 55 release-note entries, no new, removed or
renamed cops. `config/default.yml` and `config/obsoletion.yml` are byte-identical to 1.92.1's, and
the gemspec's dependency minimums are unchanged. Every native plugin pin accepts 1.92.2
(docs/plugins.md), and each pin is still that plugin's newest release.

- 48 cop rows: 30 with source changes, 18 named in the release notes only as the other cop of an
  autocorrect interaction (0 lib lines). rbcop implements all 48.
- 10 non-cop lib files: five mixins and correctors (`hash_alignment_styles`, `statement_modifier`,
  `comments_help`, `trailing_comma`, `empty_line_corrector`), `runner.rb` (the redundant-disable
  check no longer sees corrected offenses), `config.rb` (the `Using <old> configuration in ...`
  warning for a renamed cop's config is gone), `config_obsoletion.rb` and
  `plugin/configuration_integrator.rb` (obsolete keys in a plugin's own config warn, naming the
  plugin), `version.rb`.
- `autocorrect_incompatible_with` changes for six cops (Layout/SingleLineBlockChain,
  Layout/SpaceInsideBlockBraces, Style/ClassAndModuleChildren, Style/EndlessMethod,
  Style/HashAsLastArrayItem, Style/MethodCallWithArgsParentheses). `src/autocorrect/registry.tsv`
  is regenerated in the base commit, so those rows need no port.
- Nine corpora (1.92.1 vs 1.92.2 oracle2): 50 offenses gone, 3 new, no crash changes, in five cops:
  Layout/SpaceAroundOperators (47 gone), Layout/LineLength (1/1), Lint/PercentStringArray (1/1),
  Lint/RedundantSplatExpansion (1 gone), Style/FloatDivision (1 new). No cop outside the inventory
  changes on the corpora. Everything else is autocorrect output or spec-only.

## How this was measured

On CT 440, with the gate environment for 1.92.2 (docs/upgrading-rubocop.md, "Gate environments"):
`scripts/record-spec-corpus.sh` (rubocop checkout at v1.92.2), `scripts/scenarios.sh`,
`scripts/formatters.sh`, `scripts/show-cops.sh`, `tests/autocorrect/record.sh`,
`tests/auto_gen/record.sh`, `scripts/lsp.sh` and `scripts/mcp.sh`, all with RuboCop 1.92.2 plus
mcp 0.25.0 and rubydex 0.4.1 on the gem path, all exit 0. Every re-recorded diff is a version string
or traces to an inventory row; `require_unloadable_after_plugin/stderr.txt` keeps its trimmed form
(host-specific backtrace, as for 1.92.1). Tests that name the version by hand
(`tests/core_review.rs`, `tests/fallback.rs`, `tests/mcp_changed.jsonl`) moved with the bump.

## Failing tests at the base commit

`cargo fmt --check` and clippy pass. `cargo test --release --no-fail-fast`:

| test | failure | batch |
|---|---|---|
| `spec_corpus::spec_corpus_matches_rubocop` | 45 variants of 28 cops (per cop below) | C1 to C4 |
| `scenarios::config_scenarios_match_rubocop` | `config_color_warnings`, `config_deprecated_name`, `config_inherit_base_warnings`, `config_unknown_param`: rbcop still prints `Warning: Using <old> configuration in ...` | L1 |
| `formatters::formatters_match_rubocop` | `project-json`, `json-cop-names`, `json-abbrev-cop-names`: Layout/SpaceInsideRangeLiteral's correction edits | C1 |
| `auto_gen::auto_gen_matches_rubocop` | `uncorrectable`: one more `[Corrected] Lint/RedundantCopDisableDirective` (runner) | C2 |

## Batches

### C1: Layout (+ `hash_alignment_styles`, `empty_line_corrector`)

Spec variants failing: EmptyLinesAfterModuleInclusion 1, EndAlignment 1, ExtraSpacing 1,
HashAlignment 3, LineLength 1, RescueEnsureAlignment 1, SpaceAroundOperators 1,
SpaceBeforeBlockBraces 2, SpaceInsideRangeLiteral 1. Corpus: SpaceAroundOperators 47,
LineLength 2. Also the three JSON formatter cases. EmptyLinesAroundClassBody (via
`empty_line_corrector`) has no failing variant; port the corrector change anyway and check -A.

### C2: Lint (+ runner `offenses_to_check`)

Spec variants failing: InterpolationCheck 1, NestedPercentLiteral 1, ParenthesesAsGroupedExpression 1,
PercentStringArray 1 (new message `MSG_SPLIT_STRING`), RedundantSplatExpansion 1.
Lint/RedundantCopDisableDirective (repeated disables, lines shifted by autocorrect) plus the runner
change; `auto_gen` `uncorrectable`. Corpus: PercentStringArray 2, RedundantSplatExpansion 1.

### C3: Style A to M

Spec variants failing: AndOr 3, CaseEquality 1, EndlessMethod 2, FloatDivision 4, HashLookupMethod 3,
MethodDefParentheses 2. Corpus: FloatDivision 1.

### C4: Style N to Z (+ `statement_modifier`, `comments_help`, `trailing_comma`)

Spec variants failing: NumericPredicate 2, QuotedSymbols 2, RaiseArgs 2, ReduceToHash 2,
RedundantBegin 1, RedundantException 1, StringConcatenation 1, TrailingCommaInArguments 2.
Source changes without a failing variant: ParenthesesAroundCondition, RedundantCondition,
RedundantSelf, SingleLineMethods, and the `statement_modifier` / `comments_help` mixins
(Style/IfUnlessModifier, Style/WhileUntilModifier, Style/RedundantCondition); port them from the
diff with targeted cases.

### L1: config and plugin warnings

`config.rb` (drop the renamed-cop config warning), `config_obsoletion.rb#messages` and
`ConfigurationIntegrator#warn_about_obsolete_config` (a plugin's obsolete config warns with the
plugin's name and never stops the run). The four scenarios above.

When every batch is in: `FRESH=1 rbcop-remote-ci` at the head of `feat/rubocop-1.92.2`, then
`scripts/check-autocorrect-all.sh` with 1.92.2.

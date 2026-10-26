# RuboCop 1.92.1 port status

Worklist for #299 on `feat/rubocop-1.92.1`, taken 2026-10-26 at the base commit that bumps
`RUBOCOP_VERSION` to 1.92.1, vendors 1.92.1's `config/default.yml` and re-records every expectation
with RuboCop 1.92.1 (no cop ports yet). Port PRs target `feat/rubocop-1.92.1`; the branch merges to
master once everything here is green. Process: docs/upgrading-rubocop.md. Delta inventory:
docs/upgrade-1.92.1.tsv (cops) and docs/upgrade-1.92.1-lib.tsv (everything else).

## How this was measured

On CT 440, with the gate environment for 1.92.1 (`~/rubocop-1.92.1/bin/rubocop`, oracle root
`.../tmp-1.92.1/`):

- Recordings: `scripts/record-spec-corpus.sh` (rubocop checkout at v1.92.1), `scripts/scenarios.sh`,
  `scripts/formatters.sh`, `scripts/show-cops.sh`, `tests/autocorrect/record.sh`,
  `tests/auto_gen/record.sh`, `scripts/lsp.sh`, `scripts/mcp.sh`, all with `RUBOCOP` set to the
  1.92.1 wrapper plus a scratch gem home holding mcp 0.25.0 and rubydex 0.4.1 (what the 1.91.0
  recordings had: without rubydex the project-index scenarios lose their offenses). A control run
  of the same scripts with RuboCop 1.91.0 and the same extra gems reproduced every committed 1.91.0
  fixture except the host-specific backtrace in `require_unloadable_after_plugin/stderr.txt`, which
  keeps its trimmed form. So the re-recorded diffs are the release, not the host.
- Gate: `FRESH=1 rbcop-remote-ci` prints `target: rubocop 1.92.1` and stops at the first failing
  test binary (`tests/autocorrect.rs`). The rest below comes from `cargo test --release
  --no-fail-fast`, then `check-cops.sh all`, `check-cold.sh`, `check-plugins.sh` and the corpus2
  `check-cops.sh` run by hand in the same environment.

## Gate summary

| check | result |
|---|---|
| fmt, clippy, build | pass |
| `cargo test --release --no-fail-fast` | 8 failing tests in 7 binaries (below); every other test passes |
| `check-cops.sh all`, nine corpora | specs 153, rails 347, discourse 1628, rubocop-src 83, jekyll 7, chef 242, fastlane 369, brew 458, mastodon 48 (3,335 offenses: 457 only RuboCop, 2,878 only rbcop) |
| `check-cold.sh` | cold corpus matches (offenses diff 0, autocorrect diff 0) |
| `check-plugins.sh` | rubocop-rails rubocop-src diff 4 (Lint/NumberConversion, +4 rbcop); rubocop-rspec mastodon-project diff 2 (Style/BitwisePredicate, -2); everything else matches |
| corpus2 (report only, baseline not zero) | sidekiq 5, puma 25, devise 24, rspec 36, graphql-ruby 187, solidus 57 |

The corpus diffs match the inventory's `only_old` / `only_new` totals (2,878 gone, 457 new) and
every cop with a diff is an inventory row.

**Hang:** `tests/spec_corpus.rs` does not finish on its own. `rbcop -A --only
Lint/UnescapedBracketInRegexp` loops on `Lint_UnescapedBracketInRegexp/r2.7` (a new 1.92.1 spec
example; 2.8 GB RSS after 24 minutes) and was killed by hand, so that variant reports `exit=signal`.
Port Lint/UnescapedBracketInRegexp first, or run the spec corpus with
`SPEC_CORPUS_ONLY=<prefix>` until it is fixed.

## Failing tests

| test | failure | area / batch |
|---|---|---|
| `spec_corpus::spec_corpus_matches_rubocop` | 242 variants of 147 cops (per cop in the batch tables) | cop batches C1 to C11 |
| `scenarios::config_scenarios_match_rubocop` | `config_obsolete_removed` (removed-cop message without alternatives), `pending_cops_qualified` (plugin pending cops now listed), `directive_no_department_kinds` (one more no-department warning) | L4 |
| | `fuzz_regressions` (Layout/ClosingParenthesisIndentation no longer `[Correctable]`, Layout/EmptyLines on CRLF, Style/MethodCallWithoutArgsParentheses on CRLF), `it_block_lvar` (Style/CollectionQuerying message, Style/CombinableLoops not correctable), `numblock_itblock_not_block` (Style/CombinableLoops), `list_settings_string_sweep` (Naming/MethodName forbidden names) | C3 (Layout/EmptyLines), C9 (Style/MethodCallWithoutArgsParentheses), C6 (Style/CollectionQuerying, Style/CombinableLoops), L3 (the `[Correctable]` flips; Layout/ClosingParenthesisIndentation is not an inventory row), L7 (Naming/MethodName via ForbiddenIdentifiers) |
| `formatters::formatters_match_rubocop` | 19 of 158 cases: every json, sarif and pacman case, `color-pacman`, `help` (new options), `show-docs-url` (status), `parallel-ignored` | L1, L3 |
| `autocorrect::autocorrect_matches_rubocop`, `autocorrect::diff_matches_rubocop` | `plugin_directives` (first failing case; the test stops there, `redundant_disable_diff` and `transcoded_sources` were re-recorded with changes too) | L3 (team, Lint/RedundantCopDisableDirective) |
| `autocorrect` case `infinite_loop` (added by #680, re-recorded with 1.92.1) | 1.92.1 no longer applies Style/Semicolon, Layout/TrailingWhitespace and Layout/DefEndAlignment on `b.rb` before the loop is detected | L3 (autocorrect loop / team) |
| `fallback::fallback_forwards_the_command_line` | live RuboCop 1.92.1's JSON differs from rbcop's (new JSON fields) | L3 |
| `lsp::lsp_transcripts_match_rubocop` | `modes`, message 6 (formatting results, diagnostics) | L5 |
| `mcp::mcp_transcripts_match_rubocop` | `project_index`, line 2 (first failing case; `protocol` and `tools` were re-recorded with changes too) | L5 |

Tests that assert hand-typed RuboCop output in Rust (`tests/core_review.rs`, `tests/lsp.rs`,
`tests/remote_config.rs`, `tests/server.rs`, `tests/changed.rs`, `tests/debug.rs` and others whose
comments say "recorded with RuboCop 1.91") still pass because rbcop did not change; each lib batch
re-checks the ones in its area against 1.92.1. Module comments in `src/` that name 1.91.0 say what
the code was ported from and move with the port of that module.

## Cop batches

Cops are the inventory rows (docs/upgrade-1.92.1.tsv), split by department into batches of about
equal work (lib lines changed, failing spec variants, corpus diffs). "corpus" is offenses only in
RuboCop / only in rbcop over the nine corpora. Mixins and correctors used by several batches are in
L6 and L7; a batch that needs one first coordinates with that batch.

### Batch C1: Bundler, Gemspec, Metrics, Naming, Security, Bundler/GemComment to Security/JSONLoad (14 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Bundler/GemComment | 1 |  |  |  |
| Bundler/OrderedGems | 8 |  |  |  |
| Gemspec/DependencyVersion | 6 |  |  |  |
| Gemspec/OrderedDependencies | 6 | 1 |  |  |
| Metrics/ClassLength | 6 |  |  |  |
| Metrics/CyclomaticComplexity | 4 |  |  |  |
| Metrics/ModuleLength | 6 |  |  |  |
| Naming/AccessorMethodName | 16 |  |  |  |
| Naming/BlockForwarding | 2 |  |  |  |
| Naming/HeredocDelimiterCase | 11 | 1 |  |  |
| Naming/HeredocDelimiterNaming | 8 |  |  |  |
| Naming/PredicatePrefix | 29 |  |  |  |
| Naming/RescuedExceptionsVariableName | 2 |  |  |  |
| Security/JSONLoad | 20 | 1 | 4/0 | specs -4 |

### Batch C2: Lint, Lint/AmbiguousOperator to Lint/Void (20 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Lint/AmbiguousOperator | 2 |  |  |  |
| Lint/AmbiguousOperatorPrecedence | 3 | 1 | 0/2 | specs +2 |
| Lint/AmbiguousRegexpLiteral | 7 | 2 |  |  |
| Lint/EmptyConditionalBody | 4 | 1 | 4/4 | specs -4, specs +4 |
| Lint/HeredocMethodCallPosition | 4 |  |  |  |
| Lint/LiteralAsCondition | 15 | 1 |  |  |
| Lint/LiteralInInterpolation | 82 | 1 | 0/1 | specs +1 |
| Lint/Loop | 26 | 1 |  |  |
| Lint/MisplacedMagicComment | 82 | 1 |  |  |
| Lint/MissingCopEnableDirective | 21 | 1 |  |  |
| Lint/NumberConversion | 4 | 1 | 0/4 | rubocop-src +4 |
| Lint/RedundantCopDisableDirective | 13 | 1 |  |  |
| Lint/RedundantDirGlobSort | 15 | 1 |  |  |
| Lint/SafeNavigationConsistency | 48 | 1 |  |  |
| Lint/SymbolConversion | 1 | 1 |  |  |
| Lint/UnescapedBracketInRegexp | 19 | 1 |  |  |
| Lint/UselessAssignment | 21 | 2 | 29/0 | chef -6, discourse -15, specs -8 |
| Lint/UselessRuby2Keywords | 0 |  |  |  |
| Lint/UselessSetterCall | 11 | 1 |  |  |
| Lint/Void | 9 | 1 |  |  |

### Batch C3: Layout, Layout/ArgumentAlignment to Layout/FirstArrayElementLineBreak (21 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Layout/ArgumentAlignment | 6 | 1 |  |  |
| Layout/BlockAlignment | 27 |  |  |  |
| Layout/BlockEndNewline | 0 |  |  |  |
| Layout/CaseIndentation | 34 | 3 |  |  |
| Layout/ClassStructure | 36 | 1 | 106/29 | brew -11, brew +2, chef -4, chef +2, discourse -75, discourse +23, fastlane -4, fastlane +1, rails -10, rails +1, rubocop-src -2 |
| Layout/DefEndAlignment | 4 | 1 |  |  |
| Layout/DotPosition | 40 |  |  |  |
| Layout/ElseAlignment | 0 |  | 1/1 | brew -1, brew +1 |
| Layout/EmptyLineAfterMultilineCondition | 29 | 1 | 0/6 | brew +2, discourse +1, rails +1, specs +2 |
| Layout/EmptyLines | 7 | 1 |  |  |
| Layout/EmptyLinesAroundAccessModifier | 0 |  |  |  |
| Layout/EmptyLinesAroundAttributeAccessor | 1 | 1 |  |  |
| Layout/EmptyLinesAroundBlockBody | 12 | 2 |  |  |
| Layout/EmptyLinesAroundClassBody | 0 |  |  |  |
| Layout/EmptyLinesAroundExceptionHandlingKeywords | 24 | 1 |  |  |
| Layout/EmptyLinesAroundMethodBody | 0 |  |  |  |
| Layout/EmptyLinesAroundModuleBody | 0 |  |  |  |
| Layout/EndAlignment | 36 | 1 |  |  |
| Layout/ExtraSpacing | 4 | 1 |  |  |
| Layout/FirstArgumentIndentation | 28 | 1 |  |  |
| Layout/FirstArrayElementLineBreak | 15 | 1 |  |  |

### Batch C4: Layout, Layout/FirstHashElementIndentation to Layout/MultilineArrayLineBreaks (14 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Layout/FirstHashElementIndentation | 24 | 1 | 2/2 | brew -2, brew +2 |
| Layout/FirstMethodArgumentLineBreak | 6 |  |  |  |
| Layout/FirstMethodParameterLineBreak | 4 |  |  |  |
| Layout/HashAlignment | 28 | 5 |  |  |
| Layout/HeredocArgumentClosingParenthesis | 10 | 1 |  |  |
| Layout/HeredocIndentation | 15 | 1 |  |  |
| Layout/IndentationConsistency | 27 | 1 |  |  |
| Layout/IndentationStyle | 5 | 1 |  |  |
| Layout/IndentationWidth | 4 |  | 2/2 | rails -2, specs +2 |
| Layout/LineContinuationSpacing | 9 | 1 |  |  |
| Layout/LineEndStringConcatenationIndentation | 34 | 2 | 0/3 | specs +3 |
| Layout/LineLength | 174 | 1 | 3/3 | mastodon -2, mastodon +2, rails -1, rails +1 |
| Layout/MultilineArrayBraceLayout | 12 | 3 |  |  |
| Layout/MultilineArrayLineBreaks | 0 | 1 |  |  |

### Batch C5: Layout, Layout/MultilineBlockLayout to Layout/SpaceInsideStringInterpolation (22 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Layout/MultilineBlockLayout | 9 | 1 |  |  |
| Layout/MultilineHashBraceLayout | 9 | 1 |  |  |
| Layout/MultilineHashKeyLineBreaks | 0 |  |  |  |
| Layout/MultilineMethodArgumentLineBreaks | 0 | 1 | 0/9 | brew +9 |
| Layout/MultilineMethodCallBraceLayout | 6 | 2 |  |  |
| Layout/MultilineMethodCallIndentation | 35 | 3 | 0/28 | discourse +28 |
| Layout/MultilineMethodDefinitionBraceLayout | 4 |  |  |  |
| Layout/MultilineMethodParameterLineBreaks | 0 |  |  |  |
| Layout/RedundantLineBreak | 23 | 1 | 0/14 | discourse +7, specs +7 |
| Layout/RescueEnsureAlignment | 17 |  |  |  |
| Layout/SingleLineBlockChain | 12 | 1 | 0/12 | discourse +11, rubocop-src +1 |
| Layout/SpaceAfterNot | 5 | 1 |  |  |
| Layout/SpaceAroundBlockParameters | 17 | 4 | 0/13 | specs +13 |
| Layout/SpaceAroundKeyword | 10 | 1 |  |  |
| Layout/SpaceAroundOperators | 0 |  |  |  |
| Layout/SpaceBeforeBlockBraces | 5 |  |  |  |
| Layout/SpaceInLambdaLiteral | 0 |  |  |  |
| Layout/SpaceInsideArrayLiteralBrackets | 17 | 1 |  |  |
| Layout/SpaceInsideBlockBraces | 39 | 2 |  |  |
| Layout/SpaceInsideParens | 0 |  |  |  |
| Layout/SpaceInsideReferenceBrackets | 14 | 2 |  |  |
| Layout/SpaceInsideStringInterpolation | 0 |  |  |  |

### Batch C6: Style, Style/AccessModifierDeclarations to Style/CombinableLoops (13 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Style/AccessModifierDeclarations | 39 | 3 |  |  |
| Style/Alias | 23 | 1 |  |  |
| Style/AmbiguousEndlessMethodDefinition | 7 | 1 |  |  |
| Style/AndOr | 8 | 1 |  |  |
| Style/ArgumentsForwarding | 35 | 3 |  |  |
| Style/Attr | 7 | 1 |  |  |
| Style/BitwisePredicate | 10 | 1 | 24/0 | brew -2, chef -15, discourse -3, mastodon -2, rails -2 |
| Style/BlockDelimiters | 38 | 3 | 4/4 | specs -4, specs +4 |
| Style/CaseLikeIf | 9 | 2 | 0/1 | specs +1 |
| Style/ClassAndModuleChildren | 72 | 2 |  |  |
| Style/ClassMethodsDefinitions | 15 | 1 |  |  |
| Style/CollectionQuerying | 125 | 1 | 187/225 | discourse -60, discourse +68, fastlane -118, fastlane +146, jekyll -1, jekyll +1, rails -5, rails +7, specs -3, specs +3 |
| Style/CombinableLoops | 100 | 2 |  |  |

### Batch C7: Style, Style/ConcatArrayLiterals to Style/For (14 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Style/ConcatArrayLiterals | 65 | 1 |  |  |
| Style/ConditionalAssignment | 4 |  |  |  |
| Style/DisableCopsWithinSourceCodeDirective | 74 | 8 | 0/2 | specs +2 |
| Style/DocumentationMethod | 0 | 1 | 0/3 | fastlane +3 |
| Style/DoubleNegation | 11 | 2 | 0/2 | specs +2 |
| Style/EmptyLiteral | 2 | 1 |  |  |
| Style/EmptyMethod | 4 | 1 | 0/4 | specs +4 |
| Style/EmptyStringInsideInterpolation | 64 | 2 |  |  |
| Style/EndlessMethod | 77 | 9 |  |  |
| Style/ExactRegexpMatch | 53 | 1 | 0/1 | specs +1 |
| Style/ExplicitBlockArgument | 60 | 3 |  |  |
| Style/FetchEnvVar | 0 |  |  |  |
| Style/FloatDivision | 24 | 1 | 0/1 | fastlane +1 |
| Style/For | 55 | 4 | 5/5 | specs -5, specs +5 |

### Batch C8: Style, Style/FormatString to Style/ItBlockParameter (14 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Style/FormatString | 34 | 2 | 11/11 | chef -1, chef +1, discourse -4, discourse +4, rails -5, rails +5, specs -1, specs +1 |
| Style/FrozenStringLiteralComment | 3 | 1 |  |  |
| Style/GuardClause | 53 | 2 |  |  |
| Style/HashAsLastArrayItem | 4 |  |  |  |
| Style/HashConversion | 36 | 1 |  |  |
| Style/HashLookupMethod | 41 | 4 | 0/257 | brew +215, chef +4, discourse +11, fastlane +1, mastodon +13, rails +2, rubocop-src +3, specs +8 |
| Style/HashSyntax | 19 | 7 |  |  |
| Style/IdenticalConditionalBranches | 25 | 1 |  |  |
| Style/IfInsideElse | 22 | 1 | 2/0 | specs -2 |
| Style/IfUnlessModifier | 58 | 3 |  |  |
| Style/IfUnlessModifierOfIfUnless | 4 |  |  |  |
| Style/IfWithSemicolon | 51 | 1 |  |  |
| Style/InverseMethods | 26 | 1 |  |  |
| Style/ItBlockParameter | 98 | 2 |  |  |

### Batch C9: Style, Style/KeywordParametersOrder to Style/MutableConstant (15 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Style/KeywordParametersOrder | 4 |  |  |  |
| Style/Lambda | 35 | 4 |  |  |
| Style/LambdaCall | 9 | 2 |  |  |
| Style/MapIntoArray | 3 |  |  |  |
| Style/MethodCallWithArgsParentheses | 27 | 6 |  |  |
| Style/MethodCallWithoutArgsParentheses | 29 | 1 | 0/2 | specs +2 |
| Style/MethodDefParentheses | 32 | 1 |  |  |
| Style/MissingElse | 8 | 1 | 0/2112 | brew +209, chef +203, discourse +1240, fastlane +95, jekyll +5, mastodon +7, rails +262, rubocop-src +73, specs +18 |
| Style/MixinGrouping | 15 | 2 |  |  |
| Style/MultilineIfModifier | 50 | 1 |  |  |
| Style/MultilineInPatternThen | 7 | 1 |  |  |
| Style/MultilineMethodSignature | 7 | 2 | 0/11 | brew +2, discourse +1, specs +8 |
| Style/MultilineTernaryOperator | 10 | 1 |  |  |
| Style/MultilineWhenThen | 7 | 1 |  |  |
| Style/MutableConstant | 59 | 5 |  |  |

### Batch C10: Style, Style/NegatedIfElseCondition to Style/RedundantSelfAssignmentBranch (26 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Style/NegatedIfElseCondition | 3 | 1 |  |  |
| Style/NegativeArrayIndex | 10 | 1 |  |  |
| Style/NestedModifier | 35 | 1 |  |  |
| Style/NestedTernaryOperator | 34 | 1 |  |  |
| Style/Next | 34 | 2 |  |  |
| Style/NilComparison | 20 | 1 |  |  |
| Style/NonNilCheck | 2 | 1 |  |  |
| Style/Not | 15 | 1 |  |  |
| Style/NumericPredicate | 26 | 3 | 1/12 | chef +6, discourse +1, rails +2, specs -1, specs +3 |
| Style/OneLineConditional | 15 | 2 |  |  |
| Style/ParenthesesAroundCondition | 2 |  |  |  |
| Style/PercentLiteralDelimiters | 17 | 1 | 0/13 | rails +10, specs +3 |
| Style/RaiseArgs | 28 | 3 |  |  |
| Style/ReduceToHash | 18 | 1 |  |  |
| Style/RedundantBegin | 26 | 2 |  |  |
| Style/RedundantCondition | 43 | 1 |  |  |
| Style/RedundantDoubleSplatHashBraces | 6 |  |  |  |
| Style/RedundantException | 18 | 1 |  |  |
| Style/RedundantFetchBlock | 10 | 2 |  |  |
| Style/RedundantFreeze | 14 | 1 |  |  |
| Style/RedundantInterpolationUnfreeze | 25 | 1 |  |  |
| Style/RedundantParentheses | 13 | 1 | 0/6 | specs +6 |
| Style/RedundantRegexpArgument | 7 | 2 |  |  |
| Style/RedundantRegexpConstructor | 20 | 1 |  |  |
| Style/RedundantRegexpEscape | 4 |  |  |  |
| Style/RedundantSelfAssignmentBranch | 28 | 1 | 0/1 | rails +1 |

### Batch C11: Style, Style/RedundantSort to Style/YodaExpression (26 cops)

| cop | lib lines | spec variants failing | corpus -rubocop/+rbcop | per corpus |
|---|---|---|---|---|
| Style/RedundantSort | 42 | 1 |  |  |
| Style/RegexpLiteral | 99 | 1 |  |  |
| Style/RequireOrder | 33 | 1 |  |  |
| Style/RescueModifier | 20 | 1 |  |  |
| Style/ReturnNil | 19 |  |  |  |
| Style/ReturnNilInPredicateMethodDefinition | 12 | 1 |  |  |
| Style/Sample | 17 | 1 |  |  |
| Style/SelfAssignment | 27 | 1 |  |  |
| Style/Semicolon | 21 | 2 |  |  |
| Style/SendWithLiteralMethodName | 4 |  |  |  |
| Style/SingleLineMethods | 25 |  |  |  |
| Style/SoleNestedConditional | 0 |  |  |  |
| Style/StabbyLambdaParentheses | 34 | 1 |  |  |
| Style/StringConcatenation | 12 | 1 |  |  |
| Style/StructInheritance | 6 |  |  |  |
| Style/SuperArguments | 3 | 1 |  |  |
| Style/SuperWithArgsParentheses | 0 |  |  |  |
| Style/SymbolProc | 30 | 3 | 69/69 | discourse -38, discourse +38, mastodon -11, mastodon +11, rails -15, rails +15, specs -5, specs +5 |
| Style/TernaryParentheses | 24 | 1 | 1/1 | specs -1, specs +1 |
| Style/TrailingCommaInArguments | 0 |  |  |  |
| Style/TrailingCommaInArrayLiteral | 0 |  |  |  |
| Style/TrailingCommaInHashLiteral | 0 |  |  |  |
| Style/TrailingUnderscoreVariable | 40 | 1 | 2/2 | specs -2, specs +2 |
| Style/WhileUntilModifier | 2 | 2 |  |  |
| Style/YodaCondition | 33 | 2 |  |  |
| Style/YodaExpression | 11 | 2 |  |  |

## Non-cop batches

Files are RuboCop's (`lib/rubocop/...`, lines changed from docs/upgrade-1.92.1-lib.tsv); the rbcop
side is in docs/upgrading-rubocop.md step 3.

### Batch L1: command line, options, `--explain`

`options.rb` (55), `cli.rb` (5), `cli/command/cop_names.rb` (new, 57), `cli/command/explain.rb`
(new, 34), `cli/command/execute_runner.rb` (4), `cli/command/show_cops.rb` (6),
`cli/command/show_docs_url.rb` (10), `offense_limit.rb` (new, 30, `--max-offenses-per-cop`),
`cop/cop_explanation.rb` (new, 140), `cop/cop_documentation.rb` (new, 81), `cop/documentation.rb`
(2), `cop/badge.rb` (2), `cop/registry.rb` (2), `version.rb` (2), `rubocop.rb` (5). rbcop side:
`src/main.rs` option table and the verbatim `rubocop -h`. Tests: formatters `help`,
`show-docs-url`.

### Batch L2: runner, caches, target Ruby, changed files

`runner.rb` (150), `result_cache.rb` (33), `cached_data.rb` (54), `server/cache.rb` (12),
`changed_files.rb` (48), `target_ruby.rb` (30), `util.rb` (69), `remote_config.rb` (5). Tests:
formatters `parallel-ignored`; re-check `tests/cache.rs`, `tests/changed.rs`,
`tests/remote_config.rs`, `tests/server.rs`.

### Batch L3: offenses, team, formatters

`cop/offense.rb` (62), `cop/base.rb` (63), `cop/autocorrect_logic.rb` (15), `cop/team.rb` (15),
`formatter/json_formatter.rb` (72, corrections and per-file errors), `formatter/sarif_formatter.rb`
(19), `formatter/base_formatter.rb` (22), `formatter/formatter_set.rb` (4),
`formatter/pacman_formatter.rb` (4), `formatter/offense_count_formatter.rb` (4),
`formatter/worst_offenders_formatter.rb` (4). Tests: formatters json/sarif/pacman cases,
`fallback_forwards_the_command_line`, autocorrect `plugin_directives`, `redundant_disable_diff`.
The `[Correctable]` flips in `fuzz_regressions`, `it_block_lvar` and `numblock_itblock_not_block`
(Layout/ClosingParenthesisIndentation, Style/CombinableLoops) may come from `autocorrect_logic.rb`
or `base.rb` rather than the cops; check here first.

### Batch L4: config loading, validation, directives

`config_loader.rb` (9), `config_loader_resolver.rb` (49), `config_validator.rb` (11),
`config_obsoletion/removed_cop.rb` (2), `plugin/configuration_integrator.rb` (17),
`comment_config.rb` (25). Tests: scenarios `config_obsolete_removed`, `pending_cops_qualified`,
`pending_cops_qualified_order`, `directive_no_department_kinds`; autocorrect `transcoded_sources`
(re-recorded output changed, check whether it is source loading or the cops).

### Batch L5: LSP and MCP

`lsp.rb` (7), `lsp/routes.rb` (46), `lsp/runtime.rb` (58), `lsp/stdin_runner.rb` (16),
`mcp/server.rb` (272), `mcp/autocorrection_request.rb` (new, 52), `mcp/explain_tool.rb` (new, 68),
`mcp/scope.rb` (new, 65), `mcp/source_files.rb` (new, 35). Tests: `tests/lsp` `modes`, all three
`tests/mcp` transcripts. Depends on L1 (`--explain`) for the explain tool.

### Batch L6: correctors and variable force

`cop/correctors/alignment_corrector.rb` (14), `each_to_for_corrector.rb` (44),
`empty_line_corrector.rb` (2), `lambda_literal_to_method_corrector.rb` (29),
`multiline_literal_brace_corrector.rb` (34), `parentheses_corrector.rb` (53),
`cop/variable_force/assignment.rb` (2), `cop/variable_force/variable.rb` (15). Users: Style/For
(C7), Style/Lambda (C9), Layout multiline brace layouts (C4, C5), Style/RedundantParentheses and
friends (C10), Lint/UselessAssignment (C2).

### Batch L7: mixins

`cop/mixin/check_line_breakable.rb` (42), `check_single_line_suitability.rb` (10),
`code_length.rb` (12), `empty_lines_around_body.rb` (13), `endless_method_body_help.rb` (new, 105),
`forbidden_identifiers.rb` (4, Naming/MethodName in `list_settings_string_sweep`),
`hash_alignment_styles.rb` (29), `hash_shorthand_syntax.rb` (28), `min_branches_count.rb` (4),
`multiline_element_line_breaks.rb` (12), `multiline_literal_brace_layout.rb` (9),
`ordered_gem_node.rb` (2), `pattern_matching_help.rb` (new, 35),
`preceding_following_alignment.rb` (19), `project_index_help.rb` (15),
`reparsed_equivalence.rb` (16), `statement_modifier.rb` (29), `visibility_help.rb` (3),
`alignment.rb` (4), `cop/mixin.rb` (2). Grep every caller of the matching `src/helpers/` code before changing
one; several cop batches depend on this one (Style/EndlessMethod, Layout/HashAlignment,
Style/HashSyntax, Style/IfUnlessModifier, Layout/RedundantLineBreak).

`cop/style/method_call_with_args_parentheses/omit_parentheses.rb` (93) goes with
Style/MethodCallWithArgsParentheses in its cop batch. The four `cop/internal_affairs/` files need
nothing unless rbcop implements those cops.

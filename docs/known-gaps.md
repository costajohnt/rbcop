# Known gaps and deliberate differences

Tracked so the review phase can close or document each one. "Deliberate" items are intended.

## Deliberate differences from RuboCop

- `-o/--out` pointing at a file being inspected is refused (RuboCop truncates the file).
- `-o/--out` (or anything that could become it) in a `.rubocop` arguments file is refused.
- HTML formatter escapes `& < >` in messages outside code spans and fully escapes paths; GitHub
  Actions formatter escapes CR/LF in the `file=` property. Output differs only when those appear.
- JUnit escapes cop names in attributes (no effect for real cop names).
- Style/RedundantFormat evaluates `format` at lint time; a width, precision or result over 10,000
  bytes (`format('%999999999d', 1)`) gets no offense instead of building a huge string
  (`MAX_FORMAT_OUTPUT` in `src/cops/style/redundant_format.rs`).
- Lint/UselessTimes does not correct a block that assigns its argument with a value (`1.times do |i|
  i = i * 2 ... end`). RuboCop's `block_reassigns_arg?` matches only a valueless `(lvasgn :i)`
  (masgn target, op-assign), so it inlines the body with `\bi\b` replaced by `0` and writes
  `0 = 0 * 2`, a syntax error that its next -A iteration reports as Lint/Syntax and stops on. rbcop
  treats `i = ...` as a reassignment and reports the offense without `[Correctable]`. Also reached
  through other cops: Style/ParallelAssignment turning `i, j = i * 2, i * 3` into `i = i * 2`
  (corpus `specs/lint/useless_times/023.rb`, skipped by the autocorrect gates).

## Open gaps

- Target Ruby below 3.3: RuboCop parses with the parser gem for that version and reports syntax newer
  than the target (e.g. `def x(&)` under 2.7) as Lint/Syntax; rbcop parses with Prism's 3.3 grammar and
  does not. Messages for genuine syntax errors also come from Prism, not the parser gem.
- `--stdin` together with `-a/-A` (RuboCop prints the corrected source) is refused.
- `--disable-uncorrectable`, `--auto-gen-config`, `--server`, `--lsp`, `--cache` behaviour (accepted
  and ignored where harmless), custom formatter classes, `require:` of local Ruby files (custom cops).
- ERB in `.rubocop.yml` is not evaluated; remote `inherit_from` URLs are skipped with a warning.
- Deliberate: Style/WordArray correcting a `%W[...]` whose value is invalid UTF-8 (`%W[\xff]`)
  writes the escaped form (`["\xFF"]`); RuboCop writes the raw bytes and then crashes re-parsing
  its own output. The offense message matches RuboCop.
- Review-loop security audit to run: every place a cop evaluates source-derived values at lint
  time (format strings, numeric literals and arithmetic, string building, regexes compiled from
  source, recursion depth on deeply nested code) must be bounded, so linting an untrusted file
  cannot exhaust CPU or memory. First finding: Style/RedundantFormat padded `%999999999d` to ~1 GB.
- Review-loop coverage to add: every corpus targets Ruby 3.x, so behaviour at TargetRubyVersion
  2.x (parser-gem grammar differences, min/max ruby cops) is only covered by scenarios. Record a
  specs-corpus oracle at 2.5 and 2.7 and compare.
- `AllCops/UseProjectIndex` (Rubydex, off by default) is not supported: Lint/DuplicateMethods
  cross-file checks and Naming/AccessorMethodName / PredicatePrefix `overrides_inherited_method?`
  are skipped; Lint/ArgumentMismatch, DeprecatedReference, NameTypo and SuperArgumentMismatch
  (index-only) never report, and Lint/ConstantReassignment's cross-file message is skipped. This
  matches RuboCop with the default config.
- Metrics/CollectionLiteralLength does not count `rescue A, B` exception lists (parser `array`).
  Lint/SymbolConversion does not treat a backslash-newline inside `:"..."` as a continuation. Naming/PredicatePrefix `validate_config` is not ported.
- LineLength `SplitStrings: true` break points (string splitting) are not modelled.
- Cleanup pending: deprecated shims `helpers/alignment_mixin.rs`, `alignment_check.rs`,
  `macro_scope_nodes.rs`, `range_help::with_surrounding_space` (no callers; delete once all batches
  that branched before R2a are merged). `helpers/variable_force.rs` and `nil_receiver_checker.rs`
  each build a parser-shaped tree from Prism; share one builder. Private `parser_parent` copies in
  lint/ambiguous_operator, lint/constant_definition_in_block, lint/redundant_splat_expansion,
  layout/first_argument_indentation, style/access_modifier_declarations, style/double_negation and
  style/nil_comparison should move to `helpers::parser_parent` (check each copy's semantics first).
  ENUMERATOR_METHODS is duplicated in lint/unreachable_loop and style/next. Parser-ancestor
  builders now exist in `parser_parent`, `parser_path`, `variable_force` and `nil_receiver_checker`;
  converge them on one. Style/SoleNestedConditional has an inline `correction_parses?`; switch it to
  `helpers::reparsed_equivalence`. Layout/EmptyLinesAfterModuleInclusion copies the blank-line and
  enable-directive closures of EmptyLinesAroundAttributeAccessor; share them.
  `helpers::statement_modifier` copies Layout/LineLength's URI regex and validator; share them.
  Private copies to switch to helpers: AllowedReceivers in style/hash_each_methods
  (`helpers::allowed_receivers`); `float_to_s` in lint/literal_in_interpolation
  (`ruby_string::float_to_s`); add_parentheses logic in style/method_def_parentheses
  (`add_parentheses::ParenFix::Params`); `value_used?` in style/alias and style/block_delimiters
  (`helpers::value_used`); StructInheritance's DefinesConstants visitor copied in
  style/data_inheritance; the runner's `code_lines`/`StatementBounds` duplicated in
  `helpers::comment_config`. Style/RedundantParentheses keeps its own `verified_by_reparse`
  (switch to `reparsed_equivalence::verified_by_reparse`); Bundler/GemComment ports comment
  association privately (switch to `helpers::comment_association`).
- Style/DirectiveScope treats only ported cops as disabled by default config; an `enable X` ...
  `disable X` pair for an unported, default-disabled cop can differ (not hit by the gates).
- One spec snippet (`naming/method_name/050.rb`, a `%{}` template) gets a duplicated
  "unexpected write target" syntax error location.
- Lint/RedundantCopDisableDirective, Lint/RedundantCopEnableDirective, Lint/MissingCopEnableDirective
  and Style/DisableCopsWithinSourceCodeDirective need every other cop's offenses (suppressed ones
  included), so they are ported last, once all cops exist.

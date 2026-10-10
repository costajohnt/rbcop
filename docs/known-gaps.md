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
- A file whose AST is nested more than 1,000 levels deep (`runner::MAX_AST_DEPTH`; the deepest
  corpus file is 46) is not inspected: rbcop prints `rbcop: <file>: nested more than 1000 levels
  deep, not inspected.` and counts it as an error (exit 1). RuboCop dies with SystemStackError on
  such files (10k nested arrays); many cops and helpers recurse over the tree. Prism itself rejects
  10k nested expressions, but left-associative chains (`a.b.b...`, `a + a + ...`) have no limit.
- Regexps nested more than 1,000 groups or character sets deep are treated as unparsable by
  `helpers::regexp_parser` (the regexp_parser gem parses them), so cops that use it report nothing
  for that regexp. Only interpolated regexps get there: Prism rejects deeper plain literals.
- Lint/LiteralInInterpolation reports no offense for an integer literal over 1,000 32-bit words
  (about 9,600 decimal digits): converting it to decimal is quadratic (`"#{0xfff...}"` with 200k
  hex digits took 2.6 s).
- Lint/UselessTimes leaves the offense uncorrected when the block argument's name is too long for
  the regex size limit (about 100k characters) instead of failing the cop.

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
- Lint-time cost audit (done; `tests/hostile_inputs.rs` runs the generated inputs). What remains
  is size-driven, not value-driven, and shared with RuboCop's algorithms:
  - Prism is quadratic on long `a && a && ...` chains (100k operands: 16 s, Ruby's Prism 21 s)
    and on huge decimal integer literals (800k digits: 0.85 s per parse; token cops lex again).
  - `-A` on code nested hundreds of levels deep runs up to 200 correction iterations over text
    that grows with depth squared (300 nested one-line `def`s: about 50 s before the infinite-loop
    error). Inspection alone stays under a second at the depth limit.
  - Lint/UselessAssignment's did-you-mean is O(useless assignments x variables) per scope
    (20k distinct variables in one scope: over a minute).
- Regex audit (done): regexes built from source or config either use `ruby_regex` /
  `ruby_regex_classes` (Ruby's ASCII `\s \w \d`) or `regex::escape` a literal name. Fixed:
  `directives::DIRECTIVE`, Lint/CopDirectiveSyntax (also its `lstrip`/`split`/`strip`, which are
  ASCII-only in Ruby) and `statement_modifier::first_line_comment` matched Unicode whitespace or
  word characters (`# rubocop:disable`, `# rubocop:disable Foo/Baré`). Checked equal to
  RuboCop: Lint/UselessTimes and Style/DirectiveScope (Ruby's `\b` is Unicode-aware, like the
  regex crate's), Naming/PredicatePrefix (RuboCop interpolates NamePrefix unescaped too, behind the
  same `start_with?` check), Style/NumericLiterals (AllowedPatterns are already `ruby_regex`),
  Naming/InclusiveLanguage, Style/ConditionalAssignment, Style/MagicCommentFormat (static).
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

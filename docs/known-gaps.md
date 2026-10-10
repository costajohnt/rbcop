# Known gaps and deliberate differences

Tracked so the review phase can close or document each one. "Deliberate" items are intended.

## Deliberate differences from RuboCop

- `-o/--out` pointing at a file being inspected is refused (RuboCop truncates the file).
- `-o/--out` (or anything that could become it) in a `.rubocop` arguments file is refused.
- HTML formatter escapes `& < >` in messages outside code spans and fully escapes paths; GitHub
  Actions formatter escapes CR/LF in the `file=` property. Output differs only when those appear.
- JUnit escapes cop names in attributes (no effect for real cop names).

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
- `AllCops/UseProjectIndex` (Rubydex, off by default) is not supported: Lint/DuplicateMethods
  cross-file checks and Naming/AccessorMethodName / PredicatePrefix `overrides_inherited_method?`
  are skipped. Naming/PredicatePrefix `validate_config` is not ported.
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
  converge them on one.
- One spec snippet (`naming/method_name/050.rb`, a `%{}` template) gets a duplicated
  "unexpected write target" syntax error location.
- Lint/RedundantCopDisableDirective, Lint/RedundantCopEnableDirective, Lint/MissingCopEnableDirective
  and Style/DisableCopsWithinSourceCodeDirective need every other cop's offenses (suppressed ones
  included), so they are ported last, once all cops exist.

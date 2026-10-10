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

- Target Ruby below 3.3 (RuboCop parses with the parser gem's grammar for the target):
  `src/legacy_syntax.rs` reports the parser gem's Lint/Syntax for syntax newer than the target and
  for genuine syntax errors, approximating racc's error recovery for the diagnostics after the first
  (module doc). Oracles with `config/all-cops.yml` + TargetRubyVersion, diff lines left (specs: all
  low targets; rails/discourse: 2.7 and 3.1): specs 2.5 208, 2.7 61, 3.0 60, 3.1 59, 3.2 59; rails
  2.7 14 (8 files), 3.1 3; discourse 2.7 271 (76 of 12609 files), 3.1 5. What remains:
  - Follow-up diagnostics after the first error in a file: recovery is modelled, not the LALR
    tables, so cascades can differ, mostly after a def popped by recovery (`class/module definition
    in method body`, `dynamic constant assignment`), after `yyerror`s from semantic actions, and in
    `case/in` bodies below 2.7 (pattern syntax parsed as statements).
  - Parser-gem lexer errors (`bare backslash only allowed before newline`, `` unexpected `@' ``,
    `` `$%' is not allowed ``, ...) where Prism reports a parse error instead; `circular argument
    reference` is reported at the default value's read but not again at reads in the method body.
  - Below 2.3 nothing lexer-level is checked (`&.`, `<<~`, `"a": 1`, `1r`, required keyword
    arguments are accepted); no oracle below 2.5 was recorded.
  - Not syntax: numbered parameters below 2.7 are method calls for the parser gem (`_1` cops such as
    Style/SymbolProc, Lint/UnexpectedBlockArity, Lint/Void differ: most of the 157 non-Lint/Syntax
    lines in specs at 2.5, every sampled one `_1`-related);
    `a, b = 1, 2 rescue x` and Style/RescueModifier below 2.6; Lint/AmbiguousOperator's parser-gem
    warning for `foo &:sym` (5 lines per target in specs); `#$-1` interpolation (rails 3.1); an empty
    heredoc delimiter `<<''` at end of file is a Prism error but valid for the parser gem.
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
- Review-loop regex audit: every regex built from config or source must go through
  `config::value::ruby_regex` (Ruby class semantics) or `regex::escape` (literal names). Check the
  cops that call `Regex::new(&...)` directly: lint/cop_directive_syntax, lint/useless_times,
  layout/line_length, style/numeric_literals, style/directive_scope, style/conditional_assignment,
  style/magic_comment_format, naming/predicate_prefix, naming/inclusive_language.
- `AllCops/UseProjectIndex` (Rubydex, off by default) is not supported: Lint/DuplicateMethods
  cross-file checks and Naming/AccessorMethodName / PredicatePrefix `overrides_inherited_method?`
  are skipped; Lint/ArgumentMismatch, DeprecatedReference, NameTypo and SuperArgumentMismatch
  (index-only) never report, and Lint/ConstantReassignment's cross-file message is skipped. This
  matches RuboCop with the default config.
- Metrics/CollectionLiteralLength does not count `rescue A, B` exception lists (parser `array`).
  Lint/SymbolConversion does not treat a backslash-newline inside `:"..."` as a continuation. Naming/PredicatePrefix `validate_config` is not ported.
- LineLength `SplitStrings: true` break points (string splitting) are not modelled.
- Cleanup pending: Style/SoleNestedConditional has an inline `correction_parses?` that parses with
  Prism's default (latest) grammar; `reparsed_equivalence::correction_parses` parses at the target
  version like RuboCop, so switching is a behaviour change to gate on its own, not a pure dedupe.
- Look-alike code kept on purpose (different semantics, not duplicates):
  - Parser-parent walkers: lint/constant_definition_in_block treats a multi-statement body inside
    parentheses as one parser `begin` (the parentheses), where `helpers::parser_parent` stops at the
    StatementsNode; style/double_negation builds the whole ancestor chain with cop-specific frames
    (begin end offsets, `define_method` blocks, interpolation); style/access_modifier_declarations
    classifies its parent with statement-list details (index, header bound, `kwbegin`);
    layout/first_argument_indentation returns no parent for a call owning a block, then defers to
    the helper.
  - `value_used?` look-alikes: style/alias ports `alias_method_value_used?` (send argument or
    assignment value) and style/block_delimiters ports its own `return_value_used?` (through
    `begin`s to an assignment or send); neither is rubocop-ast `Node#value_used?`.
  - Bundler/GemComment already takes leading comments from `helpers::comment_association`; its
    private `Associate` walker ports the rest of `Comment::Associator` (trailing and interior
    comments of the gem call's descendants), which the helper does not cover.
  - Naming/BlockForwarding's `LocalUse` is `each_node` (the body root counts) and skips only an
    lvar under block_pass; `helpers::local_references` is `each_descendant` (root excluded) and
    skips lvars under splat, kwsplat and block_pass.
  - Parser-shaped views of Prism: `helpers::parser_parent` (nearest parser parent, an index into
    `cx.ancestors`), `ast::parser_ancestors`/`parser_ancestor_ranges` (ancestor frames), and
    `helpers::parser_path` (send/csend/block/begin frames with names and call operators for a
    descendant found below the visited node, splitting a call with a block into block + send) answer
    "what is above this node" and differ in how they classify `begin` (parser_path counts
    parentheses and multi-statement bodies; `ast::frame` only an explicit `begin`). The tree builders in
    `helpers::variable_force` (whole program, only scopes, local variables and control flow, other
    nodes hoisted away) and `helpers::nil_receiver_checker` (one subtree, every node with children,
    receivers, arguments, when/pair/asgn/rescue shapes and block argument names) build different
    node sets with different fields. One shared full-fidelity parser tree would serve both, but it
    is a rewrite of two large ports rather than a dedupe; the ancestor helpers work on
    `cx.ancestors` directly, so moving them onto a tree would add a tree build per file for cops
    that only need a parent.
- Style/DirectiveScope treats only ported cops as disabled by default config; an `enable X` ...
  `disable X` pair for an unported, default-disabled cop can differ (not hit by the gates).
- Lint/RedundantCopDisableDirective judges directives against rbcop's own offenses, so a directive
  for a cop rbcop has not ported yet (e.g. `# rubocop:disable Style/Send`) is reported as
  redundant.
- Department-level `NewCops` (a department's `NewCops: enable` or a version) is not read; only
  `AllCops: NewCops: enable` enables pending cops. The pending-cops notice RuboCop prints is not ported.
- JUnit lists testcases in sorted cop order; RuboCop uses its registry (load) order. The `-a` summary
  says "N offenses autocorrectable" where RuboCop says "N more offenses can be corrected with
  `rubocop -A`".

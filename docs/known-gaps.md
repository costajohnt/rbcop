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
- `-a/-A` writes a corrected file through a temporary file in the same directory renamed over it
  (`autocorrect::write_source`), so a crash or a full disk never leaves it truncated; RuboCop
  truncates and writes in place. Mode, group, symlinks (the target is written) and hard links (written
  in place) are kept, as are files owned by another user or in an unwritable directory (in place);
  extended attributes and ACLs are not copied to the renamed file. Files are written in the reporting
  loop, in target order, so an error or `--fail-fast` stops before later files change.
- Directory walks follow symlinked directories and keep symlinked files, as TargetFinder does, so
  `-a/-A` corrects files a project links to outside itself, exactly as RuboCop does (checked on a
  scratch project). A symlinked directory that is already an ancestor of the walked directory
  (same device and inode) is not entered: RuboCop's realpath prefix test misses two-link cycles
  (`d1/to2 -> ../d2`, `d2/to1 -> ../d1`) and lists the same files under ever longer paths until the
  OS returns ELOOP. The walk is iterative.
- An `inherit_from` cycle is an error (`... inherit_from cycle through ...`, exit 2); RuboCop recurses
  until SystemStackError.
- A panic outside a cop (cop panics are reported as cop errors) ends the run at that file with
  `rbcop: internal error while inspecting <file>` (exit 2).
- Error wording for I/O failures follows Ruby's (`Permission denied @ rb_sysopen - <file>`,
  `... @ rb_sys_fail_on_write - <out file>`) but without the backtrace; a failed `-o` directory or
  file creation is worded `<path>: <reason>`.

## Open gaps

- Target Ruby below 3.3 (RuboCop parses with the parser gem's grammar for the target):
  `src/legacy_syntax.rs` reports the parser gem's Lint/Syntax for syntax newer than the target and
  for genuine syntax errors, approximating racc's error recovery for the diagnostics after the first
  (module doc). Oracles with `config/all-cops.yml` + TargetRubyVersion, diff lines left (specs: all
  low targets; rails/discourse: 2.7 and 3.1): specs 2.5 207, 2.7 60, 3.0 59, 3.1 58, 3.2 58; rails
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
    Style/SymbolProc, Lint/UnexpectedBlockArity, Lint/Void differ: most of the 156 non-Lint/Syntax
    lines in specs at 2.5, every sampled one `_1`-related);
    `a, b = 1, 2 rescue x` and Style/RescueModifier below 2.6; Lint/AmbiguousOperator's parser-gem
    warning for `foo &:sym` (5 lines per target in specs); `#$-1` interpolation (rails 3.1); an empty
    heredoc delimiter `<<''` at end of file is a Prism error but valid for the parser gem.
- `--disable-uncorrectable`, `--auto-gen-config`, `--server`, `--lsp`, `--cache` behaviour (accepted
  and ignored where harmless), custom formatter classes, `require:` of local Ruby files (custom cops).
- ERB in `.rubocop.yml` is not evaluated; remote `inherit_from` URLs are skipped with a warning.
- A duplicated YAML key: the last value wins in the first key's place, as in Psych, but the
  `... is concealed by line N` warning is not printed. Wrong value types (`Enabled: "true"`) and
  invalid `Severity` values are not reported (see ConfigValidator below).
- Sources whose magic comment names another encoding (`# encoding: iso-8859-1`) are inspected with
  invalid UTF-8 bytes replaced; RuboCop transcodes them to UTF-8. They are never autocorrected.
  Sources that must be UTF-8 and are not get RuboCop's `Lint/Syntax: Invalid byte sequence in utf-8.`
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
- ConfigValidator is ported only for `NewCops` (AllCops and department values). Its other checks
  (unrecognized cops or parameters, obsolete config, EnforcedStyle values, Lint/Syntax config) and
  their warnings and errors are not; with `-f` of a bogus formatter RuboCop also prints a Ruby
  backtrace rbcop does not.
- The pending-cops notice treats any `Enabled: pending` key with a department as a registered cop;
  RuboCop would have rejected an unknown cop name earlier, which rbcop does not validate.
- YAML: a plain `:name` value loads as a Symbol (`Value::Sym`) and a quoted `':name'` as a String,
  so `--show-cops` dumps them apart; mapping keys do not keep the distinction (a quoted `':q':` key
  is dumped as the symbol `:q:`).

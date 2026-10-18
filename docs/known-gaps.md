# Known gaps and deliberate differences

Tracked so the review phase can close or document each one. "Deliberate" items are intended.

## Deliberate differences from RuboCop

- `-o/--out` pointing at a file being inspected is refused (RuboCop truncates the file).
- `-o/--out` (or anything that could become it) in a `.rubocop` arguments file is refused.
- `-a`/`-A` never write a file whose canonical path (symlinks resolved) is outside the canonical
  working directory, a directory given as an argument, or a file given as an argument. RuboCop
  follows symlinked files and directories found while walking and rewrites their targets. Such a
  file is still inspected and reported as without `-a` (no `[Corrected]`), left unchanged, and
  `rbcop: not autocorrecting <file>: it resolves to <path>, outside the project` goes to stderr;
  the exit status follows its offenses. The check is repeated right before the write, which follows
  no symlink after it (directories are opened one by one, relative to the last). `--stdin` is unaffected.
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
- A config integer beyond 64 bits (`Max: 99999999999999999999`) loads as `i64::MAX` (or
  `i64::MIN`); Psych makes a Bignum. Cops behave the same (nothing reaches that size), but
  `--show-cops` and messages that quote the limit (`[14/-99999999999999999999]`) print the
  saturated value where RuboCop prints the digits as written.
- Config regexps the regex crate rejects (lookaround, backreferences, atomic groups) run on
  fancy-regex (`config::value::RubyRegex`), which has no match cache: a search is capped at 1M
  backtracking steps, then retried one start position at a time with 10k steps each, within a
  budget of 64 retried positions per call; a start position that exhausts its steps, and
  every position past the budget, counts as no match. The steps spent giving up are also summed
  per pattern and file (32 call budgets, about 2 s): past that the pattern answers no match in that file
  without searching, even where a search would have been cheap. Onigmo finishes such patterns
  (`(a|aa)+(?=c)` on a long run of `a`) and may find a match at a position rbcop gave up on.
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
- Cop errors (every kind rbcop reproduces: config values, AmbiguousCopName directives, the
  InflectorPath file, crashes) print only the `An error occurred while <Cop> cop was inspecting
  <location>.` line. RuboCop's Team#handle_error follows it with `To see the complete backtrace run
  rubocop -d.` (the backtrace under `-d`, which rbcop does not have). After the `N error(s)
  occurred:` list at the end of the run rbcop prints its own text,
  `Errors are usually caused by rbcop bugs.`, `Please report a bug with the file and configuration
  that triggered it.`, `Mention the following information in the issue report:`, `rbcop <version>`,
  where RuboCop prints `Errors are usually caused by RuboCop bugs.`, `Please, update to the latest
  RuboCop version if not already in use, and report a bug if the issue still occurs on this
  version.`, its issues URL, a blank line, `Mention the following information in the issue report:`
  and its `-V` line: bugs in rbcop's output belong to rbcop.
- Error wording for I/O failures follows Ruby's (`Permission denied @ rb_sysopen - <file>`,
  `... @ rb_sys_fail_on_write - <out file>`) but without the backtrace; a failed `-o` directory or
  file creation is worded `<path>: <reason>`.
- rubocop-rails corrections that splice string or symbol content from the file into new source are
  withheld when that content is not inert. RuboCop applies them unescaped and can write arbitrary
  code (`tag("div\nsystem('x')")` becomes `tag.div` and then `system('x')()`). Offenses and
  `[Correctable]` markers are unchanged. Under `-a`/`-A` such an offense stays `[Correctable]`
  (not `[Corrected]`) and the source is left as is. Corrections are applied only when:
  Rails/ContentTag: the tag name matches `\A[a-z_][a-zA-Z0-9_]*[!?]?\z`.
  Rails/DotSeparatedKeys: every key and scope part has no quote, backslash, `#{` or line break.
  Rails/LinkToBlank: an existing `rel:` is a plain `'...'`/`"..."` literal whose source equals its
  value with none of those characters, and a new `rel:` copies a `'`, `"` or `:` delimiter.
  Rails/RootPublicPath: the path after `public/` has none of those characters.
  Rails/MigrationClassName: the file name camelizes to `\A[A-Z][A-Za-z0-9_]*\z`.
  Rails/FilePath: path segments joined into one `"..."` (or split into `, "..."` arguments) and an
  extension moved into a literal are inert in that literal: no matching quote (the other quote is
  fine), backslash, `#{`/`#@`/`#$` (for `"..."`) or line break; source moved out of a
  `"#{Rails.root}/..."` string into a new `"..."` is corrected when that string is a plain `"..."`
  literal, or (a `%{...}` literal, a heredoc) when the moved text has no `"`, backslash or line break. `File.join` segments are re-escaped (`inspect`) and stay corrected.
  Rails/EnvironmentComparison: the compared value is an identifier (it becomes `Rails.env.<value>?`).
  Rails/RakeEnvironment: a symbol task name is a plain label (`\A[A-Za-z_][A-Za-z0-9_]*\z`).
  Rails/I18nLazyLookup: the key that goes into the new `'...'` has none of those characters.
  Rails/StrongParametersExpect: a string or symbol `require` key that becomes a `key:` label is an
  identifier.
  Rails/RootPathnameMethods: a `Dir.glob(Rails.root.join(...))` pattern joined from literal
  arguments is inert in its new quotes, by the same rule as Rails/FilePath.
  Other Rails corrections insert source text or escaped literals (`inspect`/`dump`) and are not
  changed. Fixture: tests/autocorrect/rails_untrusted_text (RuboCop's output is kept in
  `rubocop-output/`).
- Rails/RedundantActiveRecordAllMethod reports an `all` that is an argument of a querying method
  whose receiver is also `.all` (`Foo.all.merge(Bar.all)`), as RuboCop does, but withholds that
  correction: RuboCop removes the argument's `all` and the parent's dot and writes
  `Foo.merge(Bar.)` (a syntax error), or `where()` for `all.where(all)`. The receiver's `all` is
  corrected as usual; once it is, the argument is no longer reported (its parent's receiver is not
  `.all` any more), so under `-a`/`-A` rbcop reports only offenses that remain. Fixture:
  tests/autocorrect/rails_all_argument (RuboCop's output in `rubocop-output/`).
- rubocop-minitest corrections were checked against the same rule: none splices string or symbol
  content into new source (they move the source of whole arguments and receivers, and write method
  names and operators, which are identifiers), so none is withheld.
- rubocop-rspec, rubocop-factory_bot and rubocop-capybara corrections that splice string or symbol
  content into new source are withheld by the same rule (offenses and `[Correctable]` unchanged).
  Corrections are applied only when:
  RSpec/ExampleWording and RSpec/ExcessiveDocstringSpacing: the docstring text written back between
  its delimiters has no backslash, none of the delimiter's characters and (when it interpolates) no
  `#{`/`#@`/`#$`; a `'a' \ 'b'` concatenation is made of plain quoted literals with one quote.
  RSpec/ExpectChange (block style): the attribute is a method name.
  RSpec/MatchWithSimpleRegex: the text has no backslash, nor `#{`/`#@`/`#$` when it needs `"..."`.
  RSpec/MetadataStyle (hash style): the symbol is a plain label.
  RSpec/ReceiveMessages: every merged key is a plain label (otherwise the whole group is withheld).
  RSpec/SharedExamples: the text is a plain symbol name (symbol style) or has no quote or backslash
  (string style).
  RSpec/SingleArgumentMessageChain: the symbol key is a plain name.
  RSpec/VerifiedDoubleReference (constant style): the string is a constant name.
  FactoryBot/AssociationStyle (implicit style): the association name, keys and factory names are
  plain names.
  FactoryBot/CreateList (block form): the factory name is a plain name.
  Capybara/SpecificFinders: the id written into `'...'` has no quote; `class:` values are written
  with `inspect`; an array of other values is withheld.
  Capybara/RedundantWithinFind: the `find_by_id` string is a `'...'`/`"..."` literal whose value is
  inert in it (no matching quote, backslash, line break or interpolation).
  Capybara/RSpec/CurrentPathExpectation: a string turned into a regexp literal is printable ASCII
  without backslashes, `#` or `()[]{}*+?|` (so the regexp is always valid and its `inspect` is the
  text with `/` escaped); a backtick string has no backslash, backtick or `#`.
  Fixture: tests/autocorrect/rspec_untrusted_text (RuboCop's output in `rubocop-output/`).

## Open gaps

- A `require:`d gem's Ruby code that changes RuboCop's directive syntax is not run: standard's
  `standard/rubocop/ext.rb` widens `DirectiveComment::DIRECTIVE_COMMENT_REGEXP`, so its
  `# standard:disable Cop` comments are directives in RuboCop and plain comments in rbcop.
- Static regexp literals Onigmo rejects (RuboCop compiles each with `Regexp.new` and reports
  Lint/Syntax): `helpers::onigmo_compile` covers group names and references, repeat ranges,
  character class ranges, POSIX bracket names, `\k<+n>`, and look-behind bodies with quantifiers,
  backreferences, nested look-arounds or (under a negative look-behind) capture groups. Not
  modelled, so not reported: `/x` literals and `(?x)`, `\p{..}` names outside the common ones
  (`onigmo_compile::PROPERTY_NAMES`; an unknown name such as `\p{Foo}` stops the check, so a later
  error in the pattern is not reported either), `\x{..}`, `\c`/`\M-`/`\C-` escapes, multi-digit
  backrefs, relative calls `\g<+n>`, `\g<0>` (never ending recursion), invalid group-name
  characters, alternation of different lengths inside a look-behind's groups (`(?<=(a|bc))`). A
  pattern with both a bad numbered backref and a bad look-behind reports the backref; Onigmo
  reports whichever comes first. Below 3.3 the error does not start racc's
  recovery when nothing else in the file fails, so `if /a{3,2}/ ... end` misses the parser gem's
  follow-up `unexpected token kEND`. At 3.3+ RuboCop crashes on such a regexp in `=~` (the
  translation's `match_op`); rbcop reports the Lint/Syntax.
- `in - 1` (a sign, whitespace, a number in a pattern) below 3.3: the parser gem lexes it as
  `tUNARY_NUM` and accepts it, but Prism (and Ruby's own parse.y) reject it, so rbcop has no tree
  for the file and reports Prism's failure as Lint/Syntax (#126).
- `a&. &.b` (`&.` right after `&.`/`.`) below 3.3: the parser gem reads `(a&.&).b`, a call to `&`,
  and lints the file; Prism lexes `&.` as one token and rejects it (as MRI does), so rbcop
  reports Lint/Syntax (#232).

- Target Ruby below 3.3 (RuboCop parses with the parser gem's grammar for the target):
  `src/legacy_syntax.rs` reports the parser gem's Lint/Syntax for syntax newer than the target and
  for genuine syntax errors, approximating racc's error recovery for the diagnostics after the first
  (module doc). Below 2.7 the runner drops Prism's NumberedParametersNodes, so `foo { _1 }` is a
  block without arguments for every cop (`_1` reads stay LocalVariableReadNodes:
  `node_types::is_legacy_numbered_read` for cops that tell variables from sends). Oracles with
  `config/all-cops.yml` + TargetRubyVersion, diff lines left (specs: all low targets;
  rails/discourse: 2.7 and 3.1): specs 2.5 7, 2.7 1, 3.0 1, 3.1 1, 3.2 1; rails 2.7 0, 3.1 0;
  discourse 2.7 32 (11 of 12609 files), 3.1 1. What remains:
  - The first error's wording, token name or position on invalid code, when both tools reject the
    file: rbcop parses with Prism and maps its errors onto the parser gem's racc messages, but the
    two lexers and grammars fail at different places on arbitrary broken input (an unclosed `{`
    or `[` pattern, `BEGIN` inside a def, `a.{}`, `x.0`, a stray `:`, doubled or trailing commas,
    unary `+`/`-` at end of file, `class ::foo`, Prism's anonymous-parameter errors, a blank or
    comment line before the `)` of `foo (a` (the parser gem's second tNL, which Prism has no
    token for: reported at the next token)). Both tools
    report the file as unparseable and run no other cop on it, so the outcome is the same; only
    the Lint/Syntax message text or column differs. Differences in WHETHER code parses (rbcop
    accepting what the parser gem rejects, or the reverse) are bugs, not part of this gap.
  - Follow-up diagnostics after the first error in a file: recovery is modelled (including the
    lexer's cmdarg stack and in_kwarg flag left set by abandoned constructs, and racc's C core
    popping one state too many after a semantic action's yyerror), not the LALR tables, so some
    cascades still differ: whether racc gives up after an error at the top level (it does not when
    the failing token follows a bare name, as in `a ^ B c`, but does in other bare-name cases),
    `case/in` and `when` bodies after recovery, an `else` left by a popped `if` in a block, and
    template junk in the specs corpus at 2.5 (`def %{x}`, `case ... in` below 2.7).
  - Below 2.3 nothing lexer-level is checked (`&.`, `<<~`, `"a": 1`, `1r`, required keyword
    arguments are accepted); no oracle below 2.5 was recorded.
  - A `{` that opens a hash whose contents are invalid (`x == { |i| i }`, `{{#if x ~}}` where `#`
    starts a comment) gets its first error at the `{` instead of the token after it.
  - The chef corpus (target 2.7) has five syntax-error files with such differences (a missed
    follow-up `unterminated string meets end of file`; the `{{` template above):
    `scripts/check-skip.txt` leaves them out of check-cops.sh, and they and two RuboCop -A outputs
    with the same differences are in `scripts/autocorrect-skip.txt`.
- Command-line options rbcop refuses (`rbcop: --NAME is not supported by rbcop yet`, exit 2):
  - `--auto-gen-config`, `--regenerate-todo`, `-f autogenconf`: generating `.rubocop_todo.yml`
    (AutoGenerateConfig and the autogenconf formatter's Exclude/Max output) is not ported. Its
    companion options (`--exclude-limit`, `--[no-]offense-counts`, `--[no-]auto-gen-timestamp`,
    `--[no-]auto-gen-only-exclude`) are parsed and give RuboCop's `can only be used together with
    --auto-gen-config` error; `--no-exclude-limit` and `--[no-]auto-gen-enforced-style` are inert.
  - `--report-unused-todo-entries`, `--disable-uncorrectable`: need the todo file and
    `# rubocop:todo` comment insertion, not ported.
  - `--display-suppressed`: the runner drops offenses disabled by comments instead of reporting
    them with a disabled status.
  - `--diff`: the unified-diff dry run of autocorrection is not ported.
  - `--changed [REVISION]`: needs RuboCop's git diff of changed files.
  - `--server`, `--start-server`, `--stop-server`, `--restart-server`, `--server-status`,
    `--no-detach`, `--lsp`, `--mcp`: long-running server modes (`--no-server` is accepted).
  - `-r/--require`, `--plugin`: load Ruby code. In a config file, `plugins:` / `require:` of a
    plugin rbcop implements natively (docs/plugins.md) runs its cops; any other gem only merges its
    default configuration and its cops do not run. A `require:` of a gem that is not a lint_roller
    plugin is a plain `require`: rbcop reads the files that feature reaches through literal
    `require`/`require_relative` lines inside its gem (every file under `lib/` when one requires a
    computed path; none of the rubocop gem's own, which RuboCop has loaded already), registers the
    cop classes found there, and merges the gem's `config/default.yml` only when one of those files
    sets the default configuration (an extension's `Inject.defaults!`). Conditions on a require
    line are not evaluated.
  - `-d/--debug`, `--profile`, `--memory`: Ruby-level diagnostics (backtraces, stackprof).
  - `-C/--cache`, `--cache-root`, `-P/--[no-]parallel`, `--raise-cop-error` are accepted and have
    no effect (rbcop keeps no result cache and always lints in parallel).
- The cops of a loaded plugin rbcop does not implement (`--show-cops`, `--list-enabled-cops-for`,
  `--show-docs-url`, `--only`, directives, the skipped-cops notice) are the cop classes its gem's
  `lib/` defines, found by reading the `module`/`class` nesting (`RuboCop::Cop::<Dept>::<Name>`
  descending from a cop base class, without `exclude_from_registry`; `extend AutoCorrector` marks
  `# Supports --autocorrect`). A cop class defined by metaprogramming is not seen; a gem whose
  `lib/` yields no cop class falls back to its default.yml cop names, and then also counts any
  `Dept/Name` made of class or module names in its `lib/` as known. The same goes for a local
  `require:` file (the files it requires in turn are not read); one with no cop class rbcop can
  read leaves every name outside the known departments known.
- A loaded plugin rbcop does not implement that defines no cops (rubocop-erb, rubocop-md) acts
  through Ruby code run inside RuboCop (rubocop-erb extracts the Ruby from each ERB template;
  rubocop-md adds Markdown files to the targets). rbcop merges its default.yml (rubocop-erb's
  `**/*.erb` Include and its Exclude lists, Lint/Syntax's included), names the plugin in the
  not-running notice, and runs none of its cops on the files the plugin's `AllCops: Include`
  brings in: they count as inspected, with no offenses. With `RBCOP_FALLBACK=1` RuboCop runs every
  cop on those files instead. Files a plugin adds from Ruby (rubocop-md's `.md` files) are not
  inspected at all.
- Custom formatter classes and `require:` of local Ruby files (custom cops) are not supported. rbcop names
  the cops it skips on stderr, and `RBCOP_FALLBACK=1` runs them with RuboCop (README, "Plugins and custom cops").
- Cops rbcop cannot run (a non-native plugin's, custom cops): Lint/RedundantCopDisableDirective
  never reports a directive naming one, nor a `disable all` while any is loaded, since their
  offenses are unknown (RuboCop reports those that turn out redundant). With custom cops loaded, a
  directive naming a cop in a department rbcop does not know is not reported as unknown.
- A native plugin uses its vendored default.yml and pinned behaviour whatever version of the gem is
  installed, and works when the gem is not installed (RuboCop fails to load it then).
- ERB in `.rubocop.yml` is not evaluated; remote `inherit_from` URLs are skipped with a warning.
- A duplicated YAML key: the last value wins in the first key's place, as in Psych, but the
  `... is concealed by line N` warning is not printed. Wrong value types (`Enabled: "true"`) and
  invalid `Severity` values are not reported (see ConfigValidator below).
- Sources whose magic comment names another encoding (`# encoding: iso-8859-1`) are inspected with
  invalid UTF-8 bytes replaced; RuboCop transcodes them to UTF-8. They are never autocorrected.
  The same holds for binary (`# encoding: BINARY`) sources with such bytes, where each replacement
  also counts as 3 bytes in later columns of its line (RuboCop: 1 per byte), e.g. the end of
  Lint/PercentSymbolArray's range on `%i[\xC0 :foo]` (`scripts/cold-skip.txt`).
  Sources that must be UTF-8 and are not get RuboCop's `Lint/Syntax: Invalid byte sequence in utf-8.`
  RuboCop's transcoding error is reproduced only for US-ASCII (`"\xe3" on us-ascii.`); a source
  in another encoding with a byte sequence invalid there (`# encoding: euc-jp`) is inspected as
  UTF-8 instead.
  In a binary source a one-character range on a multibyte character (Layout/
  EmptyLineAfterMagicComment's `source_range(buffer, line, 0)`) is one byte for RuboCop and the
  whole character for rbcop.
- RuboCop's formatters abort the whole run (`invalid byte sequence in UTF-8` from `String#tr`)
  when an offense message quotes invalid UTF-8 (from a non-UTF-8 source, or a value such as
  `%W(a\255)`'s in Style/WordArray's message); rbcop reports the file and goes on.
- A unary minus separated from a numeric literal (`- 0.09e3`, `-\n1`) is one negative literal for
  RuboCop (the translation layer's `numeric_negate`, its range starting at the `-`) and a `-@` call
  on the literal for rbcop's cops: ranges and sources of numeric-literal cops differ there
  (Style/ExponentialNotation reports at the literal, not the `-`).
- RuboCop cop crashes on odd input are not reproduced: RuboCop prints `An error occurred while
  ...` and exits 1 where rbcop's port inspects the file normally (Layout/EmptyLinesAroundMethodBody
  on `def foo(bar` / `    )end`, Style/NegativeArrayIndex, Style/HashConversion, Style/RequireOrder).
  Cop errors from config values (MinBodyLength / MinBranchesCount not a positive Integer,
  SafeNavigationChainLength `Max` 0 or less, a zero indentation width with tabs, #114) are
  reproduced, and so are list settings given as a YAML String (#440, #474): each cop reads one as
  its RuboCop counterpart does, a substring test where the cop calls `include?` on the bare
  setting, a cop error where it calls an Array-only method (`map`, `any?`, `each`, `join`, `+`,
  `-`), a one-element list where it wraps the setting in `Array()`. A list set to `~` (#500) is
  removed by the merge: a cop that reads it bare (`cop_config[key]`) raises on nil, one that reads
  it with `fetch(key, default)` or `|| []` gets the default. A String
  Naming/PredicatePrefix `ForbiddenPrefixes` ends the run (exit 2) with RuboCop's
  `undefined method 'each' for an instance of String`, without the backtrace. Order of cop errors: rbcop's `N errors occurred:` summary is sorted, RuboCop's is in
  raise order; and errors printed as they happen come from on_file cops (RuboCop's
  on_new_investigation) after the node walk's, where RuboCop prints them first. A directive naming
  a bare cop name two departments share (AmbiguousCopName) ends the run at its file as in RuboCop,
  with the same cop errors and `no department given` warnings. The VariableForce cops (Lint/UselessAssignment, the unused-argument cops,
  ...) run as one force in RuboCop, so only the first hook to add an offense errors, without a
  location: rbcop picks that cop by the earliest force event (a declaration for
  Lint/ShadowingOuterLocalVariable, leaving the innermost scope around the offense for the others,
  then team order), which can pick another cop when the flagged variable belongs to an outer scope.
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
  Lint/SymbolConversion does not treat a backslash-newline inside `:"..."` as a continuation.
- LineLength `SplitStrings: true` break points (string splitting) are not modelled. The unbracketed
  `rescue A, B` array is checked as a breakable collection, but is not an ancestor of calls inside
  the exception list (`contained_by_breakable_collection_on_same_line?`).
- `helpers::node_equality` compares tokens: plain `'a'` and `"a"` are equal, but `foo()` vs `foo` or
  escaped vs unescaped spellings of one string (`"\x41"` vs `"A"`) are not, unlike parser nodes.
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
- Style/DirectiveScope runs on the program node, so its later -A iterations print fewer
  `no department given` warnings than RuboCop's. (Its cop errors carry no location, as RuboCop's
  on_new_investigation ones, #578.)
- Style/DirectiveScope treats only ported cops as disabled by default config; an `enable X` ...
  `disable X` pair for an unported, default-disabled cop can differ (not hit by the gates).
- Lint/RedundantCopDisableDirective judges directives against rbcop's own offenses, so a directive
  for a cop rbcop has not ported yet (e.g. `# rubocop:disable Style/Send`) is reported as
  redundant.
- Config validation (`src/config/validator.rs`: ConfigValidator, ConfigObsoletion,
  Config#deprecation_check, check_target_ruby; scenarios `tests/scenarios/config_*`) matches
  RuboCop's stderr and exit status, with these differences:
  - Cops RuboCop knows from its registry rather than from default configs: a plugin's cops count
    as known when every part of the name is a class or module defined in the plugin gem's `lib/`
    (cops registered without a `config/default.yml` entry, e.g. rubocop-discourse's). A local
    `require:` file, or a plugin gem with neither `config/default.yml` nor a cop class rbcop
    can read, turns unrecognized-cop errors
    off for the run. A `plugins:`/`require:` entry Ruby cannot load stops the run with RuboCop's
    `cannot load such file -- <feature>` and exit 2 (without the backtrace that follows it there);
    without a `ruby` on PATH, gems cannot be looked up and only turn those errors off. Did-you-mean
    suggestions use the default config's cop names, so a plugin cop missing from its
    `default.yml` is never suggested.
  - Suggestions with equal Jaro-Winkler scores can come out in a different order: Ruby's
    `sort_by` is not stable (`Minitest/AssertEqualz` lists `AssertTruthy` before `AssertOutput`;
    RuboCop the other way round).
  - A file loaded again (an inherited or parent config) repeats the warnings of its first load;
    RuboCop validates it again, with the plugins loaded since.
  - Configs are resolved before inspection starts, so a subdirectory config's warnings come before
    the `--only`/`--except` name warnings, where RuboCop prints them when it reaches that
    directory's first file. An invalid subdirectory config still stops the run at its first file,
    after the files before it are reported (as in RuboCop), unless finding the targets loaded it
    first: an extensionless file there makes TargetFinder#ruby_executable? read its directory's
    `RubyInterpreters`, and the run ends before anything is reported.
  - RuboCop crashes with a backtrace on a non-Array `Exclude` (NoMethodError in
    `make_excludes_absolute`); rbcop prints the same first line and exits 2. A non-Array
    `Include` crashes RuboCop later (per cop or in `FilePatterns`); rbcop treats it as one pattern.
    Other values RuboCop crashes on (a non-String `FailLevel`, `TargetRubyVersion: true`, a
    non-Hash section under an unknown name) are not reproduced.
  - RuboCop's backtrace for a bogus `-f` formatter is not printed.
  - `-c FILE`: the `Using ... configuration` warning for a deprecated cop name shows the expanded
    path; RuboCop shows the argument as given.
- YAML: a plain `:name` value loads as a Symbol (`Value::Sym`) and a quoted `':name'` as a String,
  so `--show-cops` dumps them apart; mapping keys do not keep the distinction (a quoted `':q':` key
  is dumped as the symbol `:q:`).
- YAML aliases share their anchored node (`Arc`), so nested aliases load in linear time as in Psych,
  but `--show-cops` prints an aliased value expanded where Psych's dump writes `&1` / `*1`; on an alias
  bomb that dump is exponential (RuboCop's is not).
- YAML: `.rubocop.yml` is read by a Rust port of libyaml 0.2.5's reader, scanner and parser
  (src/yaml), Psych's TreeBuilder (with RuboCop's duplicate-key handler) and ToRuby with RuboCop's
  restricted class loader, so tokens, errors (problem, context and context mark) and loaded values
  are Psych's; `--show-cops` writes values through Psych's YAMLTree rules and a port of libyaml's
  emitter (#560-#567, #569-#574, #580). Differences left:
  - A null or anchored empty mapping key (`{?}`, `{&a : b}`) is dropped where Psych keeps a nil/""
    key, and a non-String key (`15:`) is turned into a String (so `1:` and `'1':` are one key);
    RuboCop crashes later on some of these (`undefined method 'start_with?' for an instance of
    Integer`).
  - An alias inside its own anchored collection (`&a [*a]`) is an unknown anchor (Psych makes a
    recursive Array); `!!omap [...]` loads as a plain Hash (Psych's dump keeps the `!!omap` tag);
    a `!!binary` value with non-ASCII bytes loads as a UTF-8 String (invalid bytes replaced), which
    `--show-cops` writes as text where Psych writes `!binary` and base64; `!ruby/encoding`,
    `!ruby/class Regexp` / `Symbol`, ivars of `!ruby/hash-with-ivars` / `!str` mappings and
    `!ruby/regexp` sources Onigmo rejects are not reproduced.
  - Nesting deeper than 1000 levels fails with `stack level too deep` (Ruby's limit depends on
    its stack: about 1000 mappings, 1400 sequences).
  - The Ruby backtrace RuboCop prints after the message is not reproduced.
- `DisplayStyleGuide`: the style guide URL is resolved once per run for each `StyleGuide` value,
  as RuboCop's per-process MessageAnnotator cache does, so the first offense with an anchor fixes
  its base URL for cops of other departments (#508). rbcop fills the cache in report order (files,
  then each file's sorted offenses); RuboCop fills it as offenses are added, so a disabled offense
  or one from a correcting pass can fill it first there.
- rubocop-rspec family:
  - RSpec/SpecFilePathFormat with `EnforcedInflector: active_support` uses
    ActiveSupport's default inflections; the project's `InflectorPath` file is not evaluated.
    A missing `InflectorPath` file is a cop error on every spec file that reaches the inflector
    (one top-level example group on a constant, not skipped by `IgnoreMetadata`). RuboCop raises
    it once per process: `ActiveSupportInflector.prepare_availability` sets `@prepared` before it
    raises, so only the first such file in inspection order gets the error. When rubocop-rails is
    also loaded (it requires `active_support/inflector`), RuboCop checks the later files with
    ActiveSupport's default inflections and reports their offenses; without it, every such file
    errors, as in rbcop. Which file RuboCop errors on depends on its process state (the order it
    inspects files in, one process), and rbcop lints files in parallel, so rbcop keeps the
    per-file error: with rubocop-rails loaded it reports N cop errors where RuboCop reports one
    error and the other files' offenses (checked with RuboCop 1.91, rubocop-rspec 3.10.2 and
    rubocop-rails 2.38.0, #451).
  - rubocop-rspec_rails loaded without rubocop-rspec: RuboCop fails to load the plugin
    (`uninitialized constant RuboCop::Cop::RSpec::InsideExampleGroup`); rbcop runs the RSpecRails
    cops with rubocop-rspec's default `RSpec/Language`.
  - A rubocop-rspec `Language` list set to `~`: a stabby lambda (`-> { }`) is a parser `block`
    whose send part the cops' `(block (send nil? #List ...) ...)` patterns ask the nil list of, so
    RuboCop reports a cop error at each lambda for every block cop that asks it; rbcop's block
    cops see calls only and report none there (checked with rubocop-rspec 3.10.2, #575).
  - Capybara/RSpec/CurrentPathExpectation on `match "<invalid regexp>"` (`match "("`): RuboCop's
    `Regexp.new` raises and the offense is lost to a cop error; rbcop reports the offense with the
    correction withheld.

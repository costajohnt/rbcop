# Porting a RuboCop cop to rbcop

The bar is **identical output to RuboCop 1.91.0**: same offenses, same ranges, same messages, same
`[Correctable]` flag, on every corpus. RuboCop is the oracle; when in doubt, run it.

## Where things are

- RuboCop source: `~/.rbenv/versions/4.0.6/lib/ruby/gems/4.0.0/gems/rubocop-1.91.0/lib/rubocop/cop/<dept>/<cop>.rb`,
  mixins in `.../cop/mixin/`, cop config in `.../config/default.yml` (rbcop vendors a copy in `config/default.yml`).
- rubocop-ast (node helpers such as `single_line?`, `method?`, `loc.selector`, node groups like `:any_block`):
  `~/.rbenv/versions/4.0.6/lib/ruby/gems/4.0.0/gems/rubocop-ast-1.50.0/lib/rubocop/ast/`.
- RuboCop's specs for the cop: `$RBCOP_CORPUS/rubocop-src/spec/rubocop/cop/<dept>/<cop>_spec.rb`.
- Ruby tools: prefix commands with `RBENV_VERSION=4.0.6` (e.g. `RBENV_VERSION=4.0.6 rubocop ...`).

## Writing the cop

One file per cop: `src/cops/<dept>/<snake_name>.rs` (department dir lowercase, e.g. `layout`, `style`,
`lint`, `naming`, `metrics`, `security`, `bundler`, `gemspec`, `migration`). build.rs registers it.

```rust
use crate::prelude::*;

pub const NAME: &str = "Style/Example";
pub static COP: Example = Example;
pub struct Example;

impl Cop for Example {
    fn correctable(&self) -> bool { true }            // cop `extend AutoCorrector`
    fn ruby_range(&self) -> (f64, f64) { (3.1, f64::MAX) } // minimum_target_ruby_version 3.1
    fn kinds(&self) -> &'static [usize] { &[kind::CALL_NODE] }
    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // ...
        cx.add(start, end, "Message.");
    }
    fn on_file(&self, cx: &mut Cx) { /* line/token based checks */ }
}
```

- `on_node` gets every node of the subscribed kinds in pre-order; `cx.ancestors` holds the Prism
  ancestors (outermost first). `crate::ast::parser_ancestors` gives a parser-gem-shaped view.
- `on_file` runs once per file after the walk: `cx.src.lines`, `cx.src.text`, `cx.src.comments`,
  `cx.src.tokens()` (Prism lexer tokens, `sys::PM_TOKEN_*` kinds; only computed if asked for),
  `cx.facts` (heredoc bodies, string-content lines, delimited string ranges, first code offset).
- Report with `cx.add(start, end, msg)` (byte offsets; `[Correctable]` follows `correctable()`), or
  `cx.add_with(start, end, msg, correctable)` when RuboCop adds that offense without a corrector block.
  The range must be exactly the range RuboCop passes to `add_offense` (node, `loc.selector`, etc.);
  only the start shows in emacs output, but other formatters print the range, and RuboCop drops a
  second offense on an identical range (the runner does the same).
- A cop with a corrector block reports with `cx.add_fix` instead (see Autocorrect below).
- Config: `cx.cfg.str("EnforcedStyle")`, `cx.cfg.int("Max")`, `cx.cfg.bool(..)`, `cx.cfg.strings(..)`,
  `cx.cfg.regexes(..)`, `cx.cfg.get(..)` for raw values. Another cop's config:
  `cx.config.for_cop("Layout/IndentationWidth")`. Target Ruby: `cx.config.target_ruby`.
- Severity, Enabled, Include/Exclude, `# rubocop:disable` and `--only` are handled by the runner.
- Regexes go in `static X: LazyLock<Regex>`; never compile in a loop. Keep `kinds()` narrow.
- Shared logic from RuboCop mixins goes in `src/helpers/<mixin_snake>.rs` (auto-registered; use as
  `helpers::<file>::fn`). Check what exists there first. Add new files freely; in an existing helper
  file only append functions. Do not edit runner, config, cop.rs, other cops, or build.rs; if you need
  infrastructure, put it in your cop or a helper and say so in your report.

## Autocorrect

Port the cop's `add_offense(range) { |corrector| ... }` block as `cx.add_fix`:

```rust
// add_offense(node.loc.selector) { |corrector| corrector.replace(range, '.nil?') }
cx.add_fix(sel_start, sel_end, msg, |c| c.replace(recv_end, end, ".nil?"));
```

- `c: &mut Corrector` mirrors RuboCop's Corrector (`src/autocorrect/corrector.rs`): `replace`, `remove`,
  `wrap`, `insert_before`, `insert_after` take a byte range `(start, end)` like `cx.add`;
  `remove_preceding` / `remove_leading` / `remove_trailing` take a byte size; `swap((a, b), (c, d))`.
  Pass the same range RuboCop passes, not just the insertion point: `insert_before(node)` uses the whole
  node range to decide how edits nest and clobber (`src/autocorrect/tree_rewriter.rs` is parser's
  TreeRewriter). RuboCop ranges are character offsets; convert column arithmetic to bytes.
- Compute what the block needs before calling `add_fix` (the closure cannot use `cx`); `cx.src` can
  be copied into a local first (`let src = cx.src;`).
- `[Correctable]` follows from the edits: an offense is correctable only if the block made one (and
  the cop's `AutoCorrect` allows it), so keep RuboCop's conditions inside the block. A cop using
  `add_fix` still returns `true` from `correctable()` (RuboCop's `extend AutoCorrector`).
- The block does not run for an offense on a range already reported or disabled by a comment.
- Range helpers: `helpers::range_help::range_with_surrounding_space`.
- Check with `scripts/check-autocorrect.sh Cop/Name[,...] [corpus...]` (same env as check-cops.sh,
  plus `MODE=-a` for safe mode, `LIMIT=n`, `OUT=dir`): it runs `rubocop -A` and `rbcop -A` on
  copies of the corpus files that have offenses and diffs the rewritten files and the reports.
  Include the cops that interact with yours (e.g. Layout cops whose edits touch the same lines).

## Parser gem AST vs Prism AST

RuboCop cops match parser-gem nodes. Common mappings:

| parser | Prism |
|---|---|
| `send` / `csend` | `CallNode` (`is_csend`: call operator `&.`); `send_range()` excludes an attached block |
| `block` / `numblock` / `itblock` | `CallNode` whose `block()` is a `BlockNode` (params: `BlockParametersNode`, `NumberedParametersNode`, `ItParametersNode`); the block is the call's *child* in Prism but its *parent* in parser |
| `lambda` literal `->` | `LambdaNode` |
| `block_pass` (`&blk`) | `CallNode.block()` = `BlockArgumentNode` |
| `def` / `defs` | `DefNode` (`receiver()` set for defs) |
| `str` / `dstr` / `xstr` | `StringNode` / `InterpolatedStringNode` / `XStringNode`, `InterpolatedXStringNode` |
| `sym` / `dsym` | `SymbolNode` / `InterpolatedSymbolNode` |
| `regexp` | `RegularExpressionNode` / `InterpolatedRegularExpressionNode` |
| `if` (incl. ternary, modifier), `unless` | `IfNode` (ternary: no `if_keyword_loc`), `UnlessNode`; `elsif` is an `IfNode` in `subsequent()` |
| `case`/`when`, `case_match`/`in_pattern` | `CaseNode`/`WhenNode`, `CaseMatchNode`/`InNode` |
| `while`/`until` (+ `_post`) | `WhileNode`/`UntilNode` (`is_begin_modifier` for post) |
| `and` / `or` / `not` | `AndNode` / `OrNode`; `!x` and `not x` are `CallNode` named `!` |
| `lvasgn`, `ivasgn`, `op_asgn`, `or_asgn`, `and_asgn`, `masgn` | `*WriteNode`, `*OperatorWriteNode`, `*OrWriteNode`, `*AndWriteNode`, `MultiWriteNode` |
| `begin` (parens / multi-statement body) | `ParenthesesNode` / `StatementsNode` with 2+ statements |
| `kwbegin` | `BeginNode` with `begin_keyword_loc`; `rescue`/`ensure` live on `BeginNode` (also implicit in def bodies) |
| `hash` / `pair` / `kwsplat` | `HashNode` or `KeywordHashNode` (unbraced) / `AssocNode` / `AssocSplatNode` |
| `array` / `splat` | `ArrayNode` / `SplatNode` |
| `const` / `casgn` | `ConstantReadNode`, `ConstantPathNode` / `ConstantWriteNode`, `ConstantPathWriteNode` |
| `args`, `arg`, `optarg`, `restarg`, `kwarg`, `kwoptarg`, `kwrestarg`, `blockarg` | `ParametersNode`, `RequiredParameterNode`, `OptionalParameterNode`, `RestParameterNode`, `RequiredKeywordParameterNode`, `OptionalKeywordParameterNode`, `KeywordRestParameterNode`, `BlockParameterNode` |
| `super` / `zsuper` / `yield` | `SuperNode` / `ForwardingSuperNode` / `YieldNode` |
| `irange` / `erange` | `RangeNode` (`is_exclude_end`) |

Inspect real trees instead of guessing:

```sh
RBENV_VERSION=4.0.6 rbenv exec ruby -rprism -e 'pp Prism.parse(ARGF.read).value' file.rb
RBENV_VERSION=4.0.6 rbenv exec ruby -rrubocop-ast -e 'p RuboCop::AST::ProcessedSource.new(ARGF.read, 3.4).ast' file.rb
```

Location differences to watch: parser `send` source excludes a `do`/`{}` block; heredoc nodes'
`loc.expression` is the opening `<<~X` only; `loc.selector`, `loc.dot`, `loc.begin`/`loc.end` map to
Prism's `message_loc`, `call_operator_loc`, `opening_loc`/`closing_loc`.

More traps found while porting:

- Prism turns a pattern guard (`in pat if cond`) into an `IfNode`/`UnlessNode`; parser makes it an
  `if_guard`, so RuboCop's `on_if` never sees it. Skip those with `ast::is_pattern_guard`.
- Parser `op_asgn` / `or_asgn` / `and_asgn` / `mlhs` contain a `send` that `on_send` fires on; Prism
  has `CallOperatorWriteNode`, `CallOrWriteNode`, `CallAndWriteNode`, `CallTargetNode`, `IndexTargetNode`
  instead, so an `on_send` port may need to subscribe to those too.
- rubocop-ast defines `receiver` on every node (nil for most); `ast::strip_parens` mirrors the common
  "unwrap `begin`" loop.
- To walk a subtree (`each_node` / `each_descendant`), implement `ruby_prism::Visit` on a small struct.
- rubocop-ast nodes compare structurally (type + children): `==`, `include?`, `group_by`, `uniq` treat
  two textually identical nodes as equal. Compare by source/structure, not by offset, when porting those.
- `def_node_search` without a block on a name not ending in `?` returns an Enumerator, which is always
  truthy: `return unless some_search(node)` never returns early. Port the behaviour, not the intent.
- `add_global_offense` (no location): report at `cx.add(0, 0, ..)` for now; the formatter work adds a
  proper global flag.
- The worktree sandbox reads a bare `-c` argument as `sh -c`; pass rbcop/RuboCop config with `--config`.
- `assignment?` in node patterns includes setter sends (`a.b = x`, `a[i] = x`, `a&.b = x`); in Prism
  that is a `CallNode` with `equal_loc()`.
- Parser visits a `block` before its `send`, receiver and arguments; Prism visits the `BlockNode` last.
  Cops keeping state from `on_block` must record it when they see the owning `CallNode`. RuboCop also
  rarely resets such ivars between classes, and the leaking state is observable: copy it.
- A Prism `CallNode` with a block, seen from its receiver or arguments, stands for two parser
  ancestors: the `send` (`send_range`) and then the `block`.
- Translation tokens: `PARENTHESIS_LEFT_PARENTHESES` is `tLPAREN_ARG`, `PARENTHESIS_RIGHT` is `tRPAREN`,
  `BRACKET_LEFT` is `tLBRACK2`, `BRACKET_LEFT_ARRAY` is `tLBRACK`.
- `RangeHelp#source_range(buffer, line, col)` has length 1 by default (not zero-width).
- Ruby `\s` and `String#strip` are ASCII-only while `[[:alnum:]]` is Unicode; Rust defaults are the
  other way round (`helpers::ruby_string`).
- Where RuboCop crashes (e.g. `nil.blank?` past the last line) the oracle has no offense; the gate
  skips crash-listed (cop, file) pairs, otherwise report nothing on that path.
- `cx.src.raw_text` is the file as read (CRLF intact); offsets refer to the normalised `cx.src.text`.
- rubocop-ast `BlockNode#single_line?`/`multiline?` compare the `{`/`do` line with the `}`/`end` line,
  not the whole block: `Foo.new(x)\n  .tap { ... }` is single-line.
- In a body with rescue/ensure, Prism's implicit `BeginNode` starts at `do`/`def` and its ensure
  clause includes `end`; parser's `rescue`/`ensure` spans only statements and clauses (`ast::body_range`).
- `begin ... end while x` is parser `while_post`, which `on_while` never sees (`is_begin_modifier`).
- In `Foo::Bar = 1` the target `ConstantPathNode` is part of parser's `casgn`, not a `const` node.
- Prism emits a `__END__` token; parser has none.
- `in_macro_scope?` treats `Class.new`/`Module.new`/`Struct.new`/`Data.define` blocks as class-like.
- Alignment-mixin cops register offenses nested in an earlier offense's range without a corrector
  (not `[Correctable]`, sometimes a different message): track earlier ranges in `cx.state`.
- RuboCop drops a second offense on an identical range even with another message, so the order of
  checks decides which message survives.
- `ruby_prism::Node` is not `Clone`; collect `(start, end)` ranges when items must be reordered.
- `on_node(type, arg, :send)` searches inside an argument but not into plain `send` nodes (it does go
  into csends and blocks); Prism's `ArgumentsNode` sits in between.
- Parser wraps a `rescue A, B` exception list in an `array`.
- `a[...]` has no `loc.begin` in parser although Prism has `opening_loc` `[`.
- A label pair `key: v` has no `operator_loc` in Prism; treat the missing operator as `:`.
- Parser makes a heredoc a `str` only when its body has at most one line and no interpolation;
  otherwise it is a `dstr` (matters for `str_type? && heredoc?`).
- `loc.heredoc_end` spans the whole terminator line including indentation, excluding the newline;
  Prism's `closing_loc` includes the newline. For `<<~` heredocs Prism's `content_loc`/first part
  start after the stripped indentation, parser's `heredoc_body` at the start of that line.
- `#{ ;}` has no statements in Prism but a `tSEMI` token in parser.
- RuboCop crashes inside corrector blocks lose the offense entirely (e.g. `loc.dot` on a call without
  a dot); report nothing on those paths.
- When one corrector fixes two offenses and then `ignore_node`s, the second offense is not
  `[Correctable]`; a captured flag in the `add_fix` closure reproduces this.
- Some cops behave differently only while autocorrecting (e.g. IndentationWidth's
  `other_offense_in_same_range?`): check `cx.fixes.autocorrect`.
- RuboCop visits a `kwbegin`, then its `ensure`, then its `rescue`; Prism has one BeginNode for all
  three, so order-dependent checks must run in that order there.
- A ternary's `:` is not `loc.else`; an `elsif` if-node has no `loc.end`; parser `unless` keeps its
  body as `node.body` (not swapped like `if`).
- An offense that depends on ancestors seen later can be stored in `cx.state` keyed by the node's
  span and emitted when the node itself is visited.
- macOS paths: RuboCop and rbcop print physical paths (`/private/tmp`, `/private/var`); scratch
  comparisons must normalise with `pwd -P`.
- Parser makes `/re/ =~ x` a `match_with_lvasgn` even without named captures; Prism gives a plain
  `=~` CallNode (wrapped in MatchWriteNode only when there are captures).
- Parser makes every multi-line string literal a `dstr`, not only heredocs: use `ast::is_parser_dstr`
  for `dstr_type?`, `basic_literal?` and `on_dstr` logic.
- An `elsif` node's parser range stops at its last branch body; Prism's includes `end`.
- `match_var` is Prism `LocalVariableTargetNode` only inside patterns; masgn/rescue/for targets are
  parser `lvasgn`.
- A Prism `StatementsNode` with 2+ statements is parser's `begin` for `right_sibling` purposes.
- `heredoc_body.last_line` is the terminator line; for an empty body the first body line is too.
- A `return` inside an `add_offense` corrector block skips the offense entirely (IdentityComparison).
- Diagnostics-driven cops: `helpers::parse_warnings` maps Prism warnings (`type_name()`) to
  parser's reasons.
- Directive cop names are `cops.split(/,\s*/)` untrimmed, and the registry compare uses the original
  string, not the camel-cased badge.
- The regex crate has no lookbehind: rewrite `(?<!x)` patterns by hand.
- Ruby crashes count: when RuboCop raises (e.g. `[nil, 1].max` in FormatString), the oracle has no
  offense there, so neither should rbcop.
- Alignment corrections: build the edit list first, then call `alignment_corrector::correct` inside
  the `add_fix` closure (node when RuboCop passes a node, `None` for a range). Never compute
  `[Correctable]` from a would-it-edit helper. Display width comes from `helpers::alignment`
  (table generated from unicode-display_width 3.2.0, emoji as plain characters).
- MultilineMethodCallIndentation with a block corrects the selector line, the body (`ast::body_range`;
  Prism's implicit begin starts at `do`) and the `end` line. CommentIndentation also shifts preceding
  same-column comments separated only by blank lines.
- Ruby's `^` does not match after the final newline.
- The runner dedups offenses on (cop, start, message) like RuboCop's `Offense#==`; two offenses of one
  cop starting at the same column with the same message print once (fixes still both apply).
- `after_send` hooks: there is no post-order dispatch, so subscribe to `PROGRAM_NODE` and run your own
  visitor (fire after receiver, args and block-pass, before an attached block). `MatchWriteNode` gets
  none; `Index{Operator,Or,And}WriteNode` contain a parser `send :[]` and do.
- `in pat if guard`: Prism nests the pattern inside the IfNode; don't let descendant walks reach the guard.
- `rescue *X`: Prism puts the splat directly under RescueNode; parser wraps it in an unbracketed `array`.
- A parser `(block ...)` pattern never matches numblock/itblock: exclude BlockNodes with numbered or
  `it` parameters.
- `children.first != node` compares structurally: two identical `private` lines both count as
  "first". Use `node_equality::same`, not the statement index.
- Ruby correctors that hit `nil` raise and drop the offense (EmptyLineBetweenDefs with no newline after
  the previous def; MultilineBlockLayout with lambda args before `{`): skip reporting there.
- Heredoc end: parser `loc.heredoc_end.end_pos` excludes the terminator's newline, Prism's
  `closing_loc` includes it.
- A corrector shared by two offenses can be one reference-capturing closure (it is `Copy`).
- Cops whose fix lives in a helper: reference `helpers::<name>` (whose file calls `add_fix`) or add a
  `// corrector: <where>` line so `scripts/missing-correctors.sh` stays quiet.
- VariableForce cops (Unused*, UselessAssignment, ShadowingOuterLocalVariable, ...) use
  `helpers::variable_force`: `Tree::build(program)` then `investigate(&tree, |tree, scope, vars| ...)`.
  Extend it; never write a second tracker.
- Prism `RescueNode` spans all following `rescue` clauses; parser `resbody` ends at its body (or last
  exception / `=> var`, plus a trailing `;`, never `then`). Use `helpers::resbody`.
- `void_context?` counts `initialize` only on a plain `def`, not `def self.initialize`.
- Prism `Visit` branch/leaf enter hooks are not called for concretely typed fields (StatementsNode,
  ArgumentsNode, ElseNode, ...); override `visit_X_node` instead.
- `logical_operator?` is only `&&`/`||`, not `and`/`or`. rubocop-ast `if_branch` of an `unless` is
  its body (the then part).
- Float#to_s is fixed notation only for decimal exponents in (-4, 16); otherwise `1.0e+15`,
  `1.0e-05` (2+ exponent digits).
- `children.first.to_s` on dstr/dsym prints an s-expression, so string-matching cops only ever match
  plain str/sym elements.
- `helpers::nil_methods` is `nil.methods` from a live RuboCop (includes `to_json`, `to_yaml`).
- Parser `when` without a body ends at its last condition (Prism includes `then`). Label symbol
  ranges (`a:`) exclude the colon. Parser `if` `loc.begin` can be `;`. `def (expr).foo` drops the
  parens. `"#{a}"` is a parser `begin` (on_begin fires). Plain `begin..end` is `(kwbegin a b)`
  with no inner `begin`. `a rescue b` is `(rescue a (resbody nil nil b) nil)`.
- `each_descendant` excludes the node itself.
- Compare invalid-UTF-8 string keys as raw bytes, never via `from_utf8_lossy`.
- Helpers: `comments_help` (CommentsHelp), `literal_node` (literal predicates + structural key),
  `if_node::parser_end`, `nil_receiver_checker`.
- `each_ancestor` with a block returns `self` when nothing matches, so a failed lookup can leak the
  node's s-expression into keys and messages. A `def_node_search` method called with a block
  returns `nil` when nothing matches (the generated method ends in `nil`).
- TrailingCommaInArguments (and any `on_send` cop) also sees the `send :[]` inside parser's
  op_asgn/or_asgn/and_asgn: subscribe to `Index{Operator,Or,And}WriteNode` too.
- `ast_with_comments[arg]` for call arguments: one comment cursor over the arguments; each takes the
  comments ending before its end, then those starting on its last line.
- Helper: `trailing_comma` (TrailingComma mixin, PunctuationCorrector swap_comma).
- ReparsedEquivalence (`correction_parses?`, `parses_equivalently?`): `helpers::reparsed_equivalence`.
  When an offense depends on the correction parsing, build the edit list once, test it on a
  standalone Corrector, then replay it in `add_fix`.
- `children.one?` counts only non-nil children (`if` with empty body is `[cond, nil, nil]`).
- Prism `NumberedParametersNode` / `ItParametersNode` locations span the whole block: never use
  their source as text.
- Line-based `on_new_investigation` cops stop at the last parser token's line:
  `parser_tokens::translated(src).last()`.
- A Prism `'a' \ 'b'` concatenation is a flat InterpolatedStringNode without opening (parser's flat
  dstr). A backslash-newline inside an interpolating literal is a continuation: still a `str`.
- Always pass `--cache false` to RuboCop on reused scratch dirs.
- A cop's second offense on an identical range is dropped by `Cx` (Base#add_offense
  `current_offense_locations`), whatever its message; don't add your own same-range filter.
- A panicking cop is contained by `runner::guarded` (RuboCop-style "An error occurred while ..."),
  but a panic is still a bug: guard slices against reversed ranges the way RuboCop's logic does.
- Block-range offenses: subscribe to the block's owner (CallNode/SuperNode/ForwardingSuperNode/
  LambdaNode), not BlockNode; parser visits outer blocks of a chain first.
- Parser `str` children: a regexp literal's and an xstring's content are `str` nodes that `on_str`
  sees; Prism has no StringNode there.
- `node.parent&.any_block_type?` is also true for a call that is a block's single body statement.
- `DstrNode#value` concatenates child values (source for children without one); `DsymNode#value`
  raises, dropping the offense.
- Parser heredoc `dstr` text children start at column 0 and split at every newline and
  interpolation.
- Helpers: `regexp_parser::tree` (expression tree), `allowed_receivers`, `value_used`,
  `file_open_patterns`, `comment_config`, `add_parentheses::ParenFix::Params`,
  `ruby_string::{regexp_escape, float_to_s}`.
- Parser `Range#join` takes min begin / max end: build joined ranges the same way and never slice
  with possibly reversed ends (BlockDelimiters' chained-block comment move).
- A bare lowercase type in a NodePattern (`next`) matches that type with any children.
- `BlockNode#lambda?` is `send_node.method?(:lambda)` (any receiver/arguments); `proc?` requires
  `proc`/`Proc.new` with no arguments.
- rubocop-ast's `receiver` matcher covers blocks (the send's receiver); a lambda's is nil.
- `respond_to?(:value)` holds only for str, dstr, int, float, sym, rational, complex.
- ParenthesesCorrector on a send without parentheses raises: no offense.
- `allowed_receiver?(nil)` raises: receiver-less calls get no offense in AllowedReceivers cops.
- rubocop-ast `FormatSequence#arg_number` is the last `N$` anywhere in the sequence. Integer and
  Rational `%f` format exactly with round-half-up; floats round half-even.
- Prism `unescaped()` borrows the node: `.to_vec()` when the value outlives it.
- Helpers: `check_single_line_suitability`, `comments_help::{find_end_line, comments_in_lines}`,
  `select_by`, `block_args::{Single, single_var, single_body}`.
- Prism `CallNode#message_loc` of an index call (`foo[x]`, `foo[x] = y`) spans the brackets and
  arguments; use `name()` when you mean parser's method name.
- Helpers: `if_node::{normal_keyword, parts, is_elsif}`, `ruby_string::symbol_inspect`.
- Binary-encoded files: Ruby `String#inspect` escapes every non-ASCII byte (`"\xE3\x81\x82"`), so
  use `ruby_string::inspect_binary` when `cx.src.binary`.
- `AllowedPatterns || IgnoredPatterns`: an empty AllowedPatterns array is truthy; IgnoredPatterns is
  read only when the AllowedPatterns key is missing.
- Helpers: `statement_modifier` (StatementModifier + LineLengthHelp), `string_help` (StringHelp,
  StringLiteralsHelp, StringLiteralCorrector).
- `each_ancestor(:any_block).first` checks every ancestor, not just the innermost scope.
- `Module#<=>` is 0 for the same class (truthy); `nil <=> nil` is 0, nil vs class is nil.
- Prism nests subsequent `rescue` clauses; parser makes them siblings. Treat a RescueNode whose child
  is the next RescueNode as transparent in parser-ancestor walks.
- Constants Ruby resolves at runtime: `helpers::ruby_exceptions` (regenerate with
  `scripts/dump-ruby-exceptions.rb`). Paths in messages go through PathUtil.smart_path semantics.
- Ruby `format('%.4g')` is not correctly rounded (dtoa half-even on near-halfway values): use
  `method_complexity::format_g4`.
- A ternary has no `loc.else`; an empty `else` gives `else_branch` nil. `String#blank?` counts NUL as
  whitespace. Parser has `send` nodes Prism lacks (`a.b ||= c`, `a[i] += c` left sides) and wraps
  `rescue A, B` lists in an `array`.
- Metrics helpers: `code_length` (CodeLength + CountAsOne), `method_complexity`,
  `abc_size_calculator`.
- Prism `name_loc` on a keyword parameter `a:` includes the colon; parser's `kwarg` name does not.
- "Inside a resbody" checks must test the parser resbody range (`helpers::resbody::range`), since
  Prism nests later rescue clauses.
- Resolve unqualified cop names against all of default.yml (`helpers::cop_registry`), never
  `crate::cops::ALL`.
- `body.children.last` means the else branch of an if/case and a block/loop body.
- A whole-file check that needs the AST subscribes to `PROGRAM_NODE` (the file hook has no
  ancestors).
- Naming helpers: `configurable_naming` (formats, numbering, Allowed/Forbidden identifiers).
- Corrections without an offense (`correct(range)` from an `after_class` hook, BisectedAttrAccessor):
  build a `Corrector`, merge it into `cx.fixes.corrector` when `cx.fixes.autocorrect` is set, and
  report the offenses with `add_with(.., false)`. It applies even on disabled lines.
- Parser `begin` parent: any statement list except under a plain `begin..end` (`kwbegin`);
  parenthesised statements and a 2+ statement program root are `begin`.
- `helpers::comment_association`: leading comments parser's `Comment::Associator` gives a statement.
- `arg.children.first.to_s` on an mlhs parameter is an s-expression (`"(arg :aB)"`), and its length
  can set an offense range past the line end. A single `|a|` is `arg`, not `procarg0`.
- Parser heredoc `children.empty?` is true only for an empty body.
- `helpers::uncommunicative_name`; `helpers::heredoc::parts` (opening, parser heredoc_end, empty?).
- `def (expr).foo`: use `strip_parens` to get parser's receiver source.
- `ruby_prism::Integer` has no `TryFrom<i64>`: use `try_into::<i32>()` or `to_u32_digits()`.
- Helpers: `annotation_comment` (AnnotationComment), `magic_comment::{encoding, without_encoding}`.
- `node_equality::same` includes heredoc bodies (they lie outside the node range); don't add a
  second body comparison in the cop.
- Flags that RuboCop keeps per method call (`inserted_expression` in IdenticalConditionalBranches)
  must not become one shared `Cell`.
- Keep RuboCop's evaluation order before code that crashes in Ruby: if RuboCop raises first, there
  is no offense.
- Always pass your own `OUT=` (or rely on the per-worktree default) to check-autocorrect.sh; the
  gate scripts rebuild the release binary first.
- rubocop-ast `IfNode#else_branch` on an `unless` is the `else` clause (node_parts swap), while
  raw-children node patterns see the opposite branch. `IfNode#branches`: ternary gives both, plain
  if without else gives one, elsif chains flatten.
- `def_node_search(...).to_a.last` is the last match in pre-order (largest start offset).
- `EnsureNode#branch` is the ensure body, not the main body.
- `helpers::parser_parent` (parser_parent, is_top_const, is_operator_keyword, is_ternary).
- Comment association: a `begin` (2+ statements) never gets leading comments (its first statement
  does); a statement gets a same-line trailing comment only if no descendant ends on that line.
- `|a, b,|` gives a Prism `ImplicitRestNode` parameter that parser has no argument for.
- Regexp cops reading `parsed_tree`: use `helpers::regexp_parser` (regexp_parser 2.12 subset).
  Offsets are characters from the end of `loc.begin`; `#{}` is blanked one space per char and a
  `#@ivar` part loses its `#`, shifting later positions left by one (RuboCop reports them shifted).
- Prism regexp `closing_loc` includes the options (`/i`); parser `loc.end` is the delimiter only.
- RuboCop's RESTRICT_ON_SEND lists matter: RedundantReturn includes `lambda`, so `-> {}` is checked.
- `nil` in a NodePattern matches a `(nil)` literal node; a missing child is `nil?`.
- `__FILE__` is a parser `str` (Prism `SourceFileNode`).
- Prism `BRACE_LEFT` is both `tLCURLY` (block) and `tLBRACE` (hash): tell them apart via the AST.
- Chained modifier rescue `a rescue b rescue c`: parser visits the inner rescue first.
- An `elsif` IfNode has an `end_keyword_loc()` in Prism; test that the keyword is `if`.
- Helpers: `line_break_corrector` (TrailingBody, LineBreakCorrector), `parentheses_corrector`.
- Cross-cop autocorrect interactions: `scripts/check-autocorrect-all.sh` runs -A with every ported
  cop together (per-batch gates only cover their own cops).
- Block visit order like parser (block, then its send, then args): subscribe to the block's owner
  (CallNode, SuperNode, ForwardingSuperNode, LambdaNode), run `on_block` logic, then `on_send`.
- `ignore_node` inside a corrector block runs only when the corrector runs (not for
  comment-disabled offenses): set a flag inside the `add_fix` closure.
- `on_str` cops see one parser `str` per source line of a multi-line string or heredoc.
- `contains_comment?(range)` is line-based: any comment starting on a spanned line counts.
- `loc.name` on `or_asgn`/`op_asgn` is the variable name with sigil; on a constant path, the last
  segment.
- `helpers::format_string::sequence_parts` splits one format sequence.
- Parser folds `- 12345` into one negative `int`; Prism makes a `-@` call. `1i` is a Prism
  ImaginaryNode wrapping the number; parser has no `int` there.
- NodePattern named wildcards unify: `(arg _val) ... (lvar _val)` needs the same name.
- `ast_with_comments`: a trailing comment on `module A # x` belongs to the name const (in `A::B`, to
  the inner `A`); interior comments go to descendants.
- `parser_parent(&anc[..i])` returns the owning call when `anc[i]` is a BlockNode; use
  `parser_parent_index`.
- Trailing commas: Prism `ImplicitRestNode` (`a, = x`, `|a,|`) has no parser node.
- Helpers: `frozen_string_literal`, `integer_node`, `empty_parameter`, `hash_transform_method`,
  `percent_array` (PercentArray + PercentLiteralCorrector; `safe_string_literal` for written code).
- RuboCop `each_ancestor` loops that look for a target node often never reach it and keep checking
  ancestors outside the construct: use real parser ancestors of descendants (`helpers::parser_path`).
- Cop state that a corrector mutates (`@reindented_lines` in Style/Next) must change only when the
  corrector runs: take it out of `cx.state` before `add_fix`, put it back after.
- Reading RuboCop source with `grep -v '^ *#'` drops heredoc lines starting with `#{`.
- Helpers: `parser_path`, `literal_node::parser_type`, `heredoc::dstr_body_lines`,
  `if_node::parser_begin_end`.
- `helpers::regexp_captures` counts named/numbered groups like `RegexpNode#each_capture`.
- Reusable ports: `format_string` (Utils::FormatString), `ruby_string::inspect`, `did_you_mean`,
  `add_parentheses`, `node_equality` (token approximation of structural `==`), `whole_lines`.

### Prism tokens vs parser-gem tokens

Use `helpers::parser_tokens::sorted_tokens` when porting token-based cops; it smooths most of this:

| | Prism | parser gem |
|---|---|---|
| newlines inside expressions | `IGNORED_NEWLINE` tokens | none (drop them) |
| statement newlines | `NEWLINE` | `tNL` |
| comments | `COMMENT` including the trailing `\n` | `tCOMMENT` without it (use `src.comments` ranges) |
| `=begin`/`=end` | `EMBDOC_BEGIN`, `EMBDOC_LINE`..., `EMBDOC_END` | one comment token |
| `{` | `BRACE_LEFT` for hash and block alike | `tLBRACE` (hash) vs `tLCURLY` (block): check the AST |
| interpolation | `EMBEXPR_BEGIN` / `EMBEXPR_END` | `tSTRING_DBEG` / `tSTRING_DEND` |
| labels `a:` | one `LABEL` token | one `tLABEL`, same end |
| `g ((1))` paren | `PARENTHESIS_LEFT_PARENTHESES` | `tLPAREN_ARG`, not `left_parens?` |

## Correctable flags

RuboCop marks an offense `[Correctable]` only if its corrector block actually made an edit. Where the
RuboCop cop calls `add_offense` with a block that sometimes adds nothing (e.g. `return if
part_of_ignored_node?(node)` inside the block), report with `cx.add_with(.., false)` in that case.

## Comparing on a scratch file

Fastest loop for edge cases: write a small `.rb` in a scratch dir with a config that pins the target
and enables everything, then diff both tools' emacs output:

```sh
printf 'AllCops:\n  TargetRubyVersion: 3.4\n  NewCops: enable\n' > /tmp/x/.rubocop.yml
(cd /tmp/x && RBENV_VERSION=4.0.6 rbenv exec rubocop --only Style/Example --format emacs t.rb)
(cd /tmp/x && /path/to/rbcop --only Style/Example --format emacs t.rb)
```

Without a pinned `TargetRubyVersion` RuboCop falls back to whatever `.ruby-version`/Gemfile.lock it finds.
The worktree sandbox rejects shell loops, heredoc writes and variable-built arguments: run one plain
command per call (the gate accepts a comma-separated cop list, so one call covers a whole batch).

## The gate

```sh
cargo build --release
export RBCOP_CORPUS=/Users/johncosta/.claude/jobs/89c7fab2/tmp/corpus RBCOP_ORACLE=/Users/johncosta/.claude/jobs/89c7fab2/tmp/oracle2
scripts/check-cops.sh Style/Example,Style/Other specs          # fast loop (RuboCop's own spec snippets)
scripts/check-cops.sh Style/Example,Style/Other                # full gate: specs, rails, discourse
```

The oracles were recorded with every cop enabled (`config/all-cops.yml`), so pending and
disabled-by-default cops are covered too. Diffs land in `$TMPDIR/rbcop-check/<corpus>.diff`
(`<` only RuboCop, `>` only rbcop); look at the source line and RuboCop's code until you understand
the difference. A cop is done when the full gate prints `all corpora match`. Also run
`cargo test --release` and `cargo clippy --release --all-targets` (no warnings) before committing.

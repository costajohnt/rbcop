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
- Config: `cx.cfg.str("EnforcedStyle")`, `cx.cfg.int("Max")`, `cx.cfg.bool(..)`, `cx.cfg.strings(..)`,
  `cx.cfg.regexes(..)`, `cx.cfg.get(..)` for raw values. Another cop's config:
  `cx.config.for_cop("Layout/IndentationWidth")`. Target Ruby: `cx.config.target_ruby`.
- Severity, Enabled, Include/Exclude, `# rubocop:disable` and `--only` are handled by the runner.
- Regexes go in `static X: LazyLock<Regex>`; never compile in a loop. Keep `kinds()` narrow.
- Shared logic from RuboCop mixins goes in `src/helpers/<mixin_snake>.rs` (auto-registered; use as
  `helpers::<file>::fn`). Check what exists there first. Add new files freely; in an existing helper
  file only append functions. Do not edit runner, config, cop.rs, other cops, or build.rs; if you need
  infrastructure, put it in your cop or a helper and say so in your report.

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
export RBCOP_CORPUS=/Users/johncosta/.claude/jobs/89c7fab2/tmp/corpus RBCOP_ORACLE=/Users/johncosta/.claude/jobs/89c7fab2/tmp/oracle
scripts/check-cops.sh Style/Example,Style/Other specs          # fast loop (RuboCop's own spec snippets)
scripts/check-cops.sh Style/Example,Style/Other                # full gate: specs, rails, discourse
```

The oracles were recorded with every cop enabled (`config/all-cops.yml`), so pending and
disabled-by-default cops are covered too. Diffs land in `$TMPDIR/rbcop-check/<corpus>.diff`
(`<` only RuboCop, `>` only rbcop); look at the source line and RuboCop's code until you understand
the difference. A cop is done when the full gate prints `all corpora match`. Also run
`cargo test --release` and `cargo clippy --release --all-targets` (no warnings) before committing.

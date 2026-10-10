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

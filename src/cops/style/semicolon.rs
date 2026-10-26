use crate::helpers::parser_tokens::translated;
use crate::prelude::*;
use ruby_prism::Visit;
use std::collections::{BTreeMap, HashMap, HashSet};

pub const NAME: &str = "Style/Semicolon";
pub static COP: Semicolon = Semicolon;
pub struct Semicolon;

const MSG: &str = "Do not use semicolons to terminate expressions.";

/// The arguments of a call: (end of its selector or keyword, first argument start, last argument range).
type Args = (usize, usize, (usize, usize));
type Pair = ((usize, usize), Option<Args>);

/// What the corrector and `on_begin` need from the AST, gathered in one pre-order walk.
#[derive(Default)]
struct Facts {
    /// Parser `begin` nodes with 2+ children: (source range, children's last offsets).
    begins: Vec<((usize, usize), Vec<usize>)>,
    /// irange/erange nodes, pre-order.
    ranges: Vec<(usize, usize)>,
    /// Value-omission pairs: (pair range, the arguments of the call the hash is an argument of).
    pairs: Vec<Pair>,
    /// Heredoc openings: (start, end).
    heredocs: Vec<(usize, usize)>,
    /// `{` tokens opening a hash literal (parser tLBRACE, not tLCURLY).
    hash_braces: HashSet<usize>,
    /// Keyword hash argument start -> the arguments of its call, super or yield.
    call_sel: HashMap<usize, Args>,
}

fn span(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

impl<'pr> Visit<'pr> for Facts {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        let body: Vec<Node> = node.body().iter().collect();
        if body.len() >= 2 {
            let ends = body.iter().map(|n| n.location().end_offset()).collect();
            self.begins.push((span(&node.as_node()), ends));
        }
        ruby_prism::visit_statements_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        // `begin a; b end` is (kwbegin a b): no parser `begin` for its statements.
        let plain_kwbegin =
            node.begin_keyword_loc().is_some() && node.rescue_clause().is_none() && node.ensure_clause().is_none();
        if let (true, Some(s)) = (plain_kwbegin, node.statements()) {
            for n in s.body().iter() {
                self.visit(&n);
            }
            if let Some(e) = node.else_clause() {
                self.visit_else_node(&e);
            }
            return;
        }
        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_range_node(&mut self, node: &ruby_prism::RangeNode<'pr>) {
        self.ranges.push(span(&node.as_node()));
        ruby_prism::visit_range_node(self, node);
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode<'pr>) {
        self.hash_braces.insert(node.opening_loc().start_offset());
        self.pairs_of(node.elements().iter(), None);
        ruby_prism::visit_hash_node(self, node);
    }

    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode<'pr>) {
        let args = self.call_sel.get(&node.location().start_offset()).copied();
        self.pairs_of(node.elements().iter(), args);
        ruby_prism::visit_keyword_hash_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if let Some(sel) = node.message_loc() {
            self.call_args(sel.end_offset(), node.arguments(), node.block());
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'pr>) {
        self.call_args(node.keyword_loc().end_offset(), node.arguments(), node.block());
        ruby_prism::visit_super_node(self, node);
    }

    fn visit_yield_node(&mut self, node: &ruby_prism::YieldNode<'pr>) {
        self.call_args(node.keyword_loc().end_offset(), node.arguments(), None);
        ruby_prism::visit_yield_node(self, node);
    }

    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
        self.heredoc(&node.as_node());
    }
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        self.heredoc(&node.as_node());
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode<'pr>) {
        self.heredoc(&node.as_node());
    }
    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode<'pr>) {
        self.heredoc(&node.as_node());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}

impl Facts {
    fn heredoc(&mut self, node: &Node) {
        if is_heredoc(node) {
            self.heredocs.push(span(node));
        }
    }

    fn pairs_of<'pr>(&mut self, elements: impl Iterator<Item = Node<'pr>>, args: Option<Args>) {
        for e in elements {
            if e.as_assoc_node().is_some_and(|a| a.value().as_implicit_node().is_some()) {
                self.pairs.push((span(&e), args));
            }
        }
    }

    /// Records the arguments of a call, super or yield for its keyword hash arguments.
    fn call_args(&mut self, sel_end: usize, args: Option<ruby_prism::ArgumentsNode>, block: Option<Node>) {
        let mut all: Vec<Node> = args.map(|a| a.arguments().iter().collect()).unwrap_or_default();
        all.extend(block.filter(|b| b.as_block_argument_node().is_some()));
        let (Some(first), Some(last)) = (all.first(), all.last()) else { return };
        let a = (sel_end, first.location().start_offset(), span(last));
        for h in all.iter().filter(|a| a.as_keyword_hash_node().is_some()) {
            self.call_sel.insert(h.location().start_offset(), a);
        }
    }
}

fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

impl Cop for Semicolon {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        let mut facts = Facts::default();
        facts.visit(node);
        let empty = node.as_program_node().unwrap().statements().body().iter().next().is_none();

        // on_new_investigation: a semicolon ending a line or right after an opening brace.
        if !empty && src.raw_text.contains(';') {
            let tokens = translated(src);
            let mut lines: BTreeMap<usize, Vec<Token>> = BTreeMap::new();
            for t in &tokens {
                lines.entry(src.line_of(t.start)).or_default().push(*t);
            }
            let semi = |t: Option<&Token>| t.is_some_and(|t| t.kind == sys::PM_TOKEN_SEMICOLON);
            let kind_at = |t: Option<&Token>, k: u32| t.is_some_and(|t| t.kind == k);
            let left_curly = |t: Option<&Token>| {
                t.is_some_and(|t| {
                    t.kind == sys::PM_TOKEN_LAMBDA_BEGIN
                        || (t.kind == sys::PM_TOKEN_BRACE_LEFT && !facts.hash_braces.contains(&t.start))
                })
            };
            for toks in lines.values() {
                let n = toks.len() as isize;
                let at = |i: isize| {
                    let i = if i < 0 { n + i } else { i };
                    if i < 0 { None } else { toks.get(i as usize) }
                };
                let pos: isize = if semi(at(-1)) {
                    -1
                } else if semi(at(0)) {
                    0
                } else if kind_at(at(-2), sys::PM_TOKEN_BRACE_RIGHT) && semi(at(-3)) {
                    -3
                } else if (left_curly(at(1)) || kind_at(at(1), sys::PM_TOKEN_EMBEXPR_BEGIN)) && semi(at(2)) {
                    2
                } else if kind_at(at(2), sys::PM_TOKEN_LAMBDA_BEGIN) && semi(at(3)) {
                    3
                } else if kind_at(at(-3), sys::PM_TOKEN_EMBEXPR_END) && semi(at(-4)) {
                    -4
                } else {
                    continue;
                };
                let t = *at(pos).unwrap();
                let before = *at(if pos == -1 { -2 } else { pos }).unwrap_or(&t);
                let (s, e) = (t.start, t.start + 1);
                // The corrector's node: a range to wrap, or the arguments to parenthesize.
                let mut wrap: Option<(usize, usize)> = None;
                let mut args: Option<Args> = None;
                let tb = (before.start, before.end);
                if matches!(before.kind, sys::PM_TOKEN_DOT_DOT | sys::PM_TOKEN_DOT_DOT_DOT) {
                    wrap = facts.ranges.iter().copied().find(|r| overlaps(*r, tb));
                } else if before.kind == sys::PM_TOKEN_LABEL
                    && let Some(&(_, a)) = facts.pairs.iter().find(|p| overlaps(p.0, tb))
                {
                    // `args_begin(call)` raises when the hash is not an argument of a call, super
                    // or yield. No pair (a keyword parameter, a hash pattern): nothing to wrap.
                    let Some(a) = a else { continue };
                    args = Some(a);
                }
                cx.add_fix(s, e, MSG, |c| {
                    if let Some((a, b)) = wrap {
                        c.wrap(a, b, "(", ")");
                    }
                    if let Some((sel, first, last)) = args {
                        // `args_begin(call).begin.join(first_argument.begin)` becomes `(`.
                        c.replace(sel, first, "(");
                        c.insert_after(last.0, last.1, ")");
                    }
                    c.remove(s, e);
                });
            }
        }

        // on_begin: several expressions ending on one line.
        if cx.cfg.bool("AllowAsExpressionSeparator") == Some(true) {
            return;
        }
        let mut semis: Vec<usize> = Vec::new();
        for ((bs, be), ends) in &facts.begins {
            if !src.text[*bs..*be].contains(';') {
                continue;
            }
            let mut count: BTreeMap<usize, usize> = BTreeMap::new();
            for &end in ends {
                *count.entry(src.line_of(end)).or_default() += 1;
            }
            for (&line, _) in count.iter().filter(|(_, c)| **c > 1) {
                let tokens = src.tokens();
                semis.extend(
                    tokens
                        .iter()
                        .filter(|t| t.kind == sys::PM_TOKEN_SEMICOLON && src.line_of(t.start) == line)
                        .map(|t| t.start),
                );
            }
        }
        for s in semis {
            let line = src.line_of(s);
            let heredoc_before = facts.heredocs.iter().any(|&(hs, he)| src.line_of(hs) == line && he <= s);
            cx.add_fix(s, s + 1, MSG, |c| {
                if !heredoc_before {
                    c.replace(s, s + 1, "\n");
                }
            });
        }
    }
}

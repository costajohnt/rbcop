use crate::prelude::*;
use crate::source::Source;
use helpers::whole_lines::range_by_whole_lines;
use ruby_prism::{IfNode, Visit};
use std::cell::Cell;

pub const NAME: &str = "Style/IfInsideElse";
pub static COP: IfInsideElse = IfInsideElse;
pub struct IfInsideElse;

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// Parser `else_branch` of an if node: an elsif IfNode, or the else body (None when empty).
fn else_body<'pr>(n: &IfNode<'pr>) -> Option<Node<'pr>> {
    let sub = n.subsequent()?;
    if sub.as_if_node().is_some() {
        return Some(sub);
    }
    let stmts = sub.as_else_node()?.statements()?;
    let mut it = stmts.body().iter();
    let first = it.next()?;
    if it.next().is_some() { Some(stmts.as_node()) } else { Some(first) }
}

impl Cop for IfInsideElse {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let n = node.as_if_node().unwrap();
        if n.if_keyword_loc().is_none() || is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let Some(else_node) = n.subsequent().and_then(|s| s.as_else_node()) else { return };
        let Some(inner_node) = else_body(&n) else { return };
        let Some(inner) = inner_node.as_if_node() else { return };
        let Some(kw) = inner.if_keyword_loc().filter(|k| k.as_slice() == b"if") else { return };
        let modifier = inner.end_keyword_loc().is_none();
        if modifier && cx.cfg.bool("AllowIfModifier") == Some(true) {
            return;
        }
        let else_kw = else_node.else_keyword_loc();
        if !modifier && cx.src.comments.iter().any(|c| c.start > else_kw.end_offset() && c.start < kw.start_offset()) {
            return;
        }
        let me = (node.location().start_offset(), helpers::if_node::parser_end(node));
        let ignored = cx.state::<Vec<(usize, usize)>>().iter().any(|&(a, b)| a <= me.0 && me.1 <= b);
        let src = cx.src;
        let edits = if ignored { Vec::new() } else { autocorrect(src, &inner, &inner_node, (else_kw.start_offset(), else_kw.end_offset())) };
        let ran = Cell::new(false);
        cx.add_fix(kw.start_offset(), kw.end_offset(), "Convert `if` nested inside `else` to `elsif`.", |c| {
            if ignored {
                return;
            }
            ran.set(true);
            for e in &edits {
                match e {
                    Edit::Replace(s, e, t) => c.replace(*s, *e, t),
                    Edit::Remove(s, e) => c.remove(*s, *e),
                }
            }
        });
        if ran.get() {
            cx.state::<Vec<(usize, usize)>>().push(me);
        }
    }
}

/// Whether a strict descendant of the visited node ends on `line`.
struct EndsOnLine<'a> {
    src: &'a Source<'a>,
    line: usize,
    found: bool,
    depth: usize,
}

impl<'pr> Visit<'pr> for EndsOnLine<'_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
        self.depth += 1;
    }
    fn visit_branch_node_leave(&mut self) {
        self.depth -= 1;
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
}

impl EndsOnLine<'_> {
    fn check(&mut self, node: &Node) {
        if self.depth > 0 && self.src.line_of(node.location().end_offset()) == self.line {
            self.found = true;
        }
    }
}

enum Edit {
    Replace(usize, usize, String),
    Remove(usize, usize),
}

fn autocorrect(src: &Source, inner: &IfNode, inner_node: &Node, else_kw: (usize, usize)) -> Vec<Edit> {
    let text = src.text;
    let cond = inner.predicate();
    let (cs, ce) = range(&cond);
    let if_branch = inner.statements().map(|s| range(&s.as_node()));
    if inner.then_keyword_loc().is_some_and(|t| t.as_slice() == b"then") {
        let (s, e) = range(inner_node);
        return vec![Edit::Replace(s, e, if_then_replacement(src, inner, None))];
    }
    let mut edits = vec![Edit::Replace(else_kw.0, else_kw.1, format!("elsif {}", &text[cs..ce]))];
    if inner.end_keyword_loc().is_none() {
        // modifier form
        edits.push(Edit::Remove(if_branch.map_or(cs, |b| b.1), ce));
        return edits;
    }
    let kw = inner.if_keyword_loc().unwrap();
    let (ks, ke) = (kw.start_offset(), ce);
    if let Some((bs, mut be)) = if_branch {
        // range_with_comments(if_branch): parser's Associator gives a single statement its leading
        // comments, and a decorating comment when no descendant ends on its last line first; a
        // `begin` (2+ statements) gets none.
        let stmts: Vec<Node> = inner.statements().unwrap().body().iter().collect();
        let mut start = bs;
        if let [only] = stmts.as_slice() {
            let cond_line = src.line_of(ce);
            start = helpers::comment_association::leading(src, bs, ce, Some(cond_line))
                .iter()
                .map(|c| c.start)
                .min()
                .map_or(bs, |s| s.min(bs));
            let last_line = src.line_of(be);
            let mut v = EndsOnLine { src, line: last_line, found: false, depth: 0 };
            v.visit(only);
            if !v.found
                && let Some(c) = src.comments.iter().find(|c| c.start >= be && src.line_of(c.start) == last_line) {
                    be = c.end;
                }
        }
        edits.push(Edit::Replace(ks, ke, text[start..be].to_string()));
        let (ls, le) = range_by_whole_lines(text, start, be, true);
        edits.push(Edit::Remove(ls, le));
    } else {
        let (ls, le) = range_by_whole_lines(text, ks, ke, true);
        edits.push(Edit::Remove(ls, le));
    }
    edits.push(Edit::Remove(cs, ce));
    let end = inner.end_keyword_loc().unwrap();
    let (es, ee) = range_by_whole_lines(text, end.start_offset(), end.end_offset(), true);
    edits.push(Edit::Remove(es, ee));
    edits
}

/// IfThenCorrector#replacement with `indentation: 0`.
fn if_then_replacement(src: &Source, node: &IfNode, indentation: Option<&str>) -> String {
    let text = |n: &Node| node_text(n).to_string();
    let indentation =
        indentation.map_or_else(|| " ".repeat(src.column(node.location().start_offset())), str::to_string);
    let keyword = node.if_keyword_loc().map_or("if", |k| crate::ast::text(&k));
    let elsif_ind = if keyword == "elsif" { indentation.as_str() } else { "" };
    let if_branch = node.statements().map_or("nil".to_string(), |s| text(&s.as_node()));
    let mut out = format!("{elsif_ind}{keyword} {}\n{indentation}{if_branch}\n", text(&node.predicate()));
    match node.subsequent() {
        None => out.push_str("end"),
        Some(s) => match s.as_if_node() {
            Some(elsif) => out.push_str(&if_then_replacement(src, &elsif, Some(&indentation))),
            None => {
                // an empty `else` is parser's nil else_branch
                match s.as_else_node().unwrap().statements() {
                    None => out.push_str("end"),
                    Some(b) => {
                        let body = text(&b.as_node());
                        out.push_str(&format!("{indentation}else\n{indentation}{body}\n{indentation}end"));
                    }
                }
            }
        },
    }
    out
}

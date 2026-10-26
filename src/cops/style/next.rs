use crate::prelude::*;
use ruby_prism::{Location, StatementsNode};
use std::collections::HashMap;

pub const NAME: &str = "Style/Next";
pub static COP: Next = Next;
pub struct Next;

/// The parts of a parser `if` (an IfNode or UnlessNode).
struct If<'pr> {
    pred: Node<'pr>,
    body: Option<StatementsNode<'pr>>,
    has_else: bool,
    ternary: bool,
    keyword: Location<'pr>,
    end_kw: Option<Location<'pr>>,
    inverse: &'static str,
}

fn if_parts<'pr>(n: &Node<'pr>) -> Option<If<'pr>> {
    if let Some(i) = n.as_if_node() {
        let keyword = i.if_keyword_loc().unwrap_or_else(|| i.predicate().location());
        return Some(If {
            pred: i.predicate(),
            body: i.statements(),
            has_else: i.subsequent().is_some(),
            ternary: i.if_keyword_loc().is_none(),
            keyword,
            end_kw: i.end_keyword_loc(),
            inverse: "unless",
        });
    }
    let u = n.as_unless_node()?;
    Some(If {
        pred: u.predicate(),
        body: u.statements(),
        has_else: u.else_clause().is_some(),
        ternary: false,
        keyword: u.keyword_loc(),
        end_kw: u.end_keyword_loc(),
        inverse: "if",
    })
}

fn single<'pr>(s: &Option<StatementsNode<'pr>>) -> Option<Node<'pr>> {
    let s = s.as_ref()?;
    let mut it = s.body().iter();
    let first = it.next()?;
    it.next().is_none().then_some(first)
}

/// `else?` of a parser if child (a ternary has no `loc.else`).
fn if_with_else(n: &Node) -> bool {
    if_parts(n).is_some_and(|p| p.has_else && !p.ternary)
}

fn statements_of<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    let stmts = match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            let name = method_name(&call);
            if !is_enumerator_method(name) {
                return None;
            }
            // a body with rescue/ensure is a parser `rescue`/`ensure` node, never an `if`
            call.block()?.as_block_node()?.body()?.as_statements_node()?
        }
        Node::WhileNode { .. } => {
            let w = node.as_while_node().unwrap();
            if w.is_begin_modifier() {
                return None;
            }
            w.statements()?
        }
        Node::UntilNode { .. } => {
            let w = node.as_until_node().unwrap();
            if w.is_begin_modifier() {
                return None;
            }
            w.statements()?
        }
        _ => node.as_for_node()?.statements()?,
    };
    Some(stmts.body().iter().collect())
}

fn source_line<'a>(src: &'a crate::source::Source, line: usize) -> &'a str {
    src.lines.get(line - 1).copied().unwrap_or("")
}

/// Index of the first non-whitespace character (`line =~ /\S/`).
fn indent_of(line: &str) -> Option<usize> {
    line.chars().position(|c| !helpers::ruby_string::is_space(c))
}

impl Next {
    fn simple_if_without_break(cx: &Cx, n: &Node) -> bool {
        let Some(p) = if_parts(n) else { return false };
        if p.ternary || p.has_else {
            return false;
        }
        if if_with_else(&p.pred) || single(&p.body).is_some_and(|b| if_with_else(&b)) {
            return false;
        }
        let allowed = match &p.end_kw {
            None => cx.cfg.str("EnforcedStyle") == Some("skip_modifier_ifs"),
            Some(e) => {
                let min = cx.cfg.positive_int("MinBodyLength", 1);
                cx.src.line_of(e.start_offset()) - cx.src.line_of(p.keyword.start_offset()) <= min
            }
        };
        !allowed && !single(&p.body).is_some_and(|b| matches!(kind::of(&b), kind::BREAK_NODE | kind::RETURN_NODE))
    }
}

impl Cop for Next {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::WHILE_NODE, kind::UNTIL_NODE, kind::FOR_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(stmts) = statements_of(node) else { return };
        let last = stmts.last().unwrap();
        if !Self::simple_if_without_break(cx, last) {
            return;
        }
        // offense_node: `*_, condition = *body` (an `unless` body's last child is its own body)
        let offending = match stmts.as_slice() {
            [one] => one
                .as_unless_node()
                .and_then(|u| single(&u.statements()))
                .filter(|b| if_parts(b).is_some())
                .unwrap_or_else(|| if_to_owned(one)),
            _ => if_to_owned(last),
        };
        if cx.cfg.bool("AllowConsecutiveConditionals") == Some(true)
            && stmts.len() > 1
            && if_parts(&stmts[stmts.len() - 2]).is_some()
        {
            return;
        }
        let p = if_parts(&offending).unwrap();
        let loc = offending.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let pred_loc = p.pred.location();
        let cond_src = text(&pred_loc);
        let src = cx.src;
        let msg = "Use `next` to skip iteration.";
        if p.end_kw.is_none() && !p.ternary {
            let body_end = p.body.as_ref().map_or(end, |b| b.location().end_offset());
            let next_code = format!("next {} {cond_src}\n{}", p.inverse, " ".repeat(src.column(start)));
            return cx.add_fix(start, pred_loc.end_offset(), msg, |c| {
                c.insert_before(start, end, &next_code);
                c.remove(body_end, end);
            });
        }
        // autocorrect_block on a ternary crashes (no loc.begin / loc.end): no offense
        let Some(end_kw) = p.end_kw.as_ref().filter(|_| !p.ternary) else { return };
        let next_code = format!("next {} {cond_src}", p.inverse);
        let cond_end = helpers::if_node::parser_begin_end(&offending).unwrap_or(pred_loc.end_offset());
        let (end_start, end_end) = (end_kw.start_offset(), end_kw.end_offset());
        let rest = &src.text.as_bytes()[end_end..];
        let ws = rest.iter().take_while(|&&b| helpers::parser_tokens::is_ruby_space(b)).count();
        let mut end_begin = src.line_start(src.line_of(end_start));
        if rest[..ws].contains(&b'\n') || ws == rest.len() {
            end_begin -= 1;
        }
        let heredoc = string_content_lines(src, &offending);
        let lines: Vec<usize> = (src.line_of(start) + 1..src.line_of(end_start))
            .filter(|l| !heredoc.contains(l) && !source_line(src, *l).trim_ascii().is_empty())
            .collect();
        let target = indent_of(source_line(src, src.line_of(pred_loc.start_offset()))).unwrap_or(0) as i64;
        let delta = lines.iter().filter_map(|&l| indent_of(source_line(src, l))).min().unwrap_or(0) as i64 - target;
        let mut reindented = std::mem::take(cx.state::<HashMap<usize, i64>>());
        cx.add_fix(start, pred_loc.end_offset(), msg, |c| {
            c.insert_before(start, end, &next_code);
            c.remove(start, cond_end);
            c.remove(end_begin, end_end);
            for &l in &lines {
                let adj = delta + reindented.get(&l).copied().unwrap_or(0);
                reindented.insert(l, adj);
                if adj > 0 {
                    let ls = src.line_start(l);
                    let line = source_line(src, l);
                    let bytes: usize = line.chars().take(adj as usize).map(char::len_utf8).sum();
                    c.remove_leading(ls, ls + line.len(), bytes);
                }
            }
        });
        *cx.state::<HashMap<usize, i64>>() = reindented;
    }
}

/// Re-derive an owned handle to an if-type node (Prism nodes are not `Clone`).
fn if_to_owned<'pr>(n: &Node<'pr>) -> Node<'pr> {
    match n.as_if_node() {
        Some(i) => i.as_node(),
        None => n.as_unless_node().unwrap().as_node(),
    }
}

/// `string_content_lines`: lines that start inside a string literal or a heredoc body, whose
/// leading whitespace is part of the string value.
fn string_content_lines(src: &crate::source::Source, node: &Node) -> Vec<usize> {
    let mut v = StringContents { src, lines: Vec::new() };
    ruby_prism::Visit::visit(&mut v, node);
    v.lines
}

struct StringContents<'s, 'a> {
    src: &'s crate::source::Source<'a>,
    lines: Vec<usize>,
}

impl StringContents<'_, '_> {
    fn add(&mut self, node: &Node, open: Option<Location>, close: Option<Location>) {
        if crate::ast::is_heredoc(node) {
            if let Some((b, e)) = helpers::heredoc::body_range(node).filter(|(b, e)| b < e) {
                self.lines.extend(self.src.line_of(b)..self.src.line_of(e));
            }
            return;
        }
        let (Some(o), Some(c)) = (open, close) else { return };
        let (b, e) = (o.end_offset(), c.start_offset());
        if b >= e {
            return;
        }
        let src = self.src;
        self.lines.extend((src.line_of(b)..=src.line_of(e)).filter(|&l| (b..e).contains(&src.line_start(l))));
    }
}

impl<'pr> ruby_prism::Visit<'pr> for StringContents<'_, '_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
        self.add(&node.as_node(), node.opening_loc(), node.closing_loc());
    }
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        self.add(&node.as_node(), node.opening_loc(), node.closing_loc());
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode<'pr>) {
        self.add(&node.as_node(), Some(node.opening_loc()), Some(node.closing_loc()));
    }
    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode<'pr>) {
        self.add(&node.as_node(), Some(node.opening_loc()), Some(node.closing_loc()));
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}

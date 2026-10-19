use crate::helpers::heredoc;
use crate::helpers::node_equality;
use crate::helpers::parser_parent::{parser_parent, parser_parent_index};
use crate::helpers::range_help::{range_with_surrounding_space, Side, Surrounding};
use crate::helpers::statement_modifier::{self as sm, Stmt};
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;
use ruby_prism::Visit;
use std::cell::Cell;

pub const NAME: &str = "Style/IfUnlessModifier";
pub static COP: IfUnlessModifier = IfUnlessModifier;
pub struct IfUnlessModifier;

const MSG_USE_MODIFIER: &str = "Favor modifier `%s` usage when having a single-line body. Another good alternative is the usage of control flow `&&`/`||`.";
const MSG_USE_MODIFIER_PARENS: &str = "Favor modifier `%s` usage when having a single-line body. Wrap the expression in parentheses to keep the current behavior, as it is part of a larger expression.";
const MSG_USE_NORMAL: &str = "Modifier form of `%s` makes the line too long.";

#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// A guard `in pat if cond`: Prism's IfNode/UnlessNode wrapping the pattern (parser `if_guard`).
fn guard_parts<'pr>(pattern: &Node<'pr>) -> Option<(Node<'pr>, Option<ruby_prism::StatementsNode<'pr>>)> {
    match pattern {
        Node::IfNode { .. } => {
            let n = pattern.as_if_node().unwrap();
            Some((n.predicate(), n.statements()))
        }
        Node::UnlessNode { .. } => {
            let n = pattern.as_unless_node().unwrap();
            Some((n.predicate(), n.statements()))
        }
        _ => None,
    }
}

/// Parser `if` nodes in a subtree (pattern guards excluded), plus defined?/pattern-match scans.
#[derive(Default)]
struct Scan<'pr> {
    ifs: Vec<Node<'pr>>,
    defined: Vec<Node<'pr>>,
    pattern_match: bool,
}

impl<'pr> Visit<'pr> for Scan<'pr> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        self.ifs.push(node.as_node());
        ruby_prism::visit_if_node(self, node);
    }
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        self.ifs.push(node.as_node());
        ruby_prism::visit_unless_node(self, node);
    }
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        match guard_parts(&node.pattern()) {
            Some((pred, stmts)) => {
                self.visit(&pred);
                if let Some(s) = stmts {
                    self.visit_statements_node(&s);
                }
            }
            None => self.visit(&node.pattern()),
        }
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }
    fn visit_defined_node(&mut self, node: &ruby_prism::DefinedNode<'pr>) {
        self.defined.push(node.as_node());
        ruby_prism::visit_defined_node(self, node);
    }
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.pattern_match = true;
        ruby_prism::visit_match_predicate_node(self, node);
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.pattern_match = true;
        ruby_prism::visit_match_required_node(self, node);
    }
}

fn scan<'pr>(node: &Node<'pr>) -> Scan<'pr> {
    let mut s = Scan::default();
    s.visit(node);
    s
}

fn is_parser_begin(n: &Node) -> bool {
    matches!(n, Node::ParenthesesNode { .. } | Node::EmbeddedStatementsNode { .. } | Node::StatementsNode { .. })
}

/// Parser children of a begin-like node.
fn begin_children<'pr>(n: &Node<'pr>) -> Vec<Node<'pr>> {
    fn stmts<'pr>(s: Option<Node<'pr>>) -> Vec<Node<'pr>> {
        match s {
            None => Vec::new(),
            Some(s) => match s.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => vec![s],
            },
        }
    }
    match n {
        Node::ParenthesesNode { .. } => stmts(n.as_parentheses_node().unwrap().body()),
        Node::EmbeddedStatementsNode { .. } => stmts(n.as_embedded_statements_node().unwrap().statements().map(|s| s.as_node())),
        _ => n.as_statements_node().map_or(Vec::new(), |s| s.body().iter().collect()),
    }
}

/// find_containing_collection: an array, a send/csend or (via a pair) a hash holding the node.
fn find_containing_collection<'a, 'pr>(ancestors: &'a [Node<'pr>]) -> Option<&'a Node<'pr>> {
    let mut i = parser_parent_index(ancestors, ancestors.len())?;
    if is_parser_begin(&ancestors[i]) {
        i = parser_parent_index(ancestors, i)?;
    }
    match &ancestors[i] {
        Node::ArrayNode { .. } => Some(&ancestors[i]),
        Node::CallNode { .. } if ancestors.get(i + 1).is_none_or(|n| n.as_block_node().is_none()) => Some(&ancestors[i]),
        Node::AssocNode { .. } => parser_parent_index(ancestors, i).map(|p| &ancestors[p]),
        _ => None,
    }
}

/// Parser children of a collection (nodes only).
fn collection_children<'pr>(c: &Node<'pr>) -> Vec<Node<'pr>> {
    match c {
        Node::ArrayNode { .. } => c.as_array_node().unwrap().elements().iter().collect(),
        Node::HashNode { .. } => c.as_hash_node().unwrap().elements().iter().collect(),
        Node::KeywordHashNode { .. } => c.as_keyword_hash_node().unwrap().elements().iter().collect(),
        Node::CallNode { .. } => {
            let call = c.as_call_node().unwrap();
            call.receiver().into_iter().chain(send_args(&call)).collect()
        }
        _ => Vec::new(),
    }
}

/// `if`/`unless` end line (`loc.end`), if any.
fn end_line(cx: &Cx, n: &Node) -> Option<usize> {
    Stmt::of(n).and_then(|s| s.end_loc).map(|(s, _)| cx.src.line_of(s))
}

fn multiline_inside_collection(cx: &Cx, s: &Stmt) -> bool {
    if s.modifier_form() {
        return false;
    }
    let Some(collection) = find_containing_collection(cx.ancestors) else { return false };
    let node_first = cx.src.line_of(s.range.0);
    let node_end = s.end_loc.map(|(a, _)| cx.src.line_of(a));
    collection_children(collection).into_iter().any(|child| {
        let mut inner = child;
        if let Some(p) = inner.as_assoc_node() {
            inner = p.value();
        }
        if is_parser_begin(&inner) {
            match begin_children(&inner).into_iter().next() {
                Some(f) => inner = f,
                None => return false,
            }
        }
        let is_if = matches!(inner, Node::IfNode { .. } | Node::UnlessNode { .. })
            && !crate::helpers::parser_parent::is_ternary(&inner);
        is_if
            && (Some(cx.src.line_of(inner.location().start_offset())) == node_end
                || end_line(cx, &inner).is_some_and(|l| l == node_first))
    })
}

fn another_modifier_if_on_same_line(cx: &Cx, s: &Stmt) -> bool {
    let Some(collection) = find_containing_collection(cx.ancestors) else { return false };
    let found: Vec<Scan> = match collection {
        Node::CallNode { .. } => collection_children(collection).iter().map(scan).collect(),
        _ => vec![scan(collection)],
    };
    let line = cx.src.line_of(s.range.0);
    found.iter().any(|found| {
        found.ifs.iter().any(|n| {
            let nr = range(n);
            nr != range(collection)
                && !node_equality::same(cx.src, nr, s.range)
                && Stmt::of(n).is_some_and(|m| m.modifier_form() && matches!(m.keyword, "if" | "unless"))
                && cx.src.line_of(nr.0) == line
        })
    })
}

/// another_statement_on_same_line?: the next statement in the enclosing parser `begin`
/// starts on the node's last line.
fn another_statement_on_same_line(cx: &Cx, s: &Stmt) -> bool {
    let line_no = cx.src.line_of(s.range.1);
    let anc = cx.ancestors;
    let mut child = s.range;
    for i in (0..anc.len()).rev() {
        let a = &anc[i];
        let begin = match a {
            Node::StatementsNode { .. } => {
                let n = a.as_statements_node().unwrap().body().iter().count();
                // A plain `begin..end` holds its statements directly (kwbegin).
                let kwbegin = i > 0
                    && anc[i - 1].as_begin_node().is_some_and(|b| {
                        b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none()
                    });
                n > 1 && !kwbegin
            }
            Node::ParenthesesNode { .. } | Node::EmbeddedStatementsNode { .. } => true,
            _ => false,
        };
        if begin {
            let children = begin_children(a);
            let Some(idx) = children.iter().position(|c| range(c) == child) else { return false };
            return children.get(idx + 1).is_some_and(|n| cx.src.line_of(n.location().start_offset()) == line_no);
        }
        if !matches!(a, Node::StatementsNode { .. } | Node::ArgumentsNode { .. } | Node::ElseNode { .. }) {
            child = range(a);
        }
    }
    false
}

/// defined_argument_is_undefined?
fn defined_argument_is_undefined(cx: &Cx, if_range: (usize, usize), defined: &Node) -> bool {
    let arg = defined.as_defined_node().unwrap().value();
    if arg.as_call_node().is_some_and(|c| !is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none())) {
        return true; // `node_parts[0]` of a send is its receiver: never a sibling's name
    }
    let Some(lvar) = arg.as_local_variable_read_node() else { return false };
    let name = lvar.name().as_slice();
    let Some(parent) = parser_parent(cx.ancestors) else { return true };
    let left: Vec<Node> = match parent {
        Node::StatementsNode { .. } => {
            parent.as_statements_node().unwrap().body().iter().take_while(|n| range(n) != if_range).collect()
        }
        Node::IfNode { .. } => vec![parent.as_if_node().unwrap().predicate()],
        Node::UnlessNode { .. } => vec![parent.as_unless_node().unwrap().predicate()],
        Node::WhileNode { .. } => vec![parent.as_while_node().unwrap().predicate()],
        Node::UntilNode { .. } => vec![parent.as_until_node().unwrap().predicate()],
        Node::AndNode { .. } => vec![parent.as_and_node().unwrap().left()],
        Node::OrNode { .. } => vec![parent.as_or_node().unwrap().left()],
        _ => Vec::new(),
    };
    !left
        .iter()
        .any(|n| range(n) != if_range && n.as_local_variable_write_node().is_some_and(|w| w.name().as_slice() == name))
}

/// The modifier-form autocorrection (MSG_USE_NORMAL): (removals, replacement).
fn replacement_for_modifier_form(cx: &Cx, s: &Stmt, max: f64) -> (Vec<(usize, usize)>, String) {
    let src = cx.src;
    let indent = " ".repeat(src.column(s.range.0));
    let body = s.body.as_ref().unwrap();
    let kw = s.keyword;
    let cond = node_text(&s.condition);
    let first = src.line_of(s.range.0);
    if let Some(c) = src.comment_on_line(first) {
        let ctext = src.slice(c.start, c.end);
        let len = src.lines[first - 1].chars().count();
        let clen = ctext.chars().count();
        if len.saturating_sub(clen) as f64 <= max && max <= len as f64 && clen <= len {
            let opts = Surrounding { side: Side::Left, ..Default::default() };
            let removal = range_with_surrounding_space(src.text, c.start, c.end, opts);
            return (vec![removal], format!("{ctext}\n{indent}{} {kw} {cond}", node_text(body)));
        }
    }
    let last_arg = body
        .as_call_node()
        .filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
        .and_then(|c| send_args(&c).into_iter().last());
    if let Some(arg) = last_arg
        && let Some((_, (end_s, end_e), _)) = heredoc::parts(&arg)
    {
        let body_start = heredoc_body_start(&arg).map_or(end_s, |f| src.line_start(src.line_of(f)));
        let body_src = src.slice(body_start, end_s);
        let body_src = body_src.strip_suffix('\n').unwrap_or(body_src);
        let end_src = src.slice(end_s, end_e);
        let removals = vec![
            range_by_whole_lines(src.text, body_start, end_s, true),
            range_by_whole_lines(src.text, end_s, end_e, true),
        ];
        let text = format!(
            "{kw} {cond}\n{indent}  {}\n{indent}  {body_src}\n{indent}  {end_src}\n{indent}end",
            node_text(body)
        );
        return (removals, text);
    }
    (Vec::new(), format!("{kw} {cond}\n{indent}  {}\n{indent}end", node_text(body)))
}

/// Offset of the first body character of a heredoc (None when the body is empty).
fn heredoc_body_start(node: &Node) -> Option<usize> {
    let first = match node {
        Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            let c = s.content_loc();
            if c.start_offset() == c.end_offset() {
                return None;
            }
            c.start_offset()
        }
        Node::XStringNode { .. } => {
            let c = node.as_x_string_node().unwrap().content_loc();
            if c.start_offset() == c.end_offset() {
                return None;
            }
            c.start_offset()
        }
        Node::InterpolatedStringNode { .. } => node.as_interpolated_string_node().unwrap().parts().iter().next()?.location().start_offset(),
        _ => node.as_interpolated_x_string_node()?.parts().iter().next()?.location().start_offset(),
    };
    Some(first)
}

impl Cop for IfUnlessModifier {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let Some(s) = Stmt::of(node) else { return };
        if !matches!(s.keyword, "if" | "unless") {
            return; // elsif / ternary: no message
        }
        if s.body.as_ref().and_then(|b| b.as_def_node()).is_some_and(|d| d.end_keyword_loc().is_none() && d.equal_loc().is_some())
        {
            return;
        }
        if cx.ancestors.iter().any(|a| a.as_interpolated_string_node().is_some()) {
            return;
        }
        let cond_scan = scan(&s.condition);
        if cond_scan.defined.iter().any(|d| defined_argument_is_undefined(cx, s.range, d)) || cond_scan.pattern_match {
            return;
        }
        let parenthesize = sm::parenthesize(cx.ancestors);
        let msg = if self.single_line_as_modifier(cx, node, &s, parenthesize) && !named_capture(&s.condition) {
            if parenthesize { MSG_USE_MODIFIER_PARENS } else { MSG_USE_MODIFIER }
        } else if s.modifier_form() && too_long_single_line(cx, &s) && !another_statement_on_same_line(cx, &s) {
            MSG_USE_NORMAL
        } else {
            return;
        };
        let (start, end) = s.range;
        let skip = cx.state::<Ignored>().0.iter().any(|&(a, b)| a <= start && end <= b)
            || another_modifier_if_on_same_line(cx, &s);
        let (removals, replacement) = if s.modifier_form() {
            replacement_for_modifier_form(cx, &s, sm::max_line_length(cx).unwrap_or(f64::INFINITY))
        } else {
            (Vec::new(), sm::to_modifier_form(cx, &s, parenthesize))
        };
        let ran = Cell::new(false);
        let (ks, ke) = s.keyword_loc;
        cx.add_fix(ks, ke, msg.replace("%s", s.keyword), |c| {
            if skip {
                return;
            }
            for &(a, b) in &removals {
                c.remove(a, b);
            }
            c.replace(start, end, &replacement);
            ran.set(true);
        });
        if ran.get() {
            cx.state::<Ignored>().0.push((start, end));
        }
    }
}

impl IfUnlessModifier {
    fn single_line_as_modifier(&self, cx: &Cx, node: &Node, s: &Stmt, parenthesize: bool) -> bool {
        // IfUnlessModifier#non_eligible_node?
        let has_else = crate::helpers::if_node::cond(node).is_some_and(|c| c.has_else);
        let chained = cx.ancestors.last().and_then(|p| p.as_call_node()).and_then(|c| c.receiver()).is_some_and(|r| range(&r) == s.range);
        let nested = s.body.as_ref().is_some_and(|b| {
            scan(b).ifs.iter().any(|n| n.as_if_node().is_none_or(|i| i.if_keyword_loc().is_none_or(|k| k.as_slice() != b"elsif")))
        });
        if has_else || chained || nested || multiline_inside_collection(cx, s) || sm::non_eligible_node(cx, s) {
            return false;
        }
        if sm::non_eligible_body(cx, s.body.as_ref()) || sm::non_eligible_condition(&s.condition) {
            return false;
        }
        sm::modifier_fits_on_single_line(cx, s, parenthesize)
    }
}

/// named_capture_in_condition?: parser `match_with_lvasgn` (a static regexp literal `=~`).
fn named_capture(cond: &Node) -> bool {
    if cond.as_match_write_node().is_some() {
        return true;
    }
    cond.as_call_node().is_some_and(|c| {
        method_name(&c) == "=~"
            && c.call_operator_loc().is_none()
            && c.receiver().is_some_and(|r| r.as_regular_expression_node().is_some())
    })
}

fn too_long_single_line(cx: &Cx, s: &Stmt) -> bool {
    let (a, b) = (cx.src.line_of(s.range.0), cx.src.line_of(s.range.1));
    a == b && !sm::acceptable_line_length(cx, cx.src.lines[a - 1], a)
}

use crate::prelude::*;
use crate::source::Source;
use helpers::node_types::is_assignment;
use helpers::range_help::{Surrounding, range_with_surrounding_space};
use helpers::whole_lines::range_by_whole_lines;
use ruby_prism::Visit;
use std::collections::HashSet;

pub const NAME: &str = "Style/SoleNestedConditional";
pub static COP: SoleNestedConditional = SoleNestedConditional;
pub struct SoleNestedConditional;

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// rubocop-ast IfNode view of a Prism IfNode / UnlessNode.
struct If<'pr> {
    node: Node<'pr>,
    keyword: (usize, usize),
    unless: bool,
    condition: Node<'pr>,
    body: Option<ruby_prism::StatementsNode<'pr>>,
    has_else: bool,
    end: Option<(usize, usize)>,
}

fn as_if<'pr>(node: &Node<'pr>) -> Option<If<'pr>> {
    let loc = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
    if let Some(i) = node.as_if_node() {
        let kw = i.if_keyword_loc()?;
        if kw.as_slice() != b"if" {
            return None;
        }
        return Some(If {
            node: i.as_node(),
            keyword: loc(kw),
            unless: false,
            condition: i.predicate(),
            body: i.statements(),
            has_else: i.subsequent().is_some(),
            end: i.end_keyword_loc().map(loc),
        });
    }
    let u = node.as_unless_node()?;
    Some(If {
        node: u.as_node(),
        keyword: loc(u.keyword_loc()),
        unless: true,
        condition: u.predicate(),
        body: u.statements(),
        has_else: u.else_clause().is_some(),
        end: u.end_keyword_loc().map(loc),
    })
}

impl<'pr> If<'pr> {
    fn modifier(&self) -> bool {
        self.end.is_none()
    }
    /// `if_branch`: the sole body statement (a multi-statement body is a parser `begin`).
    fn if_branch(&self) -> Option<Node<'pr>> {
        let body: Vec<Node> = self.body.as_ref()?.body().iter().collect();
        match <[Node; 1]>::try_from(body) {
            Ok([only]) => Some(only),
            Err(_) => None,
        }
    }
}

/// Names `children.first.to_s` gives for assignment nodes in and below `node` (parser puts an
/// `lvasgn`-like node inside op/or/and assignments and multiple-assignment targets).
#[derive(Default)]
struct AssignedNames(Vec<Vec<u8>>);

impl<'pr> Visit<'pr> for AssignedNames {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.record(&node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.record(&node);
    }
}

impl AssignedNames {
    fn record(&mut self, n: &Node) {
        let name = match n {
            Node::LocalVariableWriteNode { .. } => n.as_local_variable_write_node().unwrap().name(),
            Node::LocalVariableOperatorWriteNode { .. } => n.as_local_variable_operator_write_node().unwrap().name(),
            Node::LocalVariableOrWriteNode { .. } => n.as_local_variable_or_write_node().unwrap().name(),
            Node::LocalVariableAndWriteNode { .. } => n.as_local_variable_and_write_node().unwrap().name(),
            Node::LocalVariableTargetNode { .. } => n.as_local_variable_target_node().unwrap().name(),
            Node::InstanceVariableWriteNode { .. } => n.as_instance_variable_write_node().unwrap().name(),
            Node::InstanceVariableOperatorWriteNode { .. } => {
                n.as_instance_variable_operator_write_node().unwrap().name()
            }
            Node::InstanceVariableOrWriteNode { .. } => n.as_instance_variable_or_write_node().unwrap().name(),
            Node::InstanceVariableAndWriteNode { .. } => n.as_instance_variable_and_write_node().unwrap().name(),
            Node::InstanceVariableTargetNode { .. } => n.as_instance_variable_target_node().unwrap().name(),
            Node::ClassVariableWriteNode { .. } => n.as_class_variable_write_node().unwrap().name(),
            Node::ClassVariableOperatorWriteNode { .. } => n.as_class_variable_operator_write_node().unwrap().name(),
            Node::ClassVariableOrWriteNode { .. } => n.as_class_variable_or_write_node().unwrap().name(),
            Node::ClassVariableAndWriteNode { .. } => n.as_class_variable_and_write_node().unwrap().name(),
            Node::ClassVariableTargetNode { .. } => n.as_class_variable_target_node().unwrap().name(),
            Node::GlobalVariableWriteNode { .. } => n.as_global_variable_write_node().unwrap().name(),
            Node::GlobalVariableOperatorWriteNode { .. } => n.as_global_variable_operator_write_node().unwrap().name(),
            Node::GlobalVariableOrWriteNode { .. } => n.as_global_variable_or_write_node().unwrap().name(),
            Node::GlobalVariableAndWriteNode { .. } => n.as_global_variable_and_write_node().unwrap().name(),
            Node::GlobalVariableTargetNode { .. } => n.as_global_variable_target_node().unwrap().name(),
            _ => return,
        };
        self.0.push(name.as_slice().to_vec());
    }
}

/// Any assignment strictly below `node`.
struct HasAssignment<'a> {
    root: (usize, usize),
    kind: usize,
    found: &'a mut bool,
}

impl<'pr> Visit<'pr> for HasAssignment<'_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        if (span(&node) != self.root || kind::of(&node) != self.kind) && is_assignment(&node) {
            *self.found = true;
        }
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.visit_branch_node_enter(node);
    }
}

fn has_args(c: &ruby_prism::CallNode) -> bool {
    !call_args(c).is_empty() || c.block().is_some_and(|b| b.as_block_argument_node().is_some())
}

/// `add_parentheses?` (a call with a literal block is checked as its `send_node`).
fn add_parentheses(n: &Node) -> bool {
    if is_assignment(n) || n.as_or_node().is_some() {
        return true;
    }
    if n.as_and_node().is_some() {
        let mut found = false;
        HasAssignment { root: span(n), kind: kind::of(n), found: &mut found }.visit(n);
        return found;
    }
    // `/re/ =~ x` with a static regexp is parser's `match_with_lvasgn`, not a `call` (#391).
    let Some(c) = n.as_call_node().filter(|c| !match_with_lvasgn(c)) else { return false };
    let parenthesized = c.closing_loc().is_some_and(|l| l.as_slice() == b")");
    (has_args(&c) && !parenthesized) || (method_name(&c) == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"not"))
}

fn match_with_lvasgn(c: &ruby_prism::CallNode) -> bool {
    c.name().as_slice() == b"=~" && c.receiver().is_some_and(|r| r.as_regular_expression_node().is_some())
}

/// `add_parentheses_if_needed`
fn add_parentheses_if_needed(src: &str, cond: &Node) -> String {
    if !add_parentheses(cond) {
        return node_text(cond).to_string();
    }
    let block_call = cond.as_call_node().is_some_and(|c| c.block().is_some_and(|b| b.as_block_node().is_some()));
    if !block_call
        && let Some(c) = cond.as_call_node()
    {
        let name = method_name(&c);
        let parenthesized = c.closing_loc().is_some_and(|l| l.as_slice() == b")");
        let comparison = matches!(name, "==" | "===" | "!=" | "<=" | ">=" | ">" | "<");
        if has_args(&c) && !parenthesized && !comparison && !helpers::node_types::is_operator_method(name) {
            let start = cond.location().start_offset();
            let sel = c.message_loc().map_or(start, |m| m.end_offset());
            let first = call_args(&c).first().map_or_else(
                || c.block().unwrap().location().start_offset(),
                |a| a.location().start_offset(),
            );
            return format!("{}({})", &src[start..sel], &src[first..cond.location().end_offset()]);
        }
    }
    if let Some(a) = cond.as_and_node() {
        return parenthesized_and(src, &a);
    }
    format!("({})", node_text(cond))
}

fn parenthesized_and(src: &str, a: &ruby_prism::AndNode) -> String {
    let op = a.operator_loc();
    let o = Surrounding { whitespace: true, ..Default::default() };
    let (os, oe) = range_with_surrounding_space(src, op.start_offset(), op.end_offset(), o);
    let rhs = a.right();
    let rhs = if let Some(inner) = rhs.as_and_node() {
        parenthesized_and(src, &inner)
    } else if is_assignment(&rhs) {
        format!("({})", node_text(&rhs))
    } else {
        node_text(&rhs).to_string()
    };
    format!("{}{}{}", node_text(&a.left()), &src[os..oe], rhs)
}

fn chainable_condition(src: &str, i: &If) -> String {
    let wrapped = add_parentheses_if_needed(src, &i.condition);
    if !i.unless {
        wrapped
    } else if i.condition.as_and_node().is_some() {
        format!("!({wrapped})")
    } else {
        format!("!{wrapped}")
    }
}

enum Edit {
    Replace(usize, usize, String),
    Remove(usize, usize),
    InsertBefore(usize, usize, String),
    InsertAfter(usize, usize, String),
}

fn apply(c: &mut Corrector, edits: &[Edit]) {
    for e in edits {
        match e {
            Edit::Replace(s, t, x) => c.replace(*s, *t, x),
            Edit::Remove(s, t) => c.remove(*s, *t),
            Edit::InsertBefore(s, t, x) => c.insert_before(*s, *t, x),
            Edit::InsertAfter(s, t, x) => c.insert_after(*s, *t, x),
        }
    }
}

fn correct_node(src: &str, edits: &mut Vec<Edit>, i: &If) {
    if i.unless {
        edits.push(Edit::Replace(i.keyword.0, i.keyword.1, "if".into()));
    }
    let (cs, ce) = span(&i.condition);
    edits.push(Edit::Replace(cs, ce, chainable_condition(src, i)));
}

fn autocorrect(src: &Source, node: &If, branch: &If) -> Vec<Edit> {
    let text: &str = src.text;
    let mut edits = vec![];
    let no_newlines = Surrounding { newlines: false, ..Default::default() };
    if node.modifier() {
        correct_node(text, &mut edits, branch);
        let (bs, be) = span(&branch.condition);
        edits.push(Edit::InsertBefore(bs, be, format!("{} && ", chainable_condition(text, node))));
        let (rs, re) = range_with_surrounding_space(text, node.keyword.0, span(&node.condition).1, no_newlines);
        edits.push(Edit::Remove(rs, re));
        return edits;
    }
    correct_node(text, &mut edits, node);
    let (ncs, nce) = span(&node.condition);
    let (bcs, bce) = span(&branch.condition);
    if branch.modifier() {
        edits.push(Edit::InsertAfter(ncs, nce, format!(" && {}", chainable_condition(text, branch))));
        let (rs, re) = range_with_surrounding_space(text, branch.keyword.0, bce, no_newlines);
        edits.push(Edit::Remove(rs, re));
        return edits;
    }
    edits.push(Edit::Replace(nce, bcs, " && ".into()));
    edits.push(Edit::Replace(bcs, bce, chainable_condition(text, branch)));
    let (es, ee) = node.end.unwrap();
    let branch_end = branch.end.unwrap();
    let end_range = if src.line_of(es) == src.line_of(branch_end.0) {
        (es, ee)
    } else {
        range_by_whole_lines(text, es, ee, true)
    };
    edits.push(Edit::Remove(end_range.0, end_range.1));
    // correct_for_comment: leading comments of the inner conditional above its condition line.
    let (bs, _) = span(&branch.node);
    let cond_line = src.line_of(bcs);
    let comments: Vec<&str> = helpers::comment_association::leading(src, bs, nce, Some(src.line_of(nce)))
        .into_iter()
        .filter(|c| src.line_of(c.start) < cond_line)
        .map(|c| src.slice(c.start, c.end))
        .collect();
    if !comments.is_empty() {
        edits.push(Edit::InsertBefore(node.keyword.0, node.keyword.1, format!("{}\n", comments.join("\n"))));
    }
    edits
}

#[derive(Default)]
struct State {
    ignored: HashSet<(usize, usize)>,
}

impl Cop for SoleNestedConditional {
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
        let Some(outer) = as_if(node) else { return };
        if outer.has_else {
            return;
        }
        let Some(branch_node) = outer.if_branch() else { return };
        let Some(branch) = as_if(&branch_node) else { return };
        // use_variable_assignment_in_condition?
        let mut names = AssignedNames::default();
        names.visit(&outer.condition);
        if names.0.iter().any(|n| n.as_slice() == branch.condition.location().as_slice()) {
            return;
        }
        if branch.has_else || ((outer.modifier() || branch.modifier()) && cx.cfg.bool("AllowModifier") == Some(true)) {
            return;
        }
        let edits = autocorrect(cx.src, &outer, &branch);
        // correction_parses?
        let mut probe = Corrector::new(cx.facts.text.clone());
        apply(&mut probe, &edits);
        if probe.clobbered()
            || !helpers::reparsed_equivalence::valid_syntax(&probe.rewrite(), cx.src.version, cx.src.legacy())
        {
            return;
        }
        let msg = format!(
            "Consider merging nested conditions into outer `{}` conditions.",
            if outer.unless { "unless" } else { "if" }
        );
        let me = span(node);
        let branch_span = span(&branch_node);
        let mut state = std::mem::take(cx.state::<State>());
        let ignored = state.ignored.contains(&me);
        cx.add_fix(branch.keyword.0, branch.keyword.1, msg, |c| {
            if ignored {
                return;
            }
            apply(c, &edits);
            state.ignored.insert(branch_span);
        });
        *cx.state::<State>() = state;
    }
}

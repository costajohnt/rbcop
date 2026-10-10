use crate::prelude::*;
use ruby_prism::StatementsNode;

pub const NAME: &str = "Style/OrAssignment";
pub static COP: OrAssignment = OrAssignment;
pub struct OrAssignment;

const MSG: &str = "Use the double pipe equals operator `||=` instead.";

impl Cop for OrAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::IF_NODE,
            kind::UNLESS_NODE,
            kind::LOCAL_VARIABLE_WRITE_NODE,
            kind::INSTANCE_VARIABLE_WRITE_NODE,
            kind::CLASS_VARIABLE_WRITE_NODE,
            kind::GLOBAL_VARIABLE_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some((name, value)) = var_write(node) {
            // ({lvasgn ivasgn cvasgn gvasgn} _var (if ({lvar ...} _var) ({lvar ...} _var) $_))
            let Some((cond, then, other)) = if_parts(&value) else { return };
            // The correction uses rubocop-ast's `else_branch`, which for `unless` is the else clause.
            let then_range = then.as_ref().map(default_range);
            if var_read(&cond) != Some(name) || single(then).and_then(|t| var_read(&t)) != Some(name) {
                return;
            }
            // else_branch.if_type? raises NoMethodError on a missing else: no offense.
            let Some(other) = other else { return };
            let Some(default) = parser_body(&other) else { return };
            if matches!(kind::of(&default), kind::IF_NODE | kind::UNLESS_NODE) {
                return;
            }
            let (ds, de) = match then_range.filter(|_| value.as_unless_node().is_some()) {
                Some(r) => r,
                None => default_range(&other),
            };
            let fix = format!("{} ||= {}", String::from_utf8_lossy(name), cx.src.slice(ds, de));
            let loc = node.location();
            let (s, e) = (loc.start_offset(), loc.end_offset());
            return cx.add_fix(s, e, MSG, |c| c.replace(s, e, &fix));
        }
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        // (if ({lvar ivar cvar gvar} _var) nil? ({lvasgn ivasgn cvasgn gvasgn} _var _))
        let Some((cond, then, other)) = if_parts(node) else { return };
        if then.as_ref().and_then(parser_body).is_some() {
            return;
        }
        let Some(name) = var_read(&cond) else { return };
        let Some(asgn) = other.as_ref().and_then(parser_body) else { return };
        let Some((asgn_name, value)) = var_write(&asgn).filter(|(n, _)| *n == name) else { return };
        let fix = format!("{} ||= {}", String::from_utf8_lossy(asgn_name), node_text(&value));
        let (s, e) = (node.location().start_offset(), helpers::if_node::parser_end(node));
        cx.add_fix(s, e, MSG, |c| c.replace(s, e, &fix));
    }
}

fn var_write<'pr>(node: &Node<'pr>) -> Option<(&'pr [u8], Node<'pr>)> {
    if let Some(n) = node.as_local_variable_write_node() {
        return Some((n.name().as_slice(), n.value()));
    }
    if let Some(n) = node.as_instance_variable_write_node() {
        return Some((n.name().as_slice(), n.value()));
    }
    if let Some(n) = node.as_class_variable_write_node() {
        return Some((n.name().as_slice(), n.value()));
    }
    let n = node.as_global_variable_write_node()?;
    Some((n.name().as_slice(), n.value()))
}

fn var_read<'pr>(node: &Node<'pr>) -> Option<&'pr [u8]> {
    if let Some(n) = node.as_local_variable_read_node() {
        return Some(n.name().as_slice());
    }
    if let Some(n) = node.as_instance_variable_read_node() {
        return Some(n.name().as_slice());
    }
    if let Some(n) = node.as_class_variable_read_node() {
        return Some(n.name().as_slice());
    }
    Some(node.as_global_variable_read_node()?.name().as_slice())
}

/// Parser `(if cond then else)` parts of an if/unless (unless swaps its branches). The branches
/// are Prism statement holders: a StatementsNode, or an ElseNode / elsif IfNode for `else`.
fn if_parts<'pr>(node: &Node<'pr>) -> Option<(Node<'pr>, Option<Node<'pr>>, Option<Node<'pr>>)> {
    if let Some(n) = node.as_if_node() {
        return Some((n.predicate(), n.statements().map(|s| s.as_node()), n.subsequent()));
    }
    let n = node.as_unless_node()?;
    Some((n.predicate(), n.else_clause().map(|e| e.as_node()), n.statements().map(|s| s.as_node())))
}

fn statements<'pr>(node: &Node<'pr>) -> Option<StatementsNode<'pr>> {
    match node.as_else_node() {
        Some(e) => e.statements(),
        None => node.as_statements_node(),
    }
}

/// The single statement of a branch holder.
fn single<'pr>(node: Option<Node<'pr>>) -> Option<Node<'pr>> {
    let s = statements(&node?)?;
    let body: Vec<_> = s.body().iter().collect();
    if body.len() == 1 { body.into_iter().next() } else { None }
}

/// The parser node of a branch: its single statement, the `begin` of several (represented by
/// the last), or an elsif IfNode. None when empty.
fn parser_body<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    if node.as_if_node().is_some() {
        return Some(node.as_if_node().unwrap().as_node());
    }
    let s = statements(node)?;
    let body: Vec<_> = s.body().iter().collect();
    match body.len() {
        0 => None,
        1 => body.into_iter().next(),
        _ => Some(s.as_node()),
    }
}

fn default_range(node: &Node) -> (usize, usize) {
    match statements(node) {
        Some(s) if node.as_if_node().is_none() => (s.location().start_offset(), s.location().end_offset()),
        _ => (node.location().start_offset(), helpers::if_node::parser_end(node)),
    }
}

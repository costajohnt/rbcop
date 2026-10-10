use crate::helpers::node_types::is_operator_method;
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/InvertibleUnlessCondition";
pub static COP: InvertibleUnlessCondition = InvertibleUnlessCondition;
pub struct InvertibleUnlessCondition;

type Inverses = Vec<(String, String)>;

fn inverse<'a>(inv: &'a Inverses, name: &str) -> Option<&'a str> {
    inv.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// The single statement of a parenthesised `begin` (`children.one?`).
fn begin_child<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let body = node.as_parentheses_node()?.body()?;
    match body.as_statements_node() {
        Some(s) => {
            let mut it = s.body().iter();
            let first = it.next()?;
            it.next().is_none().then_some(first)
        }
        None => Some(body),
    }
}

/// A parser `send` (not csend, no literal block, not `match_with_lvasgn`).
fn send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let c = node.as_call_node()?;
    if is_csend(&c) || c.block().is_some_and(|b| b.as_block_node().is_some()) {
        return None;
    }
    if method_name(&c) == "=~" && c.receiver().is_some_and(|r| r.as_regular_expression_node().is_some()) {
        return None;
    }
    Some(c)
}

/// Parser arguments of a send, a block pass included.
fn arguments<'pr>(c: &CallNode<'pr>) -> Vec<Node<'pr>> {
    let mut args = call_args(c);
    args.extend(c.block().filter(|b| b.as_block_argument_node().is_some()));
    args
}

/// (lhs, rhs, operator range, inverse operator, is `or`)
type Logical<'pr> = (Node<'pr>, Node<'pr>, (usize, usize), &'static str, bool);

fn logical<'pr>(node: &Node<'pr>) -> Option<Logical<'pr>> {
    let range = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
    if let Some(a) = node.as_and_node() {
        let op = a.operator_loc();
        let inv = if op.as_slice() == b"&&" { "||" } else { "or" };
        return Some((a.left(), a.right(), range(op), inv, false));
    }
    let o = node.as_or_node()?;
    let op = o.operator_loc();
    let inv = if op.as_slice() == b"||" { "&&" } else { "and" };
    Some((o.left(), o.right(), range(op), inv, true))
}

/// `invertible?`; None where RuboCop raises (`<` without an argument).
fn invertible(node: &Node, inv: &Inverses) -> Option<bool> {
    if node.as_parentheses_node().is_some() {
        return match begin_child(node) {
            Some(c) => invertible(&c, inv),
            None => Some(false),
        };
    }
    if let Some((l, r, ..)) = logical(node) {
        return Some(invertible(&l, inv)? && invertible(&r, inv)?);
    }
    let Some(c) = send(node) else { return Some(false) };
    let name = method_name(&c);
    if name == "<" {
        // inheritance_check?: `argument.const_type?` on a missing argument raises
        let arg = arguments(&c).into_iter().next()?;
        let short = match (arg.as_constant_read_node(), arg.as_constant_path_node()) {
            (Some(r), _) => Some(r.name().as_slice().to_vec()),
            (_, Some(p)) => p.name().map(|n| n.as_slice().to_vec()),
            _ => None,
        };
        if short.is_some_and(|s| s.to_ascii_uppercase() != s) {
            return Some(false);
        }
    }
    Some(name == "!" || inverse(inv, name).is_some())
}

fn preferred(node: &Node, inv: &Inverses) -> String {
    if node.as_parentheses_node().is_some() {
        return format!("({})", preferred(&begin_child(node).unwrap(), inv));
    }
    if let Some((l, r, _, op, is_or)) = logical(node) {
        let operand = |n: &Node| {
            let p = preferred(n, inv);
            if is_or && n.as_and_node().is_some() { format!("({p})") } else { p }
        };
        return format!("{} {op} {}", operand(&l), operand(&r));
    }
    let c = send(node).unwrap();
    let recv = c.receiver().map(|r| node_text(&r));
    let name = method_name(&c);
    if name == "!" {
        return recv.unwrap_or("").to_string();
    }
    let dotted = recv.map_or(String::new(), |r| format!("{r}."));
    let inverse = inverse(inv, name).unwrap_or("");
    let args = arguments(&c);
    if args.is_empty() {
        return format!("{dotted}{inverse}");
    }
    let list = args.iter().map(node_text).collect::<Vec<_>>().join(", ");
    if is_operator_method(name) {
        return format!("{} {inverse} {list}", recv.unwrap_or(""));
    }
    if c.opening_loc().is_some() {
        return format!("{dotted}{inverse}({list})");
    }
    format!("{dotted}{inverse} {list}")
}

enum Edit {
    Replace(usize, usize, String),
    Remove(usize, usize),
    Wrap(usize, usize),
}

fn edits(node: &Node, inv: &Inverses, out: &mut Vec<Edit>) {
    if node.as_parentheses_node().is_some() {
        return edits(&begin_child(node).unwrap(), inv, out);
    }
    if let Some((l, r, op, inverse_op, is_or)) = logical(node) {
        out.push(Edit::Replace(op.0, op.1, inverse_op.to_string()));
        for operand in [l, r] {
            edits(&operand, inv, out);
            if is_or && operand.as_and_node().is_some() {
                out.push(Edit::Wrap(operand.location().start_offset(), operand.location().end_offset()));
            }
        }
        return;
    }
    let c = send(node).unwrap();
    let Some(sel) = c.message_loc() else { return };
    let name = method_name(&c);
    if name == "!" {
        out.push(Edit::Remove(sel.start_offset(), sel.end_offset()));
    } else {
        out.push(Edit::Replace(sel.start_offset(), sel.end_offset(), inverse(inv, name).unwrap_or("").to_string()));
    }
}

impl Cop for InvertibleUnlessCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let unless = node.as_unless_node().unwrap();
        let Some(Value::Map(m)) = cx.cfg.get("InverseMethods") else { return };
        let sym = |s: &str| s.strip_prefix(':').unwrap_or(s).to_string();
        let inv: Inverses = m.0.iter().filter_map(|(k, v)| Some((sym(k), sym(&v.as_string()?)))).collect();
        let condition = unless.predicate();
        if invertible(&condition, &inv) != Some(true) {
            return;
        }
        let msg = format!("Prefer `if {}` over `unless {}`.", preferred(&condition, &inv), node_text(&condition));
        let kw = unless.keyword_loc();
        let mut list = vec![Edit::Replace(kw.start_offset(), kw.end_offset(), "if".to_string())];
        edits(&condition, &inv, &mut list);
        let loc = node.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| {
            for e in &list {
                match e {
                    Edit::Replace(a, b, s) => c.replace(*a, *b, s),
                    Edit::Remove(a, b) => c.remove(*a, *b),
                    Edit::Wrap(a, b) => c.wrap(*a, *b, "(", ")"),
                }
            }
        });
    }
}

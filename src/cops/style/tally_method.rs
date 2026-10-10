use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;
use ruby_prism::{BlockNode, CallNode};

pub const NAME: &str = "Style/TallyMethod";
pub static COP: TallyMethod = TallyMethod;
pub struct TallyMethod;

const MSG_EACH_WITH_OBJECT: &str = "Use `tally` instead of `each_with_object`.";
const MSG_GROUP_BY: &str = "Use `tally` instead of `group_by` and `transform_values`.";

/// The block's shape: `|a, b|` plain args (names), numbered params (max), or `it`.
enum Params<'pr> {
    Args(Vec<&'pr [u8]>),
    Numbered(u8),
    It,
}

fn params<'pr>(block: &BlockNode<'pr>) -> Option<Params<'pr>> {
    let Some(p) = block.parameters() else { return Some(Params::Args(vec![])) };
    if let Some(n) = p.as_numbered_parameters_node() {
        return Some(Params::Numbered(n.maximum()));
    }
    if p.as_it_parameters_node().is_some() {
        return Some(Params::It);
    }
    let bp = p.as_block_parameters_node()?;
    if bp.locals().iter().next().is_some() {
        return None;
    }
    let Some(ps) = bp.parameters() else { return Some(Params::Args(vec![])) };
    let nodes = parameter_nodes(&ps);
    let names = nodes
        .iter()
        .map(|n| n.as_required_parameter_node().map(|r| r.name().as_slice()))
        .collect::<Option<Vec<_>>>()?;
    Some(Params::Args(names))
}

/// The single statement of a block body.
fn body<'pr>(block: &BlockNode<'pr>) -> Option<Node<'pr>> {
    let s = block.body()?.as_statements_node()?;
    let mut it = s.body().iter();
    let n = it.next()?;
    it.next().is_none().then_some(n)
}

/// `(lvar name)` (also `it`).
fn is_lvar(node: &Node, name: &[u8]) -> bool {
    if name == b"it" && node.as_it_local_variable_read_node().is_some() {
        return true;
    }
    node.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == name)
}

fn block_of<'pr>(call: &CallNode<'pr>) -> Option<BlockNode<'pr>> {
    call.block().and_then(|b| b.as_block_node())
}

fn plain_send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c))
}

/// `(sym name)` block pass as the call's only argument.
fn block_pass_sym<'pr>(call: &CallNode<'pr>) -> Option<&'pr [u8]> {
    if call.arguments().is_some() {
        return None;
    }
    let ba = call.block()?.as_block_argument_node()?;
    let sym = ba.expression()?.as_symbol_node()?;
    sym.value_loc().map(|v| v.as_slice())
}

fn counting(name: &[u8]) -> bool {
    matches!(name, b"count" | b"size" | b"length")
}

/// `tally_each_with_object?` on the block of an `each_with_object` call.
fn each_with_object_block(call: &CallNode) -> bool {
    let Some(block) = block_of(call) else { return false };
    let args = call_args(call);
    let [arg] = args.as_slice() else { return false };
    let Some(new) = plain_send(arg) else { return false };
    if method_name(&new) != "new"
        || new.block().is_some()
        || !new.receiver().is_some_and(|r| is_top_const(&r, "Hash"))
        || !matches!(call_args(&new).as_slice(), [z] if z.as_integer_node().is_some() && int_literal_value(z) == Some(0))
    {
        return false;
    }
    let Some(b) = body(&block) else { return false };
    let Some(op) = b.as_index_operator_write_node() else { return false };
    if op.binary_operator().as_slice() != b"+"
        || op.block().is_some()
        || is_csend_loc(op.call_operator_loc())
        || !(op.value().as_integer_node().is_some() && int_literal_value(&op.value()) == Some(1))
    {
        return false;
    }
    let idx: Vec<Node> = op.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
    let [elem] = idx.as_slice() else { return false };
    let Some(recv) = op.receiver() else { return false };
    let (hash, el): (&[u8], &[u8]) = match params(&block) {
        Some(Params::Args(names)) if names.len() == 2 => (names[1], names[0]),
        Some(Params::Numbered(2)) => (b"_2", b"_1"),
        _ => return false,
    };
    is_lvar(&recv, hash) && is_lvar(elem, el)
}

fn is_csend_loc(loc: Option<ruby_prism::Location>) -> bool {
    loc.is_some_and(|l| l.as_slice() == b"&.")
}

/// `(block (call _ :group_by) (args (arg _x)) (lvar _x))` and the numblock/itblock forms.
fn identity_group_by(call: &CallNode) -> bool {
    if method_name(call) != "group_by" || call.arguments().is_some() {
        return false;
    }
    let Some(block) = block_of(call) else { return false };
    let Some(b) = body(&block) else { return false };
    match params(&block) {
        Some(Params::Args(names)) if names.len() == 1 => is_lvar(&b, names[0]),
        Some(Params::Numbered(1)) => is_lvar(&b, b"_1"),
        Some(Params::It) => is_lvar(&b, b"it"),
        _ => false,
    }
}

/// The receiver of `transform_values` is a group_by by identity: the group_by call.
fn group_by_receiver<'pr>(recv: &Node<'pr>) -> Option<CallNode<'pr>> {
    let call = recv.as_call_node()?;
    if method_name(&call) != "group_by" {
        return None;
    }
    if block_pass_sym(&call) == Some(b"itself") || identity_group_by(&call) {
        return Some(call);
    }
    None
}

impl Cop for TallyMethod {
    fn correctable(&self) -> bool {
        true
    }
    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        match method_name(&call) {
            "each_with_object" => {
                if !each_with_object_block(&call) {
                    return;
                }
                let Some(sel) = call.message_loc() else { return };
                let (s, e) = (sel.start_offset(), sel.end_offset());
                let end = node.location().end_offset();
                cx.add_fix(s, e, MSG_EACH_WITH_OBJECT, |c| c.replace(s, end, "tally"));
            }
            "transform_values" => {
                let Some(recv) = call.receiver() else { return };
                let Some(group_by) = group_by_receiver(&recv) else { return };
                let end = if call.arguments().is_none() && block_pass_sym(&call).is_some_and(counting) {
                    send_range(&call).1
                } else if let Some(block) = block_of(&call).filter(|_| call.arguments().is_none()) {
                    let Some(b) = body(&block) else { return };
                    let Some(send) = plain_send(&b) else { return };
                    if !counting(send.name().as_slice()) || send.arguments().is_some() || send.block().is_some() {
                        return;
                    }
                    let Some(r) = send.receiver() else { return };
                    let ok = match params(&block) {
                        Some(Params::Args(names)) if names.len() == 1 => is_lvar(&r, names[0]),
                        Some(Params::Numbered(1)) => is_lvar(&r, b"_1"),
                        Some(Params::It) => is_lvar(&r, b"it"),
                        _ => false,
                    };
                    if !ok {
                        return;
                    }
                    node.location().end_offset()
                } else {
                    return;
                };
                let Some(sel) = group_by.message_loc() else { return };
                let (s, e) = (sel.start_offset(), sel.end_offset());
                cx.add_fix(s, e, MSG_GROUP_BY, |c| c.replace(s, end, "tally"));
            }
            _ => {}
        }
    }
}

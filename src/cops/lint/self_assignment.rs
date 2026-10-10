use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Lint/SelfAssignment";
pub static COP: SelfAssignment = SelfAssignment;
pub struct SelfAssignment;

// ponytail: AllowRBSInlineAnnotation (default false) is not ported; it needs comment association.

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// Optional nodes compared structurally (`nil == nil`).
fn same_opt(src: &Source, a: Option<Node>, b: Option<Node>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => helpers::node_equality::same(src, range(&a), range(&b)),
        _ => false,
    }
}

fn same_list(src: &Source, a: &[Node], b: &[Node]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| helpers::node_equality::same(src, range(x), range(y)))
}

/// A parser `send`/`csend` (a call carrying a `{}`/`do` block is a parser `block`).
fn plain_call<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| !c.block().is_some_and(|b| b.as_block_node().is_some()))
}

/// Parser arguments of a call: positional ones plus a `&blk` block pass.
fn args_of<'pr>(call: &ruby_prism::CallNode<'pr>) -> Vec<Node<'pr>> {
    let mut args = call_args(call);
    args.extend(call.block().filter(|b| b.as_block_argument_node().is_some()));
    args
}

fn is_call_type(node: &Node) -> bool {
    plain_call(node).is_some()
}

/// Name a variable read must have to match an assignment target (lvasgn -> lvar etc.).
fn target_name<'pr>(node: &Node<'pr>) -> Option<(usize, &'pr [u8])> {
    Some(match kind::of(node) {
        kind::LOCAL_VARIABLE_TARGET_NODE => (0, node.as_local_variable_target_node().unwrap().name().as_slice()),
        kind::INSTANCE_VARIABLE_TARGET_NODE => (1, node.as_instance_variable_target_node().unwrap().name().as_slice()),
        kind::CLASS_VARIABLE_TARGET_NODE => (2, node.as_class_variable_target_node().unwrap().name().as_slice()),
        kind::GLOBAL_VARIABLE_TARGET_NODE => (3, node.as_global_variable_target_node().unwrap().name().as_slice()),
        _ => return None,
    })
}

fn read_name<'pr>(node: &Node<'pr>) -> Option<(usize, &'pr [u8])> {
    Some(match kind::of(node) {
        kind::LOCAL_VARIABLE_READ_NODE => (0, node.as_local_variable_read_node().unwrap().name().as_slice()),
        kind::INSTANCE_VARIABLE_READ_NODE => (1, node.as_instance_variable_read_node().unwrap().name().as_slice()),
        kind::CLASS_VARIABLE_READ_NODE => (2, node.as_class_variable_read_node().unwrap().name().as_slice()),
        kind::GLOBAL_VARIABLE_READ_NODE => (3, node.as_global_variable_read_node().unwrap().name().as_slice()),
        _ => return None,
    })
}

/// Namespace of a constant: 0 = none, 1 = cbase, 2 = a parent node.
fn same_namespace(src: &Source, a: Option<Option<Node>>, b: Option<Option<Node>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same_opt(src, a, b),
        _ => false,
    }
}

/// (namespace, short name) of a const read: namespace None for `Foo`, Some(None) for `::Foo`.
fn const_parts<'pr>(node: &Node<'pr>) -> Option<(Option<Option<Node<'pr>>>, &'pr [u8])> {
    if let Some(c) = node.as_constant_read_node() {
        return Some((None, c.name().as_slice()));
    }
    let p = node.as_constant_path_node()?;
    Some((Some(p.parent()), p.name()?.as_slice()))
}

fn const_self_assignment(src: &Source, lhs: (Option<Option<Node>>, &[u8]), rhs: &Node) -> bool {
    const_parts(rhs).is_some_and(|(ns, name)| name == lhs.1 && same_namespace(src, lhs.0, ns))
}

/// `reader_self_assignment?` for `recv.name ||= rhs` / `recv[args] ||= rhs`.
fn reader_self_assignment(src: &Source, csend: bool, recv: Option<Node>, name: &[u8], args: &[Node], rhs: &Node) -> bool {
    let Some(r) = plain_call(rhs) else { return false };
    is_csend(&r) == csend
        && r.name().as_slice() == name
        && same_opt(src, recv, r.receiver())
        && same_list(src, args, &args_of(&r))
        && !args.iter().any(is_call_type)
}

impl SelfAssignment {
    fn on_send(cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let args = args_of(&call);
        let src = cx.src;
        let offense = if name == "[]=" {
            let Some((value, keys)) = args.split_last() else { return };
            plain_call(value).is_some_and(|v| {
                method_name(&v) == "[]"
                    && same_opt(src, call.receiver(), v.receiver())
                    && !keys.iter().any(is_call_type)
                    && same_list(src, keys, &args_of(&v))
            })
        } else if name.ends_with('=') && !matches!(name, "==" | "===" | "!=" | "<=" | ">=") {
            if args.len() != 1 {
                return;
            }
            // respond_to?(:arguments) && arguments.empty?, then call_type?
            plain_call(&args[0]).is_some_and(|v| {
                args_of(&v).is_empty()
                    && same_opt(src, call.receiver(), v.receiver())
                    && method_name(&v) == &name[..name.len() - 1]
            })
        } else {
            return;
        };
        if offense {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Self-assignment detected.");
        }
    }
}

impl Cop for SelfAssignment {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::LOCAL_VARIABLE_WRITE_NODE,
            kind::INSTANCE_VARIABLE_WRITE_NODE,
            kind::CLASS_VARIABLE_WRITE_NODE,
            kind::GLOBAL_VARIABLE_WRITE_NODE,
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::MULTI_WRITE_NODE,
            kind::LOCAL_VARIABLE_OR_WRITE_NODE,
            kind::LOCAL_VARIABLE_AND_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
            kind::CLASS_VARIABLE_OR_WRITE_NODE,
            kind::CLASS_VARIABLE_AND_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_AND_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
            kind::CONSTANT_PATH_AND_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::CALL_AND_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::INDEX_AND_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        let var = |k: usize, name: &[u8], value: &Node| read_name(value).is_some_and(|(rk, rn)| rk == k && rn == name);
        let offense = match kind::of(node) {
            kind::CALL_NODE => return Self::on_send(cx, node),
            kind::LOCAL_VARIABLE_WRITE_NODE => {
                let n = node.as_local_variable_write_node().unwrap();
                var(0, n.name().as_slice(), &n.value())
            }
            kind::INSTANCE_VARIABLE_WRITE_NODE => {
                let n = node.as_instance_variable_write_node().unwrap();
                var(1, n.name().as_slice(), &n.value())
            }
            kind::CLASS_VARIABLE_WRITE_NODE => {
                let n = node.as_class_variable_write_node().unwrap();
                var(2, n.name().as_slice(), &n.value())
            }
            kind::GLOBAL_VARIABLE_WRITE_NODE => {
                let n = node.as_global_variable_write_node().unwrap();
                var(3, n.name().as_slice(), &n.value())
            }
            kind::LOCAL_VARIABLE_OR_WRITE_NODE => {
                let n = node.as_local_variable_or_write_node().unwrap();
                var(0, n.name().as_slice(), &n.value())
            }
            kind::LOCAL_VARIABLE_AND_WRITE_NODE => {
                let n = node.as_local_variable_and_write_node().unwrap();
                var(0, n.name().as_slice(), &n.value())
            }
            kind::INSTANCE_VARIABLE_OR_WRITE_NODE => {
                let n = node.as_instance_variable_or_write_node().unwrap();
                var(1, n.name().as_slice(), &n.value())
            }
            kind::INSTANCE_VARIABLE_AND_WRITE_NODE => {
                let n = node.as_instance_variable_and_write_node().unwrap();
                var(1, n.name().as_slice(), &n.value())
            }
            kind::CLASS_VARIABLE_OR_WRITE_NODE => {
                let n = node.as_class_variable_or_write_node().unwrap();
                var(2, n.name().as_slice(), &n.value())
            }
            kind::CLASS_VARIABLE_AND_WRITE_NODE => {
                let n = node.as_class_variable_and_write_node().unwrap();
                var(2, n.name().as_slice(), &n.value())
            }
            kind::GLOBAL_VARIABLE_OR_WRITE_NODE => {
                let n = node.as_global_variable_or_write_node().unwrap();
                var(3, n.name().as_slice(), &n.value())
            }
            kind::GLOBAL_VARIABLE_AND_WRITE_NODE => {
                let n = node.as_global_variable_and_write_node().unwrap();
                var(3, n.name().as_slice(), &n.value())
            }
            kind::CONSTANT_WRITE_NODE => {
                let n = node.as_constant_write_node().unwrap();
                const_self_assignment(src, (None, n.name().as_slice()), &n.value())
            }
            kind::CONSTANT_OR_WRITE_NODE => {
                let n = node.as_constant_or_write_node().unwrap();
                const_self_assignment(src, (None, n.name().as_slice()), &n.value())
            }
            kind::CONSTANT_AND_WRITE_NODE => {
                let n = node.as_constant_and_write_node().unwrap();
                const_self_assignment(src, (None, n.name().as_slice()), &n.value())
            }
            kind::CONSTANT_PATH_WRITE_NODE => {
                let n = node.as_constant_path_write_node().unwrap();
                let t = n.target();
                t.name().is_some_and(|name| const_self_assignment(src, (Some(t.parent()), name.as_slice()), &n.value()))
            }
            kind::CONSTANT_PATH_OR_WRITE_NODE => {
                let n = node.as_constant_path_or_write_node().unwrap();
                let t = n.target();
                t.name().is_some_and(|name| const_self_assignment(src, (Some(t.parent()), name.as_slice()), &n.value()))
            }
            kind::CONSTANT_PATH_AND_WRITE_NODE => {
                let n = node.as_constant_path_and_write_node().unwrap();
                let t = n.target();
                t.name().is_some_and(|name| const_self_assignment(src, (Some(t.parent()), name.as_slice()), &n.value()))
            }
            kind::CALL_OR_WRITE_NODE => {
                let n = node.as_call_or_write_node().unwrap();
                let csend = n.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.");
                reader_self_assignment(src, csend, n.receiver(), n.read_name().as_slice(), &[], &n.value())
            }
            kind::CALL_AND_WRITE_NODE => {
                let n = node.as_call_and_write_node().unwrap();
                let csend = n.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.");
                reader_self_assignment(src, csend, n.receiver(), n.read_name().as_slice(), &[], &n.value())
            }
            kind::INDEX_OR_WRITE_NODE => {
                let n = node.as_index_or_write_node().unwrap();
                let csend = n.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.");
                let mut args: Vec<Node> = n.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
                args.extend(n.block().map(|b| b.as_node()));
                reader_self_assignment(src, csend, n.receiver(), b"[]", &args, &n.value())
            }
            kind::INDEX_AND_WRITE_NODE => {
                let n = node.as_index_and_write_node().unwrap();
                let csend = n.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.");
                let mut args: Vec<Node> = n.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
                args.extend(n.block().map(|b| b.as_node()));
                reader_self_assignment(src, csend, n.receiver(), b"[]", &args, &n.value())
            }
            kind::MULTI_WRITE_NODE => {
                let n = node.as_multi_write_node().unwrap();
                let mut lhs: Vec<Node> = n.lefts().iter().collect();
                lhs.extend(n.rest().filter(|r| kind::of(r) != kind::IMPLICIT_REST_NODE));
                lhs.extend(n.rights().iter());
                n.value().as_array_node().is_some_and(|a| {
                    let rhs: Vec<Node> = a.elements().iter().collect();
                    lhs.len() == rhs.len()
                        && lhs.iter().zip(&rhs).all(|(l, r)| target_name(l).is_some_and(|t| read_name(r) == Some(t)))
                })
            }
            _ => false,
        };
        if offense {
            let (s, e) = range(node);
            cx.add(s, e, "Self-assignment detected.");
        }
    }
}

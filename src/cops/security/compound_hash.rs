use crate::prelude::*;

pub const NAME: &str = "Security/CompoundHash";
pub static COP: CompoundHash = CompoundHash;
pub struct CompoundHash;

const COMBINATOR_IN_HASH_MSG: &str = "Use `[...].hash` instead of combining hash values manually.";
const MONUPLE_HASH_MSG: &str = "Delegate hash directly without wrapping in an array when only using a single value.";
const REDUNDANT_HASH_MSG: &str = "Calling .hash on elements of a hashed array is redundant.";

fn combinator_op(op: &[u8]) -> bool {
    matches!(op, b"^" | b"+" | b"*" | b"|")
}

/// Operator of a parser `op_asgn` node.
fn op_asgn_operator<'pr>(node: &Node<'pr>) -> Option<&'pr [u8]> {
    macro_rules! op {
        ($($as:ident),*) => {
            $(if let Some(n) = node.$as() {
                return Some(n.binary_operator().as_slice());
            })*
        };
    }
    op!(
        as_local_variable_operator_write_node,
        as_instance_variable_operator_write_node,
        as_class_variable_operator_write_node,
        as_global_variable_operator_write_node,
        as_constant_operator_write_node,
        as_constant_path_operator_write_node,
        as_call_operator_write_node,
        as_index_operator_write_node
    );
    None
}

/// `({send | op-asgn} _ {:^ | :+ | :* | :|} _)`
fn bad_hash_combinator(node: &Node) -> bool {
    if let Some(call) = node.as_call_node() {
        return !is_csend(&call) && combinator_op(call.name().as_slice()) && send_args(&call).len() == 1;
    }
    op_asgn_operator(node).is_some_and(combinator_op)
}

/// `({def | defs _} :hash (args) _)` or `(block (send _ {:define_method :define_singleton_method} (sym :hash)) (args) _)`
fn hash_method_definition(node: &Node) -> bool {
    if let Some(def) = node.as_def_node() {
        return def.name().as_slice() == b"hash" && def.parameters().is_none_or(|p| parameter_nodes(&p).is_empty());
    }
    let Some(call) = node.as_call_node() else { return false };
    let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return false };
    let empty_params = match block.parameters() {
        None => true,
        Some(p) => p.as_block_parameters_node().is_some_and(|bp| bp.parameters().is_none() && bp.locals().iter().next().is_none()),
    };
    let args = call_args(&call);
    empty_params
        && !is_csend(&call)
        && matches!(method_name(&call), "define_method" | "define_singleton_method")
        && args.len() == 1
        && args[0].as_symbol_node().is_some_and(|s| s.unescaped() == b"hash")
}

fn same_node(a: &Node, b: &Node) -> bool {
    let (a, b) = (a.location(), b.location());
    (a.start_offset(), a.end_offset()) == (b.start_offset(), b.end_offset())
}

impl Cop for CompoundHash {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CONSTANT_OPERATOR_WRITE_NODE,
            kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (start, end) = match node.as_call_node() {
            Some(call) => send_range(&call),
            None => (node.location().start_offset(), node.location().end_offset()),
        };
        let anc = cx.ancestors;
        if bad_hash_combinator(node) {
            // Parser ancestors: a call is not an ancestor of the block it owns.
            let outer = !(0..anc.len()).any(|i| {
                bad_hash_combinator(&anc[i]) && !anc.get(i + 1).is_some_and(|n| n.as_block_node().is_some())
            });
            if outer && anc.iter().any(hash_method_definition) {
                cx.add(start, end, COMBINATOR_IN_HASH_MSG);
            }
        }
        let Some(call) = node.as_call_node() else { return };
        if call.name().as_slice() != b"hash" || !send_args(&call).is_empty() {
            return;
        }
        // (send (array _) :hash)
        let monuple = !is_csend(&call)
            && call.receiver().and_then(|r| r.as_array_node()).is_some_and(|a| a.elements().iter().count() == 1);
        if monuple {
            cx.add(start, end, MONUPLE_HASH_MSG);
        }
        // (^^(send array ... :hash) _ :hash)
        let n = anc.len();
        // (an identical range keeps only the first offense)
        if n >= 2
            && !monuple
            && call.block().is_none()
            && let Some(array) = anc[n - 1].as_array_node()
            && let Some(outer) = anc[n - 2].as_call_node()
            && !is_csend(&outer)
            && outer.name().as_slice() == b"hash"
            && outer.arguments().is_none()
            && outer.block().is_none_or(|b| b.as_block_node().is_some())
            && outer.receiver().is_some_and(|r| same_node(&r, &array.as_node()))
        {
            cx.add(start, end, REDUNDANT_HASH_MSG);
        }
    }
}

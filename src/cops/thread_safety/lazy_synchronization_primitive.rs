use crate::helpers::node_types::plain_block_params;
use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "ThreadSafety/LazySynchronizationPrimitive";
pub static COP: LazySynchronizationPrimitive = LazySynchronizationPrimitive;
pub struct LazySynchronizationPrimitive;

/// `(send (const {nil? cbase} {:Mutex :Monitor}) :new ...)` / `(send (const (const nil? :Thread) :Mutex) :new ...)`
fn synchronization_primitive(node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if method_name(&call) != "new" || is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return false;
    }
    let Some(r) = call.receiver() else { return false };
    is_top_const(&r, "Mutex")
        || is_top_const(&r, "Monitor")
        || r.as_constant_path_node().is_some_and(|p| {
            p.name().is_some_and(|n| n.as_slice() == b"Mutex")
                && p.parent().is_some_and(|q| q.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Thread"))
        })
}

impl Cop for LazySynchronizationPrimitive {
    fn kinds(&self) -> &'static [usize] {
        &[kind::INSTANCE_VARIABLE_OR_WRITE_NODE, kind::CLASS_VARIABLE_OR_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let value = match node {
            Node::InstanceVariableOrWriteNode { .. } => node.as_instance_variable_or_write_node().unwrap().value(),
            _ => node.as_class_variable_or_write_node().unwrap().value(),
        };
        if !synchronization_primitive(&value) || !cx.ancestors.iter().any(|a| a.as_def_node().is_some()) {
            return;
        }
        // synchronized?: a `block` ancestor whose method is `synchronize`.
        let synchronized = cx.ancestors.iter().enumerate().any(|(i, a)| {
            a.as_block_node().is_some_and(|b| plain_block_params(b.parameters()))
                && i > 0
                && cx.ancestors[i - 1].as_call_node().is_some_and(|c| method_name(&c) == "synchronize")
        });
        if !synchronized {
            let l = node.location();
            cx.add(l.start_offset(), l.end_offset(), "Do not lazily initialize synchronization primitives with `||=`.");
        }
    }
}

use crate::prelude::*;

pub const NAME: &str = "Performance/Size";
pub static COP: Size = Size;
pub struct Size;

impl Cop for Size {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call {#array? #hash?} :count)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "count" || !send_args(&call).is_empty() {
            return;
        }
        // `node.parent&.block_type?`: its own `block` (not a numblock/itblock), or one whose
        // body it is.
        if call.block().and_then(|b| b.as_block_node()).is_some_and(|b| plain(&b)) {
            return;
        }
        if call.block().is_none()
            && helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_block_node()).is_some_and(|b| plain(&b))
        {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        if !(collection(&recv, kind::ARRAY_NODE, "to_a", "Array") || collection(&recv, kind::HASH_NODE, "to_h", "Hash")) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (start, end) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(start, end, "Use `size` instead of `count`.", |c| c.replace(start, end, "size"));
    }
}

fn plain(b: &ruby_prism::BlockNode) -> bool {
    !b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
}

/// array?/hash?: a literal, `x.to_a`, `Array[x]`, `Array(x)` (and the Hash forms).
fn collection(node: &Node, literal: usize, conversion: &str, class: &str) -> bool {
    if kind::of(node) == literal {
        return true;
    }
    let Some(call) = node.as_call_node() else { return false };
    if call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return false;
    }
    let args = send_args(&call);
    let name = method_name(&call);
    if name == conversion && args.is_empty() {
        return true;
    }
    if is_csend(&call) || args.len() != 1 {
        return false;
    }
    match call.receiver() {
        // (send (const nil? :Array) :[] _)
        Some(r) => name == "[]" && r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == class.as_bytes()),
        // (send nil? :Array _)
        None => name == class,
    }
}

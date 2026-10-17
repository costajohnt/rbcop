use crate::prelude::*;

pub const NAME: &str = "Performance/ReverseEach";
pub static COP: ReverseEach = ReverseEach;
pub struct ReverseEach;

impl Cop for ReverseEach {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call (call _ :reverse) :each)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "each" || !send_args(&call).is_empty() {
            return;
        }
        let Some(reverse) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&reverse) != "reverse" || !send_args(&reverse).is_empty() || reverse.block().is_some() {
            return;
        }
        if uses_return_value(cx.ancestors) {
            return;
        }
        let start = reverse.message_loc().unwrap().start_offset();
        let end = call.message_loc().unwrap().end_offset();
        cx.add_fix(start, end, "Use `reverse_each` instead of `reverse.each`.", |c| c.replace(start, end, "reverse_each"));
    }
}

/// use_return_value?: any parser ancestor is an assignment, a `send` or a `return`.
fn uses_return_value(anc: &[Node]) -> bool {
    (0..anc.len()).any(|i| {
        let a = &anc[i];
        // A call owning the block we are inside is not a parser ancestor.
        let owns_block = a.as_call_node().is_some() && anc.get(i + 1).is_some_and(|n| n.as_block_node().is_some());
        if owns_block {
            return false;
        }
        a.as_return_node().is_some()
            || is_assignment(a)
            || a.as_call_node().is_some_and(|c| !is_csend(&c) && helpers::literal_node::folded_numeric(a).is_none())
    })
}

use crate::prelude::*;

pub const NAME: &str = "RSpec/Pending";
pub static COP: Pending = Pending;
pub struct Pending;

impl Cop for Pending {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let l = helpers::rspec::lang(cx);
        let n = method_name(&call);
        let has = |list: &helpers::rspec::List| list.has(n);
        let nil_recv = call.receiver().is_none();
        let rspec = helpers::rspec::rspec_recv(&call);
        // pending_block?
        let pending_block = (rspec && has(&l.eg_skipped)) || (nil_recv && (has(&l.ex_skipped) || has(&l.ex_pending)));
        // skippable_example?: (send nil? #Examples.regular _ ...)
        let skippable_example = nil_recv && has(&l.ex_regular) && !send_args(&call).is_empty();
        let skippable = (rspec && has(&l.eg_regular)) || skippable_example;
        let skipped = (skippable && helpers::rspec::skipped_in_metadata(cx, &call))
            || (skippable_example && helpers::rspec::block_of(&call).is_none());
        if pending_block || skipped {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Pending spec found.");
        }
    }
}

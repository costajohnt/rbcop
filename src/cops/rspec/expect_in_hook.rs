use crate::prelude::*;

pub const NAME: &str = "RSpec/ExpectInHook";
pub static COP: ExpectInHook = ExpectInHook;
pub struct ExpectInHook;

impl Cop for ExpectInHook {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::hook(&l, node) {
            return;
        }
        let call = node.as_call_node().unwrap();
        let Some(body) = helpers::rspec::block_body(&call) else { return };
        let hook = method_name(&call);
        for expect in helpers::rspec::expectations(&l, &body) {
            let sel = expect.message_loc().unwrap();
            let msg = format!("Do not use `{}` in `{hook}` hook", method_name(&expect));
            cx.add(sel.start_offset(), sel.end_offset(), msg);
        }
    }
}

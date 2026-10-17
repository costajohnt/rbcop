use crate::prelude::*;

pub const NAME: &str = "RSpec/ExpectInLet";
pub static COP: ExpectInLet = ExpectInLet;
pub struct ExpectInLet;

impl Cop for ExpectInLet {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_block (plain blocks): let? and a body
        let l = helpers::rspec::lang(cx);
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if !helpers::rspec::let_(&l, node) {
            return;
        }
        let Some(body) = helpers::rspec::block_body(&call) else { return };
        for expect in helpers::rspec::expectations(&l, &body) {
            let sel = expect.message_loc().unwrap();
            cx.add(sel.start_offset(), sel.end_offset(), format!("Do not use `{}` in let", method_name(&expect)));
        }
    }
}

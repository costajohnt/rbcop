use crate::helpers::code_length::{Measured, check};
use crate::prelude::*;

pub const NAME: &str = "RSpec/ExampleLength";
pub static COP: ExampleLength = ExampleLength;
pub struct ExampleLength;

impl Cop for ExampleLength {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::example(&l, node) {
            return;
        }
        let body = helpers::rspec::block_body(&node.as_call_node().unwrap());
        let range = helpers::rspec::node_range(node);
        check(cx, Measured::Body { root: node, body }, range, range, |len, max| {
            format!("Example has too many lines. [{len}/{max}]")
        });
    }
}

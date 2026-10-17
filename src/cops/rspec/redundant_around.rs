use crate::prelude::*;

pub const NAME: &str = "RSpec/RedundantAround";
pub static COP: RedundantAround = RedundantAround;
pub struct RedundantAround;

impl Cop for RedundantAround {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "around" {
            return;
        }
        let redundant = match call.block() {
            // (any_block (send _ :around ...) ... (send _ :run))
            Some(b) if b.as_block_node().is_some() => {
                helpers::rspec::parser_body(b.as_block_node().unwrap().body()).is_some_and(|body| {
                    helpers::rspec::plain_send(&body, "run").is_some_and(|r| send_args(&r).is_empty())
                })
            }
            // (send _ :around ... (block-pass (sym :run)))
            Some(b) => b
                .as_block_argument_node()
                .and_then(|ba| ba.expression())
                .is_some_and(|x| helpers::rspec::is_sym(cx, &x) && helpers::rails::sym_value(&x).as_deref() == Some("run")),
            None => false,
        };
        if redundant {
            let (s, e) = helpers::rspec::node_range(node);
            cx.add_fix(s, e, "Remove redundant `around` hook.", |c| c.remove(s, e));
        }
    }
}
